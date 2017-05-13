/**
 * Copyright (c) 2017 Daniel Abramov
 * Copyright (c) 2017 Alexey Galakhov
 * 
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 * 
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 * 
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 * 
 * Mles-support Copyright (c) 2017 Mles developers
 */

extern crate futures;
extern crate tokio_core;
extern crate tokio_tungstenite;
extern crate tungstenite;

use std::io::{Error, ErrorKind};
use std::thread;
use std::net::SocketAddr;

use futures::stream::Stream;
use futures::{Future, Sink};
use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;
use self::tungstenite::protocol::Message;

use self::tokio_tungstenite::accept_async;

const WSPORT: &str = ":8076";

pub fn process_ws_proxy(raddr: SocketAddr, keyval: String, keyaddr: String) {                             
    let addr = "0.0.0.0".to_string() + WSPORT;
    let addr = addr.parse().unwrap();
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let socket = TcpListener::bind(&addr, &handle).unwrap();
    println!("Listening WebSockets on: {}", addr);
    let mut cnt = 0;

    let srv = socket.incoming().for_each(|(stream, addr)| {
        let _val = stream.set_nodelay(true)
                         .map_err(|_| panic!("Cannot set to no delay"));
        let _val = stream.set_keepalive_ms(::KEEPALIVEMS)
                         .map_err(|_| panic!("Cannot set keepalive"));
        let handle_inner = handle.clone();
        cnt += 1;

        let (ws_tx, ws_rx) = futures::sync::mpsc::unbounded();
        let (mles_tx, mles_rx) = futures::sync::mpsc::unbounded();

        let keyval_inner = keyval.clone();
        let keyaddr_inner = keyaddr.clone();
        let ws_tx_inner = ws_tx.clone();
        accept_async(stream).and_then(move |ws_stream| {
            let ws_tx_own = ws_tx_inner.clone();
            println!("New WebSocket connection {}: {}", cnt, addr);

            thread::spawn(move || ::process_mles_client(raddr, keyval_inner, keyaddr_inner, 
                                                        ws_tx_inner, mles_rx));

            let (sink, stream) = ws_stream.split();

            let ws_reader = stream.for_each(move |message: Message| {
                let ws_tx = ws_tx_own.clone();
                let mles_tx = mles_tx.clone();
                let mles_message = message.into_data();
                let _ = mles_tx.send(mles_message.clone()).wait();
                let _ = ws_tx.send(mles_message).wait();
                Ok(())
            });            

            let ws_writer = ws_rx.fold(sink, |mut sink, msg| {
                sink.start_send(Message::binary(msg)).unwrap();
                Ok(sink)
            });
            let connection = ws_reader.map(|_| ()).map_err(|_| ())
                                      .select(ws_writer.map(|_| ()).map_err(|_| ()));

            handle_inner.spawn(connection.then(move |_| {
                println!("Connection {} closed.", cnt);
                Ok(())
            }));

            Ok(())
        }).map_err(|e| {
            println!("Error during the WebSocket handshake occurred: {}", e);
            Error::new(ErrorKind::Other, e)
        })
    });
    let _run = match core.run(srv) {
        Ok(_) => {}
        Err(err) => {
            println!("Error: {}", err);
        }
    };
}
