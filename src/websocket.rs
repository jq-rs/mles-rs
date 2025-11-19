/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use crate::auth::verify_auth;
use crate::metrics::Metrics;
use crate::types::{ChannelInfo, MlesHeader, WsEvent};

use futures_util::{SinkExt, StreamExt};
use siphasher::sip::SipHasher;
use std::collections::{HashMap, VecDeque, hash_map::Entry};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use warp::Filter;
use warp::ws::Message;

pub(crate) const ACCEPTED_PROTOCOL: &str = "mles-websocket";
const WS_BUF: usize = 128;
const CHANNEL_CLEANUP_DAYS: u64 = 30;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60); // Run once per day
const INIT_TIMEOUT: Duration = Duration::from_secs(30); // Timeout for initial message
const MAX_MESSAGE_SIZE: usize = 1024 * 1024; // 1MB max per message
const SLOW_CLIENT_THRESHOLD: usize = WS_BUF - 10; // Buffer nearly full
const MESSAGE_HISTORY_MAX_BYTES: usize = 10 * 1024 * 1024; // 10MB total history per channel

// Configurable settings
#[derive(Clone, Debug)]
pub struct WebSocketConfig {
    pub ping_interval_ms: u64,
    pub max_missed_pongs: u64,
    pub rate_limit_messages_per_sec: u32,
    pub rate_limit_window_secs: u64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            ping_interval_ms: 24000,
            max_missed_pongs: 3,
            rate_limit_messages_per_sec: 100,
            rate_limit_window_secs: 1,
        }
    }
}

use tokio::sync::mpsc::Sender;

// Rate limiter for per-connection message throttling
struct RateLimiter {
    messages: VecDeque<SystemTime>,
    max_messages: u32,
    window: Duration,
}

impl RateLimiter {
    fn new(max_messages: u32, window_secs: u64) -> Self {
        Self {
            messages: VecDeque::new(),
            max_messages,
            window: Duration::from_secs(window_secs),
        }
    }

