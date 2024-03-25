/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2024  Mles developers
 */

use futures_util::{SinkExt, StreamExt};

use base64::{engine::general_purpose, Engine as _};
use tokio::net::TcpStream;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::client_async_tls;
use tokio_tungstenite::tungstenite::protocol::Message as WebMessage;
use tungstenite::handshake::client::Request;

use crate::ConsolidatedError;
use crate::Message;
use crate::MlesPeerHeader;

#[derive(Debug)]
pub enum WsPeerEvent {
    Init(
        MlesPeerHeader,
        Sender<Option<Result<Message, ConsolidatedError>>>,
    ),
}

pub fn init_peers(
    peers: Vec<String>,
    peerrx: Receiver<WsPeerEvent>,
    port: u16,
    nodeid: u64,
    key: u64,
) {
    tokio::spawn(async move {
        log::debug!("Peers {peers:?}");

        for peer in &peers {
            let url = "wss://".to_owned() + peer;
            let key = general_purpose::STANDARD.encode(key.to_be_bytes());
            log::debug!("Url: {url}");
            log::debug!("Key: {key}");
            let request = Request::builder()
                .uri(&url)
                .header("Host", peer)
                .header("Connection", "keep-alive, upgrade")
                .header("Upgrade", "websocket")
                .header("Sec-WebSocket-Version", "13")
                .header("Sec-WebSocket-Protocol", "mles-websocket")
                .header("Sec-WebSocket-Key", key.clone())
                .body(())
                .unwrap();
            let cparam = format!("{peer}:{port}");

            let tcp_stream = TcpStream::connect(&cparam).await.unwrap();
            let (ws_stream, _) = client_async_tls(request, tcp_stream).await.unwrap();

            let (mut peer_tx, mut peer_rx) = ws_stream.split();
            let peerhdr = MlesPeerHeader { peerid: format!("{nodeid:x}") };
            let res = peer_tx
                .send(WebMessage::text(serde_json::to_string(&peerhdr).unwrap()))
                .await;
            match res {
                Ok(_) => {}
                Err(err) => println!("Error {:?}", err),
            }

            //Receive first acknowledge from peer
            if let Some(message) = peer_rx.next().await {
                match message {
                    Ok(msg)  => 'message: {
                        if msg.is_text() {
                            let msg = msg.to_text().unwrap();
                            if let Ok(peerhdr) = serde_json::from_str::<MlesPeerHeader>(msg) {
                                log::info!("Got valid peer header {peerhdr:?}");
                                break 'message;
                            }
                        }
                        log::info!("NOT valid valid peer header {msg:?}");
                    },
                    Err(err) => {
                        log::error!("{}", err);
                        return;
                    }
                }
            }
            //IF not ok, return or retry etc...

            //TODO receive peer ack, send frames to all sockets and send frames also to peer socket
            //TODO in case of connection close, implement reconnect attempt
            //With several peers send at most one in alphabetical order?
            while let Some(message) = peer_rx.next().await {
                match message {
                    Ok(msg) => {
                        // Handle the message
                        println!("Received message: {:?}", msg);
                    }
                    Err(e) => {
                        // Handle error
                        eprintln!("Error receiving message: {:?}", e);
                        break;
                    }
                }
            }
        }
    });
}
