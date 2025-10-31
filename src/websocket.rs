/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use crate::auth::verify_auth;
use crate::types::{ChannelInfo, MlesHeader, WsEvent};
use futures_util::{SinkExt, StreamExt};
use siphasher::sip::SipHasher;
use std::collections::HashMap;
use std::collections::{VecDeque, hash_map::Entry};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::time;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use warp::Filter;
use warp::ws::Message;

pub const ACCEPTED_PROTOCOL: &str = "mles-websocket";
const WS_BUF: usize = 128;
const PING_INTERVAL: u64 = 24000;
const CHANNEL_CLEANUP_DAYS: u64 = 30;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60); // Run once per day

pub fn add_message(msg: Message, limit: u32, queue: &mut VecDeque<Message>) {
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

pub fn spawn_event_loop(mut rx: ReceiverStream<WsEvent>, limit: u32) {
    tokio::spawn(async move {
        let mut msg_db: HashMap<u64, ChannelInfo> = HashMap::new();
        let mut ch_db: HashMap<u64, HashMap<u64, Sender<Option<Result<Message, warp::Error>>>>> =
            HashMap::new();

        // Create cleanup interval
        let mut cleanup_interval = time::interval(CLEANUP_INTERVAL);

        loop {
            tokio::select! {
                // Handle cleanup
                _ = cleanup_interval.tick() => {
                    let now = SystemTime::now();
                    let threshold = Duration::from_secs(CHANNEL_CLEANUP_DAYS * 24 * 60 * 60);

                    msg_db.retain(|&ch, info| {
                        if let Ok(elapsed) = now.duration_since(info.last_activity) {
                            if elapsed > threshold {
                                log::info!("Cleaning up inactive channel {ch:x}");
                                return false; // Remove channel
                            }
                        }
                        true // Keep channel
                    });
                }

                // Handle websocket events
                Some(event) = rx.next() => {
                    match event {
                        WsEvent::Init(h, ch, tx2, err_tx, msg) => {
                            if !ch_db.contains_key(&ch) {
                                ch_db.entry(ch).or_default();
                            }
                            if let Some(uid_db) = ch_db.get_mut(&ch) {
                                if let Entry::Vacant(e) = uid_db.entry(h) {
                                    // Initialize or update channel info
                                    let channel_info = msg_db.entry(ch).or_insert(ChannelInfo {
                                        messages: VecDeque::new(),
                                        last_activity: SystemTime::now(),
                                    });

                                    e.insert(tx2.clone());
                                    log::info!("Added {h:x} into {ch:x}");

                                    // Send confirmation through err_tx
                                    if let Err(err) = err_tx.send(h) {
                                        log::debug!("Failed to send init confirmation: {}", err);
                                    }

                                    // Rest of init handling...
                                    for qmsg in &channel_info.messages {
                                        let res = tx2.send(Some(Ok(qmsg.clone()))).await;
                                        if let Err(err) = res {
                                            log::debug!("Failed to send queue message: {}", err);
                                        }
                                    }
                                    add_message(msg, limit, &mut channel_info.messages);
                                    channel_info.last_activity = SystemTime::now();
                                } else {
                                    log::warn!("Init done to {h:x} into {ch:x}, closing!");
                                    // Notify about duplicate connection
                                    drop(err_tx);
                                }
                            }
                        }
                        WsEvent::Msg(h, ch, msg) => {
                            if let Some(uid_db) = ch_db.get(&ch) {
                                for (_, tx) in uid_db.iter().filter(|(xh, _)| **xh != h) {
                                    let res = tx.send(Some(Ok(msg.clone()))).await;
                                    if let Err(err) = res {
                                        log::debug!("Failed to send message: {}", err);
                                    }
                                }
                                if let Some(channel_info) = msg_db.get_mut(&ch) {
                                    add_message(msg, limit, &mut channel_info.messages);
                                    channel_info.last_activity = SystemTime::now();
                                }
                            }
                        }
                        WsEvent::Logoff(h, ch) => {
                            if let Some(uid_db) = ch_db.get_mut(&ch) {
                                uid_db.remove(&h);
                                if uid_db.is_empty() {
                                    ch_db.remove(&ch);
                                }
                                log::info!("Removed {h:x} from {ch:x}");
                            }
                        }
                    }
                }
            }
        }
    });
}

pub fn create_ws_handler(
    tx_clone: Sender<WsEvent>,
) -> impl warp::Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::ws()
        .and(warp::header::exact(
            "Sec-WebSocket-Protocol",
            ACCEPTED_PROTOCOL,
        ))
        .map(move |ws: warp::ws::Ws| {
            let tx_inner = tx_clone.clone();
            ws.on_upgrade(move |websocket| {
                let (tx2, rx2) = mpsc::channel::<Option<Result<Message, warp::Error>>>(WS_BUF);
                let (err_tx, err_rx) = oneshot::channel();
                let mut rx2 = ReceiverStream::new(rx2);
                let (mut ws_tx, mut ws_rx) = websocket.split();
                let h = Arc::new(AtomicU64::new(0));
                let ch = Arc::new(AtomicU64::new(0));
                let ping_cntr = Arc::new(AtomicU64::new(0));
                let is_closed = Arc::new(AtomicBool::new(false));

                // Message receiver task
                let tx = tx_inner.clone();
                let tx2_inner = tx2.clone();
                let h_clone = h.clone();
                let ch_clone = ch.clone();
                let ping_cntr_clone = ping_cntr.clone();
                let is_closed_rx = is_closed.clone();

                tokio::spawn(async move {
                    let h = h_clone;
                    let ch = ch_clone;
                    let tx2_spawn = tx2_inner.clone();
                    let ping_cntr_inner = ping_cntr_clone;

                    if let Some(Ok(msg)) = ws_rx.next().await {
                        if !msg.is_text() {
                            is_closed_rx.store(true, Ordering::SeqCst);
                            return;
                        }
                        let msghdr: Result<MlesHeader, serde_json::Error> =
                            serde_json::from_str(msg.to_str().unwrap());
                        match msghdr {
                            Ok(msghdr) => {
                                // Validate UTF-8 and emptiness
                                if !msghdr.uid.is_ascii() || !msghdr.channel.is_ascii() {
                                    log::warn!("Invalid UTF-8 in uid or channel");
                                    is_closed_rx.store(true, Ordering::SeqCst);
                                    return;
                                }
                                if msghdr.uid.is_empty() || msghdr.channel.is_empty() {
                                    log::warn!("Empty uid or channel");
                                    is_closed_rx.store(true, Ordering::SeqCst);
                                    return;
                                }

                                let mut hasher = SipHasher::new();
                                msghdr.hash(&mut hasher);
                                h.store(hasher.finish(), Ordering::SeqCst);
                                let hasher = SipHasher::new();
                                ch.store(hasher.hash(msghdr.channel.as_bytes()), Ordering::SeqCst);

                                if !verify_auth(
                                    &msghdr.uid,
                                    &msghdr.channel,
                                    msghdr.auth.as_deref(),
                                ) {
                                    log::warn!(
                                        "Authentication failed for user {} on channel {}",
                                        h.load(Ordering::SeqCst),
                                        ch.load(Ordering::SeqCst)
                                    );
                                    is_closed_rx.store(true, Ordering::SeqCst);
                                    return;
                                }

                                if let Err(err) = tx
                                    .send(WsEvent::Init(
                                        h.load(Ordering::SeqCst),
                                        ch.load(Ordering::SeqCst),
                                        tx2_spawn.clone(),
                                        err_tx,
                                        msg,
                                    ))
                                    .await
                                {
                                    log::debug!("Failed to send init event: {}", err);
                                    is_closed_rx.store(true, Ordering::SeqCst);
                                    return;
                                }
                            }
                            Err(err) => {
                                log::debug!("Invalid header format: {}", err);
                                is_closed_rx.store(true, Ordering::SeqCst);
                                return;
                            }
                        }
                    }
                    match err_rx.await {
                        Ok(_) => {}
                        Err(_) => {
                            log::debug!("Duplicate entry or init error, closing");
                            is_closed_rx.store(true, Ordering::SeqCst);
                            h.store(0, Ordering::SeqCst);
                            ch.store(0, Ordering::SeqCst);
                            let _ = tx2_spawn.send(Some(Ok(Message::close()))).await;
                            return;
                        }
                    }

                    while let Some(Ok(msg)) = ws_rx.next().await {
                        if is_closed_rx.load(Ordering::SeqCst) {
                            break;
                        }

                        if 0 == h.load(Ordering::SeqCst) {
                            log::debug!("Invalid connection state, closing");
                            break;
                        }

                        if msg.is_close() {
                            log::debug!("Received close frame");
                            is_closed_rx.store(true, Ordering::SeqCst);
                            break;
                        }

                        ping_cntr_inner.store(0, Ordering::Relaxed);
                        if msg.is_pong() {
                            log::debug!(
                                "Got pong for {:x} of {:x}",
                                h.load(Ordering::SeqCst),
                                ch.load(Ordering::SeqCst)
                            );
                            continue;
                        }

                        if let Err(err) = tx
                            .send(WsEvent::Msg(
                                h.load(Ordering::SeqCst),
                                ch.load(Ordering::SeqCst),
                                msg,
                            ))
                            .await
                        {
                            log::debug!("Failed to send message: {}", err);
                            break;
                        }
                    }
                    is_closed_rx.store(true, Ordering::SeqCst);
                    let _ = tx2_spawn.send(None).await;
                });

                // Ping handler task
                let tx2_inner = tx2.clone();
                let ping_cntr_inner = ping_cntr.clone();
                let is_closed_ping = is_closed.clone();
                tokio::spawn(async move {
                    let mut interval = time::interval(Duration::from_millis(PING_INTERVAL));
                    interval.tick().await;

                    while !is_closed_ping.load(Ordering::SeqCst) {
                        interval.tick().await;

                        if let Err(_) = tx2_inner.send(Some(Ok(Message::ping(Vec::new())))).await {
                            break;
                        }

                        let ping_cnt = ping_cntr_inner.fetch_add(1, Ordering::Relaxed);
                        if ping_cnt > 2 {
                            log::debug!("No pongs received, closing connection");
                            is_closed_ping.store(true, Ordering::SeqCst);
                            let _ = tx2_inner.send(None).await;
                            break;
                        }
                    }
                });

                // Message sender task
                let is_closed_tx = is_closed.clone();
                async move {
                    while let Some(Some(Ok(msg))) = rx2.next().await {
                        if is_closed_tx.load(Ordering::SeqCst) {
                            break;
                        }

                        match ws_tx.send(msg).await {
                            Ok(_) => {}
                            Err(err) => {
                                // Handle specific error cases
                                if err.to_string().contains("broken pipe")
                                    || err.to_string().contains("connection reset")
                                    || err.to_string().contains("protocol error")
                                    || err.to_string().contains("sending after closing")
                                {
                                    log::debug!("Connection closed: {}", err);
                                } else {
                                    log::warn!("Unexpected websocket error: {}", err);
                                }
                                is_closed_tx.store(true, Ordering::SeqCst);
                                break;
                            }
                        }
                    }

                    // Final cleanup
                    let _ = ws_tx.send(Message::close()).await;
                    let hval = h.load(Ordering::SeqCst);
                    let chval = ch.load(Ordering::SeqCst);
                    if hval != 0 && chval != 0 {
                        let _ = tx_inner.send(WsEvent::Logoff(hval, chval)).await;
                        h.store(0, Ordering::SeqCst);
                        ch.store(0, Ordering::SeqCst);
                    }
                }
            })
        })
        .with(warp::reply::with::header(
            "Sec-WebSocket-Protocol",
            ACCEPTED_PROTOCOL,
        ))
}