    fn check_and_add(&mut self) -> bool {
        let now = SystemTime::now();

        // Remove old entries outside the window
        while let Some(front) = self.messages.front() {
            if let Ok(elapsed) = now.duration_since(*front) {
                if elapsed > self.window {
                    self.messages.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Check if we're at the limit
        if self.messages.len() >= self.max_messages as usize {
            return false;
        }

        self.messages.push_back(now);
        true
    }
}

pub(crate) fn add_message(
    msg: Message,
    limit: u32,
    queue: &mut VecDeque<Message>,
    current_bytes: &mut usize,
) -> bool {
    let msg_size = msg.as_bytes().len();

    // Check if adding this message would exceed memory limit
    if *current_bytes + msg_size > MESSAGE_HISTORY_MAX_BYTES {
        // Remove old messages until we have space
        while !queue.is_empty() && *current_bytes + msg_size > MESSAGE_HISTORY_MAX_BYTES {
            if let Some(old_msg) = queue.pop_front() {
                *current_bytes = current_bytes.saturating_sub(old_msg.as_bytes().len());
            }
        }
    }

    let limit = limit as usize;
    let len = queue.len();
    let cap = queue.capacity();
    if cap < limit && len == cap && cap * 2 >= limit {
        queue.reserve(limit - queue.capacity())
    }
    if len == limit {
        if let Some(old_msg) = queue.pop_front() {
            *current_bytes = current_bytes.saturating_sub(old_msg.as_bytes().len());
        }
    }

    *current_bytes += msg_size;
    queue.push_back(msg);
    true
}

pub(crate) fn spawn_event_loop(
    mut rx: ReceiverStream<WsEvent>,
    limit: u32,
    metrics: Metrics,
    shutdown: CancellationToken,
) {
    tokio::spawn(async move {
        let mut msg_db: HashMap<u64, ChannelInfo> = HashMap::new();
        let mut ch_db: HashMap<u64, HashMap<u64, Sender<Option<Result<Message, warp::Error>>>>> =
            HashMap::new();
        let mut history_bytes: HashMap<u64, usize> = HashMap::new();

        // Create cleanup interval
        let mut cleanup_interval = time::interval(CLEANUP_INTERVAL);

        // Metrics update interval
        let mut metrics_interval = time::interval(Duration::from_secs(10));

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    log::info!("Shutting down event loop, notifying {} channels", ch_db.len());

                    // Notify all clients about shutdown
                    for (ch, uid_db) in ch_db.iter() {
                        log::info!("Notifying channel {ch:x} about shutdown");
                        for (uid, tx) in uid_db.iter() {
                            log::debug!("Notifying user {uid:x} in channel {ch:x}");
                            let _ = tx.send(Some(Ok(Message::close()))).await;
                        }
                    }

                    log::info!("Event loop shutdown complete");
                    break;
                }

                // Update metrics periodically
                _ = metrics_interval.tick() => {
                    metrics.update_channel_count(ch_db.len());
                    let stats = metrics.get_stats();
                    log::debug!(
                        "Metrics: {} active connections, {} channels, {} msgs sent, {} msgs received, {} errors",
                        stats.active_connections,
                        stats.total_channels,
                        stats.total_messages_sent,
                        stats.total_messages_received,
                        stats.total_errors
                    );
                }

                // Handle cleanup
                _ = cleanup_interval.tick() => {
                    let now = SystemTime::now();
                    let threshold = Duration::from_secs(CHANNEL_CLEANUP_DAYS * 24 * 60 * 60);

                    msg_db.retain(|&ch, info| {
                        if let Ok(elapsed) = now.duration_since(info.last_activity) {
                            if elapsed > threshold {
                                log::info!("Cleaning up inactive channel {ch:x}, last activity {} days ago",
                                    elapsed.as_secs() / (24 * 60 * 60));
                                history_bytes.remove(&ch);
                                return false; // Remove channel
                            }
                        }
                        true // Keep channel
                    });

                    metrics.update_channel_count(msg_db.len());
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
                                    log::info!("Added user {h:x} into channel {ch:x}, total users in channel: {}", uid_db.len());

                                    // Send confirmation through err_tx
                                    if let Err(err) = err_tx.send(h) {
                                        log::debug!("Failed to send init confirmation for {h:x}: {}", err);
                                    }

                                    // Send message history
                                    for qmsg in &channel_info.messages {
                                        let res = tx2.send(Some(Ok(qmsg.clone()))).await;
                                        if let Err(err) = res {
                                            log::warn!("Failed to send history message to {h:x} on {ch:x}: {}", err);
                                            metrics.increment_errors();
                                        } else {
                                            metrics.increment_messages_sent();
                                        }
                                    }

                                    let current_bytes = history_bytes.entry(ch).or_insert(0);
                                    add_message(msg, limit, &mut channel_info.messages, current_bytes);
                                    channel_info.last_activity = SystemTime::now();

                                    log::debug!("Channel {ch:x} history: {} messages, {} bytes",
                                        channel_info.messages.len(), current_bytes);
                                } else {
                                    log::warn!("Duplicate connection attempt for {h:x} in channel {ch:x}, rejecting");
                                    metrics.increment_errors();
                                    // Notify about duplicate connection
                                    drop(err_tx);
                                }
                            }
                        }
                        WsEvent::Msg(h, ch, msg) => {
                            if let Some(uid_db) = ch_db.get(&ch) {
                                let recipient_count = uid_db.len() - 1; // Exclude sender
                                log::debug!("Broadcasting message from {h:x} in channel {ch:x} to {} recipients", recipient_count);

                                for (uid, tx) in uid_db.iter().filter(|(xh, _)| **xh != h) {
                                    let res = tx.send(Some(Ok(msg.clone()))).await;
                                    if let Err(err) = res {
                                        log::warn!("Failed to send message to {uid:x} in channel {ch:x}: {}", err);
                                        metrics.increment_errors();
                                    } else {
                                        metrics.increment_messages_sent();
                                    }
                                }
                                if let Some(channel_info) = msg_db.get_mut(&ch) {
                                    let current_bytes = history_bytes.entry(ch).or_insert(0);
                                    add_message(msg, limit, &mut channel_info.messages, current_bytes);
                                    channel_info.last_activity = SystemTime::now();
                                }
                            } else {
                                log::warn!("Received message for non-existent channel {ch:x} from user {h:x}");
                                metrics.increment_errors();
                            }
                        }
                        WsEvent::Logoff(h, ch) => {
                            if let Some(uid_db) = ch_db.get_mut(&ch) {
                                uid_db.remove(&h);
                                let remaining = uid_db.len();
                                if uid_db.is_empty() {
                                    ch_db.remove(&ch);
                                    log::info!("Removed user {h:x} from channel {ch:x}, channel now empty and removed");
                                } else {
                                    log::info!("Removed user {h:x} from channel {ch:x}, {} users remaining", remaining);
                                }
                            } else {
                                log::debug!("Logoff for {h:x} in non-existent channel {ch:x}");
                            }
                        }
                    }
                }
            }
        }
    });
}

