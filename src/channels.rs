/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2024  Mles developers
 */

use futures_util::{SinkExt, StreamExt};

use std::collections::VecDeque;
use std::collections::{hash_map::Entry, HashMap};

use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use tokio_stream::wrappers::ReceiverStream;

use warp::ws::Message;

use crate::ConsolidatedError;

use crate::Arc;
use crate::Mutex;

#[derive(Debug)]
pub enum WsEvent {
    Init(
        u64,
        u64,
        String,
        Sender<Option<Result<Message, ConsolidatedError>>>,
        broadcast::Sender<u64>,
        Message,
    ),
    Msg(u64, u64, Message),
    SendHistoryForPeer(u64, Sender<Option<Result<Message, ConsolidatedError>>>),
    Logoff(u64, u64),
}

fn add_message(h: u64, msg: Message, limit: u32, queue: &mut VecDeque<(u64, Message)>) {
    let limit = limit as usize;
    let len = queue.len();
    let cap = queue.capacity();
    if cap < limit && len == cap && cap * 2 >= limit {
        queue.reserve(limit - queue.capacity())
    }
    if len == limit {
        queue.pop_front();
    }
    queue.push_back((h, msg));
}

pub fn init_channels(mut rx: ReceiverStream<WsEvent>, limit: u32, peermtx: Arc<Mutex<()>>) {
    tokio::spawn(async move {
        let mut msg_db: HashMap<u64, VecDeque<(u64, Message)>> = HashMap::new();
        let mut ch_db: HashMap<
            u64,
            HashMap<u64, (String, Sender<Option<Result<Message, ConsolidatedError>>>)>,
        > = HashMap::new();
        while let Some(event) = rx.next().await {
            match event {
                WsEvent::Init(h, ch, msgstr, tx2, err_tx, msg) => {
                    if !ch_db.contains_key(&ch) {
                        ch_db.entry(ch).or_default();
                    }
                    if let Some(uid_db) = ch_db.get_mut(&ch) {
                        if let Entry::Vacant(e) = uid_db.entry(h) {
                            msg_db.entry(ch).or_default();
                            e.insert((msgstr, tx2.clone()));
                            log::info!("Added {h:x} into {ch:x}.");

                            let val = err_tx.send(h);
                            if let Err(err) = val {
                                log::info!("Got err tx msg err {err}");
                            }


                            for (_, (_, tx)) in uid_db.iter().filter(|(&xh, _)| xh != h) {
                                let res = tx.send(Some(Ok(msg.clone()))).await;
                                if let Err(err) = res {
                                    log::info!("Got tx msg err {err}");
                                }
                            }

                            let queue = msg_db.get_mut(&ch);
                            if let Some(queue) = queue {

                                for (_, qmsg) in &*queue {
                                    let res = tx2.send(Some(Ok(qmsg.clone()))).await;
                                    if let Err(err) = res {
                                        log::info!("Got tx snd qmsg err {err}");
                                    }
                                }
                                log::info!("Add {msg:?} ch {ch} to history!");
                                add_message(h, msg, limit, queue);
                            }
                        } else {
                            log::warn!("Init done to {h:x} into {ch:x}, closing!");
                        }
                    }
                }
                WsEvent::Msg(h, ch, msg) => {
                    if let Some(uid_db) = ch_db.get(&ch) {
                        for (_, (_,tx)) in uid_db.iter().filter(|(&xh, _)| xh != h) {
                            let res = tx.send(Some(Ok(msg.clone()))).await;
                            if let Err(err) = res {
                                log::info!("Got tx snd msg err {err}");
                            }
                        }
                        let queue = msg_db.get_mut(&ch);
                        if let Some(queue) = queue {
                            log::info!("Add {msg:?} ch {ch} to history!");
                            add_message(h, msg, limit, queue);
                        }
                    }
                },
                WsEvent::SendHistoryForPeer(ch, tx) => {
                    let queue = msg_db.get(&ch);
                    if let Some(queue) = queue {
                        log::info!("Sending history msg for channel {ch}!");
                        if let Some(uid_db) = ch_db.get(&ch) {
                            for (h, qmsg) in &*queue {
                                if let Some((msgstr, _)) = uid_db.get(h) {
                                    log::info!("Sending history msg {qmsg:?} for uid {h}!");
                                    let _ = peermtx.lock().await;
                                    let _res = tx.send(Some(Ok(Message::text(msgstr)))).await;
                                    let _res = tx.send(Some(Ok(qmsg.clone()))).await;
                                }
                            }
                        }
                    }
                    else {
                        log::error!("Message queue empty for {ch}");
                    }
                }
                WsEvent::Logoff(h, ch) => {
                    if let Some(uid_db) = ch_db.get_mut(&ch) {
                        uid_db.remove(&h);
                        if uid_db.is_empty() {
                            ch_db.remove(&ch);
                        }
                        log::info!("Removed {h:x} from {ch:x}.");
                    }
                }
            }
        }
    });
}
