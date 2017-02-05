/*
* Copyright (c) 2016 Alex Crichton
*
* Permission is hereby granted, free of charge, to any
* person obtaining a copy of this software and associated
* documentation files (the "Software"), to deal in the
* Software without restriction, including without
* limitation the rights to use, copy, modify, merge,
* publish, distribute, sublicense, and/or sell copies of
* the Software, and to permit persons to whom the Software
* is furnished to do so, subject to the following
* conditions:
*
* The above copyright notice and this permission notice
* shall be included in all copies or substantial portions
* of the Software.
*
* THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
* ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
* TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
* PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
* SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
* CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
* OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
* IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
* DEALINGS IN THE SOFTWARE.
*
* Mles-support Copyright (c) 2017 Mles developers
*
*/

/* 
 * Mles-example client based on Tokio core-connect example.
 */

extern crate mles_utils;
extern crate futures;
extern crate tokio_core;

use std::{env, process};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::thread;

use futures::{Sink, Future, Stream};
use futures::sync::mpsc;
use tokio_core::reactor::Core;
use tokio_core::io::{Io, EasyBuf, Codec};
use tokio_core::net::TcpStream;
use mles_utils::*;

const KEYL: usize = 8; //key len
const HDRL: usize = 4 + KEYL; //hdr + key len

fn main() {
    // Parse what address we're going to connect to
    let addr = env::args().nth(1).unwrap_or_else(|| {
        println!("Usage: mles-client <server-address>");
        process::exit(1);
    }
    );
    // add port
    let addr = addr + ":8077";
    let addr = match addr.parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(err) => {
            println!("Error: {}\nUsage: mles-client <server-address>", err);
            process::exit(1);
        },
    };

    let keyval =match env::var("MLES_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let tcp = TcpStream::connect(&addr, &handle);
    let mut key = 0;
    
    // Handle stdin in a separate thread 
    let (stdin_tx, stdin_rx) = mpsc::channel(0);
    thread::spawn(|| read_stdin(stdin_tx));
    let stdin_rx = stdin_rx.map_err(|_| panic!()); // errors not possible on rx

    let mut stdout = io::stdout();
    let client = tcp.and_then(|stream| {
        let _val = stream.set_nodelay(true).map_err(|_| panic!("Cannot set to no delay"));
        /* Set key */
        if 0 == keyval.len() {
            key = match stream.local_addr() {
                Ok(laddr) => do_hash(&laddr),
                Err(_) => {
                    panic!("Cannot get local address");
                },
            };
        }
        else {
            key = do_hash(&keyval);
        }
        let (sink, stream) = stream.framed(Bytes).split();
        let stdin_rx = stdin_rx.and_then(|buf| {
            let mut keyv = write_key(key);
            keyv.extend(buf);
            Ok(keyv)
        });
        let send_stdin = stdin_rx.forward(sink);
        let write_stdout = stream.for_each(move |buf| {
            let decoded = message_decode(buf.as_slice());
            let mut msg = "".to_string();
            if 0 == decoded.message.len() {
                println!("Error: Incorrect message length");
            }
            else {
                msg.push_str(&decoded.uid);
                msg.push_str(":");
                msg.push_str(String::from_utf8_lossy(decoded.message.as_slice()).into_owned().as_str());
            }
            stdout.write_all(&msg.into_bytes())
        });

        send_stdin.map(|_| ())
        .select(write_stdout.map(|_| ()))
        .then(|_| Ok(()))
    });

    let _run = match core.run(client) {
        Ok(run) => run,
        Err(err) => {
            println!("Error: {}", err);
            process::exit(1);
        }
    };
}

struct Bytes;

impl Codec for Bytes {
    type In = EasyBuf;
    type Out = Vec<u8>;

    fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<EasyBuf>> {
        if buf.len() >= HDRL { // HDRL is header min size
            if read_hdr_type(buf.as_slice()) != 'M' as u32 {
                let len = buf.len();
                buf.drain_to(len);
                return Ok(None);   
            }
            let hdr_len = read_hdr_len(buf.as_slice()); 
            if 0 == hdr_len {
                let len = buf.len();
                buf.drain_to(len);
                return Ok(None);
            }
            let len = buf.len();
            if len < (HDRL + hdr_len) {
                return Ok(None); 
            }
            if HDRL + hdr_len < len { 
                buf.drain_to(HDRL);
                return Ok(Some(buf.drain_to(hdr_len)));
            }
            buf.drain_to(HDRL);
            Ok(Some(buf.drain_to(hdr_len)))
        } else {
            Ok(None)
        }
    }

    fn encode(&mut self, data: Vec<u8>, buf: &mut Vec<u8>) -> io::Result<()> {
        let mut msgv = write_hdr(data.len()-KEYL);
        msgv.extend(data);
        buf.extend(msgv);
        Ok(())
    }
}

fn read_stdin(mut rx: mpsc::Sender<Vec<u8>>) {
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    /* Set user */
    let _val = stdout.write_all(b"User name?\n");
    let mut buf = vec![0; 80];
    let n = match stdin.read(&mut buf) {
        Err(_) |
            Ok(0) => return,
            Ok(n) => n,
    };
    buf.truncate(n-1);
    let mut userstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();
    if userstr.ends_with("\r") {
        let len = userstr.len();
        userstr.truncate(len - 1);
    }

    /* Set channel */
    let _val = stdout.write_all(b"Channel?\n");
    let mut buf = vec![0; 80];
    let n = match stdin.read(&mut buf) {
        Err(_) |
            Ok(0) => return,
            Ok(n) => n,
    };
    buf.truncate(n-1);
    let mut channelstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();
    if channelstr.ends_with("\r") {
        let len = channelstr.len();
        channelstr.truncate(len - 1);
    }

    /* Join channel */
    let msg = message_encode(&Msg { uid: userstr.clone(), channel: channelstr.clone(), message: Vec::new() }); 
    rx = rx.send(msg).wait().unwrap();

    let mut msg = userstr.clone();
    msg += " to ";
    msg += channelstr.as_str();

    /* Say welcome */
    let mut welcome = "Welcome ".to_string();
    welcome += msg.as_str();
    welcome += "!\n";
    let _val = stdout.write_all(welcome.as_bytes());

    loop {
        let mut buf = vec![0;80];
        let n = match stdin.read(&mut buf) {
            Err(_) |
                Ok(0) => break,
                Ok(n) => n,
        };
        buf.truncate(n);
        let str =  String::from_utf8_lossy(buf.as_slice()).into_owned();
        let msg = message_encode(&Msg { uid: userstr.clone(), channel: channelstr.clone(), message: str.into_bytes() });
        rx = rx.send(msg).wait().unwrap();
    }
}

