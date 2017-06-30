/**
 *   Mles server
 *
 *   Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
 *
 *   This program is free software: you can redistribute it and/or modify
 *   it under the terms of the GNU General Public License as published by
 *   the Free Software Foundation, either version 3 of the License, or
 *   (at your option) any later version.
 *
 *   This program is distributed in the hope that it will be useful,
 *   but WITHOUT ANY WARRANTY; without even the implied warranty of
 *   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *   GNU General Public License for more details.
 *
 *   You should have received a copy of the GNU General Public License
 *   along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

extern crate futures;
extern crate tokio_core;
extern crate tokio_tungstenite;
extern crate tungstenite;

use std::io::{Error, ErrorKind};
use std::thread;
use std::net::SocketAddr;
use std::time::Duration;

use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
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
    let socket = match TcpListener::bind(&addr, &handle) {
        Ok(listener) => listener,
        Err(err) => {
            println!("Proxy error: {}", err);
            return;
        },
    };
    println!("Listening WebSockets on: {}", addr);
    let mut cnt = 0;

    let srv = socket.incoming().for_each(|(stream, addr)| {
        let _val = stream.set_nodelay(true)
                         .map_err(|_| panic!("Cannot set to no delay"));
        let _val = stream.set_keepalive(Some(Duration::new(::KEEPALIVE, 0)))
                         .map_err(|_| panic!("Cannot set keepalive"));
        let handle_inner = handle.clone();
        let handle_out = handle.clone();
        cnt += 1;

        let (ws_tx, ws_rx) = unbounded();
        let (mles_tx, mles_rx) = unbounded();

        let keyval_inner = keyval.clone();
        let keyaddr_inner = keyaddr.clone();
        let ws_tx_inner = ws_tx.clone();
        let accept = accept_async(stream).map_err(|err|{
            println!("Accept error: {}", err);
            Error::new(ErrorKind::Other, err)
        });
        let accept = accept.and_then(move |ws_stream| {
            let ws_tx_own = ws_tx_inner.clone();
            println!("New WebSocket connection {}: {}", cnt, addr);

            thread::spawn(move || ::process_mles_client(raddr, keyval_inner, keyaddr_inner, 
                                                        ws_tx_inner, mles_rx));

            let (sink, stream) = ws_stream.split();

            let ws_reader = stream.for_each(move |message: Message| {
                let ws_tx = ws_tx_own.clone();
                let mles_tx = mles_tx.clone();
                let mles_message = message.into_data();
                let _ = mles_tx.send(mles_message.clone()).wait().map_err(|err| {
                    Error::new(ErrorKind::Other, err)
                });
                let _ = ws_tx.send(mles_message).wait().map_err(|err| {
                    Error::new(ErrorKind::Other, err)
                });
                Ok(())
            });            

            let ws_writer = ws_rx.fold(sink, |mut sink, msg| {
                let _ = sink.start_send(Message::binary(msg)).map_err(|err| {
                    Error::new(ErrorKind::Other, err)
                });                                      
                Ok(sink)
            });
            let connection = ws_reader.map(|_| ()).map_err(|_| ())
                                      .select(ws_writer.map(|_| ()).map_err(|_| ()));

            handle_inner.spawn(connection.then(move |_| {
                println!("Connection {} closed.", cnt);
                Ok(())
            }));

            Ok(())
        });
        handle_out.spawn(accept.then(move |_| {
            Ok(())
        }));

        //keep accepting connections
        Ok(())
    });

    match core.run(srv) {
        Ok(_) => {}
        Err(err) => {
            println!("Error: {}", err);
        }
    };
}
