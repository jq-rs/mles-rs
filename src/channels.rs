/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2024  Mles developers
 */

use futures_util::{SinkExt, StreamExt};

use std::collections::VecDeque;
use std::collections::{hash_map::Entry, HashMap};

use tokio::sync::mpsc::Sender;
use tokio::sync::broadcast;

use tokio_stream::wrappers::ReceiverStream;

use warp::ws::Message;

use crate::ConsolidatedError;

#[derive(Debug)]
pub enum WsEvent {
    Init(
        u64,
        u64,
        Sender<Option<Result<Message, ConsolidatedError>>>,
        broadcast::Sender<u64>,
        Message,
        bool,
    ),
    Msg(u64, u64, Message),
    Logoff(u64, u64),
}

fn add_message(msg: Message, limit: u32, queue: &mut VecDeque<Message>) {
    let limit = limit as usize;
    let len = queue.len();
    let cap = queue.capacity();
    if cap < limit && len == cap && cap * 2 >= limit {
        queue.reserve(limit - queue.capacity())
    }
    if len == limit {
        queue.pop_front();
    }
    queue.push_back(msg);
}

pub fn init_channels(mut rx: ReceiverStream<WsEvent>, limit: u32) {
    tokio::spawn(async move {
        let mut msg_db: HashMap<u64, VecDeque<Message>> = HashMap::new();
        let mut ch_db: HashMap<
            u64,
            HashMap<u64, Sender<Option<Result<Message, ConsolidatedError>>>>,
        > = HashMap::new();
        while let Some(event) = rx.next().await {
            match event {
                WsEvent::Init(h, ch, tx2, err_tx, msg, is_peer) => {
                    if !ch_db.contains_key(&ch) {
                        ch_db.entry(ch).or_default();
                    }
                    if let Some(uid_db) = ch_db.get_mut(&ch) {
                        if let Entry::Vacant(e) = uid_db.entry(h) {
                            msg_db.entry(ch).or_default();
                            e.insert(tx2.clone());
                            log::info!("Added {h:x} into {ch:x}.");

                            if !is_peer {
                                let val = err_tx.send(h);
                                if let Err(err) = val {
                                    log::info!("Got err tx msg err {err}");
                                }
                            }

                            for (_, tx) in uid_db.iter().filter(|(&xh, _)| xh != h) {
                                let res = tx.send(Some(Ok(msg.clone()))).await;
                                if let Err(err) = res {
                                    log::info!("Got tx msg err {err}");
                                }
                            }

                            let queue = msg_db.get_mut(&ch);
                            if let Some(queue) = queue {
                                if !is_peer {
                                    for qmsg in &*queue {
                                        let res = tx2.send(Some(Ok(qmsg.clone()))).await;
                                        if let Err(err) = res {
                                            log::info!("Got tx snd qmsg err {err}");
                                        }
                                    }
                                }
                                add_message(msg, limit, queue);
                            }
                        } else {
                            log::warn!("Init done to {h:x} into {ch:x}, closing!");
                        }
                    }
                }
                WsEvent::Msg(h, ch, msg) => {
                    if let Some(uid_db) = ch_db.get(&ch) {
                        for (_, tx) in uid_db.iter().filter(|(&xh, _)| xh != h) {
                            let res = tx.send(Some(Ok(msg.clone()))).await;
                            if let Err(err) = res {
                                log::info!("Got tx snd msg err {err}");
                            }
                        }
                        let queue = msg_db.get_mut(&ch);
                        if let Some(queue) = queue {
                            add_message(msg, limit, queue);
                        }
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
