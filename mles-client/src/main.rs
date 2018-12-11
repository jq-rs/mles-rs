/**
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
 * Mles-support Copyright (c) 2017-2018  Mles developers
 *
 */

/*
 * Mles client example based on Tokio core-connect example.
 */

mod ws;

use std::{env, process};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::thread;
use std::io::{Error, ErrorKind};
use futures::sync::mpsc::{UnboundedSender, UnboundedReceiver};
use std::time::Duration;

use bytes::BytesMut;
use futures::{Sink, Future, Stream};
use futures::sync::mpsc;
use tokio_core::reactor::Core;
use tokio_core::net::TcpStream;
use tokio_codec::{Encoder, Decoder};
use mles_utils::*;

use crate::ws::*;

const SRVPORT: &str = ":8077";
const USAGE: &str = "Usage: mles-client <server-address> [--use-websockets]";
const KEEPALIVE: u64 = 5;

fn main() {
    let mut ws_enabled: Option<bool> = None;

    // Parse what address we're going to connect to
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| {
                            println!("{}", USAGE);
                            process::exit(1);
                        });

    let raddr = addr + SRVPORT;
    let raddr: Vec<_> = raddr.to_socket_addrs()
        .unwrap_or_else(|_| vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)].into_iter())
        .collect();
    let raddr = *raddr.first().unwrap();
    let raddr = Some(raddr).unwrap();
    // check that we really got a proper peer
    if 0 == raddr.port() {
        println!("{}", USAGE);
        process::exit(1);
    }

    if let Some(ws) = env::args().nth(2) {
        if ws == "--use-websockets" {
            ws_enabled = Some(true);
        } else {
            println!("{}", USAGE);
            process::exit(1);
        }
    }

    let keyval = match env::var("MLES_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    let keyaddr = match env::var("MLES_ADDR_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    if ws_enabled.is_some() {
        /* Websocket proxy support */
        process_ws_proxy(raddr, keyval, keyaddr);
    } else {
        // Handle stdin in a separate thread
        let (stdin_tx, stdin_rx) = mpsc::channel(16);

        let mut core = Core::new().unwrap();
        let handle = core.handle();
        let tcp = TcpStream::connect(&raddr, &handle);
        let mut cid: Option<u32> = None;
        let mut key: Option<u64> = None;
        let mut keys = Vec::new();

        /* Normal console connection */
        thread::spawn(|| read_stdin(stdin_tx));
        let stdin_rx = stdin_rx.map_err(|_| panic!()); // errors not possible on rx

        let mut stdout = io::stdout();
        let client = tcp.and_then(|stream| {
            let _val = stream.set_nodelay(true)
                             .map_err(|_| panic!("Cannot set to no delay"));
            let _val = stream.set_keepalive(Some(Duration::new(KEEPALIVE, 0)))
                             .map_err(|_| panic!("Cannot set keepalive"));
            let laddr = match stream.local_addr() {
                Ok(laddr) => laddr,
                Err(_) => {
                    let addr = "0.0.0.0:0";
                    addr.parse::<SocketAddr>().unwrap()
                }
            };
            if  !keyval.is_empty() {
                keys.push(keyval.clone());
            } else {
                keys.push(MsgHdr::addr2str(&laddr));
                if !keyaddr.is_empty() {
                    keys.push(keyaddr.clone());
                }
            }
            let (sink, stream) = Bytes.framed(stream).split();
            let stdin_rx = stdin_rx.and_then(|buf| {
                if None == key {
                    //create hash for verification
                    let decoded_message = Msg::decode(buf.as_slice());
                    keys.push(decoded_message.get_uid().to_string());
                    keys.push(decoded_message.get_channel().to_string());
                    key = Some(MsgHdr::do_hash(&keys));
                    cid = Some(MsgHdr::select_cid(key.unwrap()));
                }
                let msghdr = MsgHdr::new(buf.len() as u32, cid.unwrap(), key.unwrap());
                let mut msgv = msghdr.encode();
                msgv.extend(buf);
                Ok(msgv)
            });

            let send_stdin = stdin_rx.forward(sink);
            let write_stdout = stream.for_each(move |buf| {
                let mut msg = "".to_string();
                let buf = buf.to_vec();
                /* Just ignore Resyncs */
                let rdecoded = ResyncMsg::decode(buf.as_slice());
                if rdecoded.get_messages().len() > 0 {
                    return stdout.write_all(&msg.into_bytes());
                }
                let decoded = Msg::decode(buf.as_slice());
                if !decoded.get_message().is_empty() {
                    msg.push_str(decoded.get_uid());
                    msg.push_str(":");
                    msg.push_str(String::from_utf8_lossy(decoded.get_message().as_slice())
                                     .into_owned()
                                     .as_str());
                }
                stdout.write_all(&msg.into_bytes())
            });

            send_stdin
                .map(|_| ())
                .select(write_stdout.map(|_| ()))
                .then(|_| Ok(()))
        });

        match core.run(client) {
            Ok(_) => {}
            Err(err) => {
                println!("Error: {}", err);
                process::exit(1);
            }
        };
    }
}

struct Bytes;

