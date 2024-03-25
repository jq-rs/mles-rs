/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2024  Mles developers
 */

use futures_util::{SinkExt, StreamExt};

use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::client_async_tls;
use tokio_tungstenite::tungstenite::protocol::Message as WebMessage;
use tungstenite::handshake::client::Request;

use crate::ConsolidatedError;
use crate::Message;
use crate::ReceiverStream;

use std::collections::{hash_map::Entry, HashMap};

#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct MlesPeerHeader {
    peerid: String,
}

#[derive(Debug)]
pub enum WsPeerEvent {
    Init(
        MlesPeerHeader,
        Sender<Option<Result<Message, ConsolidatedError>>>,
    ),
    InitChannel(
        u64,
        u64,
        Message,
        Sender<Option<Result<Message, ConsolidatedError>>>,
    ),
    Msg(u64, u64, Message),
    Logoff(u64, u64),
}

pub fn init_peers(
    peers: Option<Vec<String>>,
    mut peerrx: ReceiverStream<WsPeerEvent>,
    port: u16,
    nodeid: u64,
    key: u64,
) {
    tokio::spawn(async move {
        let mut ptx: Option<Sender<Option<Result<Message, ConsolidatedError>>>> = None;
        let mut chdb: HashMap<
            (u64, u64),
            (Message, Sender<Option<Result<Message, ConsolidatedError>>>),
        > = HashMap::new();
        while let Some(event) = peerrx.next().await {
            match event {
                WsPeerEvent::Init(msg, tx) => {
                    log::debug!("Got msg {msg:?}");
                    let peerhdr = MlesPeerHeader {
                        peerid: format!("{nodeid:x}"),
                    };
                    let _ = tx
                        .send(Some(Ok(Message::text(
                            serde_json::to_string(&peerhdr).unwrap(),
                        ))))
                        .await;
                    ptx = Some(tx);
                }
                WsPeerEvent::InitChannel(h, ch, msg, ctx) => {
                    chdb.insert((h, ch), (msg, ctx));
                }
                WsPeerEvent::Msg(h, ch, msg) => {
                    //Find out msg hdr with h + ch
                    //Send first msg hdr and then msg as below
                    if let Some(ref tx) = ptx {
                        let first = chdb.get(&(h, ch));
                        if let Some((first_msg, _)) = first {
                            let _ = tx.send(Some(Ok(first_msg.clone()))).await;
                            let _ = tx.send(Some(Ok(msg))).await;
                        }
                    }
                }
                WsPeerEvent::Logoff(h, ch) => {
                    chdb.remove(&(h, ch));
                }
            }
        }
    });

    if let Some(peers) = peers {
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
                let peerhdr = MlesPeerHeader {
                    peerid: format!("{nodeid:x}"),
                };
                log::info!("Send to peer {}", serde_json::to_string(&peerhdr).unwrap());
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
                        Ok(msg) => 'message: {
                            if msg.is_text() {
                                let msg = msg.to_text().unwrap();
                                if let Ok(peerhdr) = serde_json::from_str::<MlesPeerHeader>(msg) {
                                    log::info!("Got valid peer header {peerhdr:?}");
                                    break 'message;
                                }
                            }
                            log::info!("NOT valid valid peer header {msg:?}");
                            return;
                        }
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
                            log::info!("Received message: {:?}", msg);
                        }
                        Err(e) => {
                            // Handle error
                            log::error!("Error receiving message: {:?}", e);
                            break;
                        }
                    }
                }
            }
        });
    }
}
