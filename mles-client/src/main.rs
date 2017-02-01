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
*/

/* 
 * Mles-example client based on Tokio core-connect example.
 */

extern crate mles_utils;
extern crate futures;
extern crate tokio_core;

use std::env;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::thread;

use futures::{Sink, Future, Stream};
use futures::sync::mpsc;
use tokio_core::reactor::Core;
use tokio_core::io::{Io, EasyBuf, Codec};
use tokio_core::net::TcpStream;
use mles_utils::*;

const HDRL: usize = 4;
const KEYL: usize = 8;

fn main() {
    // Parse what address we're going to connect to
    //let addr = env::args().nth(1).unwrap_or_else(|| {
    //    panic!("this program requires at least one argument")
    //});
    let addr = "127.0.0.1:8077";
    let addr = addr.parse::<SocketAddr>().unwrap();

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let tcp = TcpStream::connect(&addr, &handle);

    // Handle stdin in a separate thread 
    let (stdin_tx, stdin_rx) = mpsc::channel(0);
    thread::spawn(|| read_stdin(stdin_tx));
    let stdin_rx = stdin_rx.map_err(|_| panic!()); // errors not possible on rx

    let mut stdout = io::stdout();
    let client = tcp.and_then(|stream| {
        let (sink, stream) = stream.framed(Bytes).split();
        let send_stdin = stdin_rx.forward(sink);
        let write_stdout = stream.for_each(move |buf| {
            let decoded = message_decode(buf.as_slice());
            let mut msg = "".to_string();
            if 0 == decoded.message.len() {
                println!("Error happened");
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

    core.run(client).unwrap();
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
            if len < (HDRL + KEYL + hdr_len) {
                return Ok(None); 
            }
            if HDRL + KEYL + hdr_len < len { 
                buf.drain_to(HDRL + KEYL);
                return Ok(Some(buf.drain_to(hdr_len)));
            }
            buf.drain_to(HDRL + KEYL);
            Ok(Some(buf.drain_to(hdr_len)))
        } else {
            Ok(None)
        }
    }

    fn encode(&mut self, data: Vec<u8>, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.extend(data);
        Ok(())
    }
}

fn read_stdin(mut rx: mpsc::Sender<Vec<u8>>) {
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    /* Set user */
    stdout.write_all(b"User name?\n");
    let mut buf = vec![0; 80];
    let n = match stdin.read(&mut buf) {
        Err(_) |
            Ok(0) => return,
            Ok(n) => n,
    };
    buf.truncate(n-1);
    let userstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();

    /* Set channel */
    stdout.write_all(b"Channel?\n");
    let mut buf = vec![0; 80];
    let n = match stdin.read(&mut buf) {
        Err(_) |
            Ok(0) => return,
            Ok(n) => n,
    };
    buf.truncate(n-1);
    let channelstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();

    /* Join channel */
    let msg = message_encode(&Msg { uid: userstr.clone(), channel: channelstr.clone(), message: Vec::new() }); 
    let mut msgv = write_hdr(msg.len());
    let keyv = write_key(0);
    msgv.extend(keyv);
    msgv.extend(msg);
    rx = rx.send(msgv).wait().unwrap();

    let mut msg = userstr.clone();
    msg += "::";
    msg += channelstr.as_str();

    /* Say welcome */
    let mut welcome = "Welcome to ".to_string();
    welcome += msg.as_str();
    welcome += "!\n";
    stdout.write_all(welcome.as_bytes());


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
        let mut msgv = write_hdr(msg.len());
        let keyv = write_key(0);
        msgv.extend(keyv);
        msgv.extend(msg);
        rx = rx.send(msgv).wait().unwrap();
    }
}