impl Decoder for Bytes {
    type Item = BytesMut;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<BytesMut>> {
        if buf.len() >= MsgHdr::get_hdrkey_len() {
            let msghdr = MsgHdr::decode(buf.to_vec());
            // HDRKEYL is header min size
            if msghdr.get_type() != 'M' as u8 {
                let len = buf.len();
                buf.split_to(len);
                return Ok(None);
            }
            let hdr_len = msghdr.get_len() as usize;
            if 0 == hdr_len {
                let len = buf.len();
                buf.split_to(len);
                return Ok(None);
            }
            let len = buf.len();
            if len < (MsgHdr::get_hdrkey_len() + hdr_len) {
                return Ok(None);
            }
            if MsgHdr::get_hdrkey_len() + hdr_len < len {
                buf.split_to(MsgHdr::get_hdrkey_len());
                return Ok(Some(buf.split_to(hdr_len)));
            }
            buf.split_to(MsgHdr::get_hdrkey_len());
            return Ok(Some(buf.split_to(hdr_len)));
        }
        Ok(None)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> io::Result<Option<BytesMut>> {
        self.decode(buf)
    }
}

impl Encoder for Bytes {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn encode(&mut self, data: Vec<u8>, buf: &mut BytesMut) -> io::Result<()> {
        buf.extend_from_slice(&data[..]);
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
        Err(_) | Ok(0) => return,
        Ok(n) => n,
    };
    buf.truncate(n - 1);
    let mut userstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();
    if userstr.ends_with('\r') {
        let len = userstr.len();
        userstr.truncate(len - 1);
    }

    /* Set channel */
    let _val = stdout.write_all(b"Channel?\n");
    let mut buf = vec![0; 80];
    let n = match stdin.read(&mut buf) {
        Err(_) | Ok(0) => return,
        Ok(n) => n,
    };
    buf.truncate(n - 1);
    let mut channelstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();
    if channelstr.ends_with('\r') {
        let len = channelstr.len();
        channelstr.truncate(len - 1);
    }

    /* Join channel */
    let msg = Msg::new(userstr.clone(), channelstr.clone(), Vec::new());
    let msg = msg.encode();
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
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);
        let str = String::from_utf8_lossy(buf.as_slice()).into_owned();
        let msg = Msg::new(userstr.clone(), channelstr.clone(), str.into_bytes());
        let msg = msg.encode();
        rx = rx.send(msg).wait().unwrap();
    }
}

pub fn process_mles_client(raddr: SocketAddr, keyval: String, keyaddr: String,
                           ws_tx: UnboundedSender<Vec<u8>>, mles_rx: UnboundedReceiver<Vec<u8>>) {
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let tcp = TcpStream::connect(&raddr, &handle);
    let mut cid: Option<u32> = None;
    let mut key: Option<u64> = None;
    let mut keys = Vec::new();

    let client = tcp.and_then(|stream| {
        let _val = stream.set_nodelay(true)
                         .map_err(|_| panic!("Cannot set to no delay"));
        let _val = stream.set_keepalive(Some(Duration::new(KEEPALIVE, 0)))
                         .map_err(|_| panic!("Cannot set keepalive"));
        let laddr = match stream.local_addr() {
            Ok(laddr) => laddr,
            Err(_) => {
                let addr = "0.0.0.0:0";
                addr.parse::<SocketAddr>().unwrap()
            }
        };
        if  !keyval.is_empty() {
            keys.push(keyval);
        } else {
            keys.push(MsgHdr::addr2str(&laddr));
            if !keyaddr.is_empty() {
                keys.push(keyaddr);
            }
        }
        let (sink, stream) = Bytes.framed(stream).split();
        let mles_rx = mles_rx.map_err(|_| panic!()); // errors not possible on rx XXX
        let mles_rx = mles_rx.and_then(|buf| {
            if buf.is_empty() {
                return Err(Error::new(ErrorKind::BrokenPipe, "broken pipe"));
            }
            if None == key {
                //create hash for verification
                let decoded_message = Msg::decode(buf.as_slice());
                keys.push(decoded_message.get_uid().to_string());
                keys.push(decoded_message.get_channel().to_string());
                key = Some(MsgHdr::do_hash(&keys));
                cid = Some(MsgHdr::select_cid(key.unwrap()));
            }
            let msghdr = MsgHdr::new(buf.len() as u32, cid.unwrap(), key.unwrap());
            let mut msgv = msghdr.encode();
            msgv.extend(buf);
            Ok(msgv)
        });

        let send_wsrx = mles_rx.forward(sink);
        let write_wstx = stream.for_each(move |buf| {
            let ws_tx_inner = ws_tx.clone();
            // send to websocket
            let _ = ws_tx_inner.send(buf.to_vec()).wait().map_err(|err| {
                Error::new(ErrorKind::Other, err)
            });
            Ok(())
        });

        send_wsrx
            .map(|_| ())
            .select(write_wstx.map(|_| ()))
            .then(|_| Ok(()))
    });

    match core.run(client) {
        Ok(_) => {}
        Err(err) => {
            println!("Error: {}", err);
        }
    };
}

