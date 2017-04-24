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
 * Mles Client example based on Tokio core-connect example.
 */

extern crate mles_utils;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate bytes;

/* websocket proxy support */
extern crate websocket;
extern crate hyper;

use std::{env, process};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::thread;

use bytes::{BufMut, BytesMut};
use futures::{Sink, Future, Stream};
use futures::sync::mpsc;
use tokio_core::reactor::Core;
use tokio_core::net::TcpStream;
use tokio_io::AsyncRead;
use tokio_io::codec::{Encoder, Decoder};
use mles_utils::*;

/* websocket proxy support */
use websocket::{Server, Message};
use websocket::message::Type;
use hyper::server::{Request, Response};

const HTML: &'static str = include_str!("mles-websockets.html");

fn hello(_: Request, res: Response) {
        res.send(HTML.as_bytes()).unwrap();
}

const KEYL: usize = 8; //key len
const HDRKEYL: usize = 4 + KEYL; //hdr + key len

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

    /* Websocket proxy support */
    thread::spawn(move || {
        let _listening = hyper::Server::http("0.0.0.0:8080").unwrap()
        .handle(hello);
        println!("Mles Websockets listening on port 8080");
    });

    // Handle stdin in a separate thread 
    let (stdin_tx, stdin_rx) = mpsc::channel(16);
    let (mles_tx_ws, mles_rx_ws) = std::sync::mpsc::channel();

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let tcp = TcpStream::connect(&addr, &handle);
    let mut key = 0;


    let ws_tx_inner = stdin_tx.clone();
    let mles_tx_ws_inner = mles_tx_ws.clone();
    thread::spawn(move || {
        let ws_server = Server::bind("0.0.0.0:8076").unwrap();
        for connection in ws_server.filter_map(Result::ok) {
            let ws_tx = ws_tx_inner.clone();
            let mles_tx_ws = mles_tx_ws_inner.clone();
            if !connection.protocols().contains(&"mles-websocket".to_string()) {
                connection.reject().unwrap();
                println!("Protocol rejected");
                continue;
            }
            let client = connection.use_protocol("mles-websocket").accept().unwrap();
            let ip = client.peer_addr().unwrap();
            println!("Connection from {}", ip);

            let (mut receiver, mut sender) = client.split().unwrap();
            thread::spawn(move || {
                for message in receiver.incoming_messages() {
                    let ws_tx_msg = ws_tx.clone();
                    let mles_tx_ws_msg = mles_tx_ws.clone();
                    let message: Message = message.unwrap();

                    match message.opcode {
                        Type::Close => {
                            let message = Message::close();
                            let msg = message.payload.into_owned().to_vec();
                            mles_tx_ws_msg.send(msg).unwrap();
                            println!("Client {} disconnected", ip);
                            return;
                        }
                        Type::Ping => {
                            let message = Message::pong(message.payload);
                            let msg = message.payload.into_owned().to_vec();
                            mles_tx_ws_msg.send(msg).unwrap();
                        }
                        _ => {
                            let msg = message.payload.into_owned().to_vec();
                            println!("Rcvd msg {:?} len {}", msg, msg.len());
                            mles_tx_ws_msg.send(msg.clone()).unwrap();
                            let _ = ws_tx_msg.send(msg).wait().unwrap();
                        }
                    }
                }
            });

            loop {
                let mles_msg: Vec<u8> = mles_rx_ws.recv().unwrap();
                println!("Sending msg {:?} len {}", mles_msg, mles_msg.len());
                let message: Message = Message::binary(mles_msg);
                sender.send_message(&message).unwrap();
            }
        }
    });

    thread::spawn(|| read_stdin(stdin_tx));
    let stdin_rx = stdin_rx.map_err(|_| panic!()); // errors not possible on rx

    let mut stdout = io::stdout();
    let client = tcp.and_then(|stream| {
        let _val = stream.set_nodelay(true).map_err(|_| panic!("Cannot set to no delay"));
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
            /* send to websocket */
            match mles_tx_ws.send(buf.to_vec().clone()) {
                Ok(_) => {},
                Err(err) => {println!("Error: {}", err)},
            }

            let decoded = message_decode(buf.to_vec().as_slice());
            let mut msg = "".to_string();
            if  decoded.get_message().len() > 0 {
                msg.push_str(decoded.get_uid());
                msg.push_str(":");
                msg.push_str(String::from_utf8_lossy(decoded.get_message().as_slice()).into_owned().as_str());
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

impl Decoder for Bytes {
    type Item = BytesMut;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<BytesMut>> {
        if buf.len() >= HDRKEYL { // HDRKEYL is header min size
            if read_hdr_type(buf.to_vec().as_slice()) != 'M' as u32 {
                let len = buf.len();
                buf.split_to(len);
                return Ok(None);   
            }
            let hdr_len = read_hdr_len(buf); 
            if 0 == hdr_len {
                let len = buf.len();
                buf.split_to(len);
                return Ok(None);
            }
            let len = buf.len();
            if len < (HDRKEYL + hdr_len) {
                return Ok(None); 
            }
            if HDRKEYL + hdr_len < len { 
                buf.split_to(HDRKEYL);
                return Ok(Some(buf.split_to(hdr_len)));
            }
            buf.split_to(HDRKEYL);
            Ok(Some(buf.split_to(hdr_len)))
        } else {
            Ok(None)
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> io::Result<Option<BytesMut>> {
        self.decode(buf)
    }
}

impl Encoder for Bytes {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn encode(&mut self, data: Vec<u8>, buf: &mut BytesMut) -> io::Result<()> {
        let mut msgv = write_hdr(data.len()-KEYL);
        msgv.extend(data);
        buf.put(&msgv[..]);
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
    let msg = message_encode(&Msg::new(userstr.clone(), channelstr.clone(), Vec::new())); 
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
        let msg = Msg::new(userstr.clone(), channelstr.clone(), str.into_bytes());
        //println!("Msg from input: {:?}", msg);
        let msg = message_encode(&msg);
        rx = rx.send(msg).wait().unwrap();
    }
}