pub(crate) fn create_ws_handler(
    tx_clone: Sender<WsEvent>,
    config: WebSocketConfig,
    metrics: Metrics,
) -> impl warp::Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::ws()
        .and(warp::header::exact(
            "Sec-WebSocket-Protocol",
            ACCEPTED_PROTOCOL,
        ))
        .map(move |ws: warp::ws::Ws| {
            let tx_inner = tx_clone.clone();
            let config = config.clone();
            let metrics = metrics.clone();
            let mles_key = std::env::var("MLES_KEY").ok();

            ws.on_upgrade(move |websocket| {
                metrics.increment_connections();
                let connection_start = SystemTime::now();

                let (tx2, rx2) = mpsc::channel::<Option<Result<Message, warp::Error>>>(WS_BUF);
                let (err_tx, err_rx) = oneshot::channel();
                let mut rx2 = ReceiverStream::new(rx2);
                let (mut ws_tx, mut ws_rx) = websocket.split();
                let h = Arc::new(AtomicU64::new(0));
                let ch = Arc::new(AtomicU64::new(0));
                let ping_cntr = Arc::new(AtomicU64::new(0));
                let is_closed = Arc::new(AtomicBool::new(false));
                let metrics_clone = metrics.clone();

                // Message receiver task
                let tx = tx_inner.clone();
                let tx2_inner = tx2.clone();
                let h_clone = h.clone();
                let ch_clone = ch.clone();
                let ping_cntr_clone = ping_cntr.clone();
                let is_closed_rx = is_closed.clone();
                let metrics_rx = metrics.clone();

                tokio::spawn(async move {
                    let h = h_clone;
                    let ch = ch_clone;
                    let tx2_spawn = tx2_inner.clone();
                    let ping_cntr_inner = ping_cntr_clone;
                    let mut rate_limiter = RateLimiter::new(
                        config.rate_limit_messages_per_sec,
                        config.rate_limit_window_secs,
                    );

                    // Wait for initial message with timeout
                    match time::timeout(INIT_TIMEOUT, ws_rx.next()).await {
                        Ok(Some(Ok(msg))) => {
                            if !msg.is_text() {
                                log::warn!("First message is not text, rejecting connection");
                                is_closed_rx.store(true, Ordering::SeqCst);
                                metrics_rx.increment_errors();
                                return;
                            }

                            // Validate message size
                            if msg.as_bytes().len() > MAX_MESSAGE_SIZE {
                                log::warn!("Init message exceeds max size of {} bytes", MAX_MESSAGE_SIZE);
                                is_closed_rx.store(true, Ordering::SeqCst);
                                metrics_rx.increment_errors();
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
                                        metrics_rx.increment_errors();
                                        return;
                                    }
                                    if msghdr.uid.is_empty() || msghdr.channel.is_empty() {
                                        log::warn!("Empty uid or channel");
                                        is_closed_rx.store(true, Ordering::SeqCst);
                                        metrics_rx.increment_errors();
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
                                        mles_key.as_deref(),
                                    ) {
                                        log::warn!(
                                            "Authentication failed for user {:x} on channel {:x}",
                                            h.load(Ordering::SeqCst),
                                            ch.load(Ordering::SeqCst)
                                        );
                                        is_closed_rx.store(true, Ordering::SeqCst);
                                        metrics_rx.increment_errors();
                                        return;
                                    }

                                    log::info!(
                                        "Connection initialized: user={:x}, channel={:x}, uid={}, channel_name={}",
                                        h.load(Ordering::SeqCst),
                                        ch.load(Ordering::SeqCst),
                                        msghdr.uid,
                                        msghdr.channel
                                    );

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
                                        log::warn!("Failed to send init event for {:x} on {:x}: {}",
                                            h.load(Ordering::SeqCst), ch.load(Ordering::SeqCst), err);
                                        is_closed_rx.store(true, Ordering::SeqCst);
                                        metrics_rx.increment_errors();
                                        return;
                                    }
                                }
                                Err(err) => {
                                    log::warn!("Invalid header format: {}", err);
                                    is_closed_rx.store(true, Ordering::SeqCst);
                                    metrics_rx.increment_errors();
                                    return;
                                }
                            }
                        }
                        Ok(Some(Err(e))) => {
                            log::warn!("WebSocket error during init: {}", e);
                            is_closed_rx.store(true, Ordering::SeqCst);
                            metrics_rx.increment_errors();
                            return;
                        }
                        Ok(None) => {
                            log::debug!("Connection closed before init");
                            is_closed_rx.store(true, Ordering::SeqCst);
                            return;
                        }
                        Err(_) => {
                            log::warn!("Client failed to send init message within {}s timeout", INIT_TIMEOUT.as_secs());
                            is_closed_rx.store(true, Ordering::SeqCst);
                            metrics_rx.increment_errors();
                            return;
                        }
                    }

                    match err_rx.await {
                        Ok(_) => {}
                        Err(_) => {
                            log::warn!("Duplicate entry or init error for {:x} on {:x}, closing connection",
                                h.load(Ordering::SeqCst), ch.load(Ordering::SeqCst));
                            is_closed_rx.store(true, Ordering::SeqCst);
                            h.store(0, Ordering::SeqCst);
                            ch.store(0, Ordering::SeqCst);
                            let _ = tx2_spawn.send(Some(Ok(Message::close()))).await;
                            metrics_rx.increment_errors();
                            return;
                        }
                    }

                    while let Some(Ok(msg)) = ws_rx.next().await {
                        if is_closed_rx.load(Ordering::SeqCst) {
                            break;
                        }

                        if 0 == h.load(Ordering::SeqCst) {
                            log::warn!("Invalid connection state for {:x}, closing", h.load(Ordering::SeqCst));
                            metrics_rx.increment_errors();
                            break;
                        }

                        if msg.is_close() {
                            log::debug!("Received close frame from {:x} on {:x}",
                                h.load(Ordering::SeqCst), ch.load(Ordering::SeqCst));
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

                        // Validate message size
                        if msg.as_bytes().len() > MAX_MESSAGE_SIZE {
                            log::warn!("Message from {:x} on {:x} exceeds max size of {} bytes, dropping",
                                h.load(Ordering::SeqCst), ch.load(Ordering::SeqCst), MAX_MESSAGE_SIZE);
                            metrics_rx.increment_errors();
                            continue;
                        }

                        // Rate limiting
                        if !rate_limiter.check_and_add() {
                            log::warn!("Rate limit exceeded for {:x} on {:x}, dropping message",
                                h.load(Ordering::SeqCst), ch.load(Ordering::SeqCst));
                            metrics_rx.increment_errors();
                            continue;
                        }

                        metrics_rx.increment_messages_received();

                        if let Err(err) = tx
                            .send(WsEvent::Msg(
                                h.load(Ordering::SeqCst),
                                ch.load(Ordering::SeqCst),
                                msg,
                            ))
                            .await
                        {
                            log::warn!("Failed to send message from {:x} on {:x}: {}",
                                h.load(Ordering::SeqCst), ch.load(Ordering::SeqCst), err);
                            metrics_rx.increment_errors();
                            break;
                        }
                    }
                    is_closed_rx.store(true, Ordering::SeqCst);
                    let _ = tx2_spawn.send(None).await;
                });

                // Ping handler task
                let tx2_inner = tx2.clone();
                let h_clone = h.clone();
                let ch_clone = ch.clone();
                let ping_cntr_inner = ping_cntr.clone();
                let is_closed_ping = is_closed.clone();
                let max_missed_pongs = config.max_missed_pongs;

                tokio::spawn(async move {
                    let mut interval = time::interval(Duration::from_millis(config.ping_interval_ms));
                    interval.tick().await;

                    while !is_closed_ping.load(Ordering::SeqCst) {
                        interval.tick().await;

                        if let Err(_) = tx2_inner.send(Some(Ok(Message::ping(Vec::new())))).await {
                            break;
                        }

                        let ping_cnt = ping_cntr_inner.fetch_add(1, Ordering::Relaxed);
                        if ping_cnt >= max_missed_pongs {
                            log::warn!("No pongs received from {:x} on {:x} after {} attempts, closing connection",
                                h_clone.load(Ordering::SeqCst), ch_clone.load(Ordering::SeqCst), ping_cnt);
                            is_closed_ping.store(true, Ordering::SeqCst);
                            let _ = tx2_inner.send(None).await;
                            break;
                        }
                    }
                });

                // Message sender task
                let is_closed_tx = is_closed.clone();
                let h_sender = h.clone();
                let ch_sender = ch.clone();
                let metrics_tx = metrics_clone.clone();

                async move {
                    let mut slow_client_warnings = 0u32;
                    let mut pending_count = 0usize;

                    while let Some(Some(Ok(msg))) = rx2.next().await {
                        if is_closed_tx.load(Ordering::SeqCst) {
                            break;
                        }

                        // Approximate backpressure check
                        pending_count += 1;
                        if pending_count > SLOW_CLIENT_THRESHOLD {
                            slow_client_warnings += 1;
                            if slow_client_warnings % 10 == 1 {
                                log::warn!("Slow client detected: {:x} on {:x}, approximate pending {}/{} (warning #{})",
                                    h_sender.load(Ordering::SeqCst),
                                    ch_sender.load(Ordering::SeqCst),
                                    pending_count, WS_BUF, slow_client_warnings);
                            }
                        }

                        match ws_tx.send(msg).await {
                            Ok(_) => {
                                pending_count = pending_count.saturating_sub(1);
                            }
                            Err(err) => {
                                // Handle specific error cases
                                if err.to_string().contains("broken pipe")
                                    || err.to_string().contains("connection reset")
                                    || err.to_string().contains("protocol error")
                                    || err.to_string().contains("sending after closing")
                                {
                                    log::debug!("Connection closed for {:x} on {:x}: {}",
                                        h_sender.load(Ordering::SeqCst),
                                        ch_sender.load(Ordering::SeqCst), err);
                                } else {
                                    log::warn!("Unexpected websocket error for {:x} on {:x}: {}",
                                        h_sender.load(Ordering::SeqCst),
                                        ch_sender.load(Ordering::SeqCst), err);
                                }
                                is_closed_tx.store(true, Ordering::SeqCst);
                                metrics_tx.increment_errors();
                                break;
                            }
                        }
                    }

                    // Final cleanup
                    let _ = ws_tx.send(Message::close()).await;
                    let hval = h_sender.load(Ordering::SeqCst);
                    let chval = ch_sender.load(Ordering::SeqCst);
                    if hval != 0 && chval != 0 {
                        if let Ok(duration) = SystemTime::now().duration_since(connection_start) {
                            log::info!("Connection closed: user={hval:x}, channel={chval:x}, duration={}s",
                                duration.as_secs());
                        }
                        let _ = tx_inner.send(WsEvent::Logoff(hval, chval)).await;
                        h_sender.store(0, Ordering::SeqCst);
                        ch_sender.store(0, Ordering::SeqCst);
                    }

                    metrics_clone.decrement_connections();
                }
            })
        })
        .with(warp::reply::with::header(
            "Sec-WebSocket-Protocol",
            ACCEPTED_PROTOCOL,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::Duration;

    #[test]
    fn rate_limiter_allows_then_blocks_then_allows_after_window() {
        // allow 2 messages per 1 second window
        let mut rl = RateLimiter::new(2, 1);

        // first two should be allowed
        assert!(rl.check_and_add());
        assert!(rl.check_and_add());

        // third should be blocked (exceeds limit)
        assert!(!rl.check_and_add());

        // wait for the window to expire
        std::thread::sleep(Duration::from_millis(1100));

        // now should be allowed again
        assert!(rl.check_and_add());
    }

    #[test]
    fn add_message_basic_and_eviction() {
        let mut queue: VecDeque<Message> = VecDeque::new();
        let mut bytes = 0usize;
        let msg1 = Message::text("hello");
        let msg2 = Message::text("world!!!");

        // Add first message with limit 1
        assert!(add_message(msg1.clone(), 1, &mut queue, &mut bytes));
        assert_eq!(queue.len(), 1);
        assert_eq!(bytes, msg1.as_bytes().len());

        // Add second message; with limit == 1 the first should be evicted
        assert!(add_message(msg2.clone(), 1, &mut queue, &mut bytes));
        assert_eq!(queue.len(), 1);
        assert_eq!(bytes, msg2.as_bytes().len());

        // Ensure the remaining message is the second one
        let front = queue.pop_front().unwrap();
        assert_eq!(front.as_bytes(), msg2.as_bytes());
    }
}
