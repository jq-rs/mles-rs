/* This Source Code Form is subject to the terms of the Mozilla Public
* License, v. 2.0. If a copy of the MPL was not distributed with this
* file, You can obtain one at http://mozilla.org/MPL/2.0/.
*
*  Copyright (C) 2023-2025  Mles developers
*/
use async_compression::Level::Precise;
use async_compression::tokio::write::{BrotliEncoder, ZstdEncoder};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use http_types::mime;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use log::LevelFilter;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use serde::{Deserialize, Serialize};
use simple_logger::SimpleLogger;
use siphasher::sip::SipHasher;
use std::collections::VecDeque;
use std::collections::{HashMap, hash_map::Entry};
use std::hash::{Hash, Hasher};
use std::io;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpSocket;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::time;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::wrappers::TcpListenerStream;
use warp::Filter;
use warp::filters::BoxedFilter;
use warp::http::StatusCode;
use warp::ws::Message;

mod mina;

const BR: &str = "br";
const ZSTD: &str = "zstd";
const ACCEPTED_PROTOCOL: &str = "mles-websocket";
const TASK_BUF: usize = 16;
const WS_BUF: usize = 128;
const HISTORY_LIMIT: &str = "200";
const MAX_FILES_OPEN: &str = "256";
const TLS_PORT: &str = "443";
const PING_INTERVAL: u64 = 24000;
const BACKLOG: u32 = 1024;

const CHANNEL_CLEANUP_DAYS: u64 = 30;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60); // Run once per day

#[derive(Serialize, Deserialize, Hash)]
struct MlesHeader {
    uid: String,
    channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<String>,
}

struct ChannelInfo {
    messages: VecDeque<Message>,
    last_activity: SystemTime,
}

#[derive(Parser, Debug)]
struct Args {
    /// Domain(s)
    #[arg(short, long, required = true)]
    domains: Vec<String>,

    /// Contact info
    #[arg(short, long)]
    email: Vec<String>,

    /// Cache directory
    #[arg(short, long)]
    cache: Option<PathBuf>,

    /// History limit
    #[arg(short, long, default_value = HISTORY_LIMIT, value_parser = clap::value_parser!(u32).range(1..1_000_000))]
    limit: u32,

    /// Open files limit
    #[arg(short, long, default_value = MAX_FILES_OPEN, value_parser = clap::value_parser!(u32).range(1..1_000_000))]
    filelimit: u32,

    /// Www-root directory for domain(s)
    #[arg(short, long, required = true)]
    wwwroot: PathBuf,

    /// Use Let's Encrypt staging environment
    #[arg(short, long)]
    staging: bool,

    #[arg(short, long, default_value = TLS_PORT, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    /// Use http redirect for port 80
    #[arg(short, long)]
    redirect: bool,
}

#[derive(Debug)]
enum WsEvent {
    Init(
        u64,
        u64,
        Sender<Option<Result<Message, warp::Error>>>,
        oneshot::Sender<u64>,
        Message,
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

fn create_tcp_incoming(addr: SocketAddr) -> io::Result<TcpListenerStream> {
    let socket = TcpSocket::new_v6()?;
    socket.set_keepalive(true)?;
    socket.set_nodelay(true)?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    let tcp_listener = socket.listen(BACKLOG)?;
    Ok(TcpListenerStream::new(tcp_listener))
}

fn verify_auth(uid: &str, channel: &str, auth: Option<&str>) -> bool {
    let mut hasher = SipHasher::new();
    hasher.write(uid.as_bytes());
    hasher.write(channel.as_bytes());

    if let Ok(key) = std::env::var("MLES_KEY") {
        if let Some(auth) = auth {
            hasher.write(key.as_bytes());
            let hash = hasher.finish();
            let auth_hash = u64::from_str_radix(auth, 16).unwrap_or(0);
            hash == auth_hash
        } else {
            false
        }
    } else {
        if let Some(auth) = auth {
            let hash = hasher.finish();
            let auth_hash = u64::from_str_radix(auth, 16).unwrap_or(0);
            return hash == auth_hash;
        }
        true
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Warn)
        .env()
        .init()
        .unwrap();
    let args = Args::parse();
    let limit = args.limit;
    let filelimit = args.filelimit;
    let www_root_dir = args.wwwroot;
    let semaphore = Arc::new(Semaphore::new(filelimit as usize));

    let (tx, rx) = mpsc::channel::<WsEvent>(TASK_BUF);
    let mut rx = ReceiverStream::new(rx);

    let addr = format!("[{}]:{}", Ipv6Addr::UNSPECIFIED, args.port)
        .parse()
        .unwrap();

    let tcp_incoming = create_tcp_incoming(addr)?;

    let tls_incoming = AcmeConfig::new(args.domains.clone())
        .contact(args.email.iter().map(|e| format!("mailto:{}", e)))
        .cache_option(args.cache.clone().map(DirCache::new))
        .directory_lets_encrypt(!args.staging)
        .tokio_incoming(tcp_incoming, Vec::new());

    // Wrap the incoming connections stream with a filter to enforce a connection limit
    let sem_inner = semaphore.clone();
    let tls_incoming = tls_incoming.filter_map(move |conn| {
        let sem = sem_inner.clone();
        async move {
            let _permit = sem.acquire().await;
            Some(conn)
        }
    });

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

    let tx_clone = tx.clone();
    let ws = warp::ws()
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
        ));

    if args.redirect {
        let domains = args.domains.clone();
        let mut http_index = Vec::new();
        for domain in domains {
            let redirect = warp::get()
                .and(warp::header::<String>("host"))
                .and(warp::path::tail())
                .map(move |uri: String, path: warp::path::Tail| (uri, domain.clone(), path))
                .and_then(dyn_hreply);
            http_index.push(redirect);
        }

        let mut hindex: BoxedFilter<_> = http_index.swap_remove(0).boxed();
        for val in http_index {
            hindex = val.or(hindex).unify().boxed();
        }

        tokio::spawn(async move {
            let addr = format!("[{}]:{}", Ipv6Addr::UNSPECIFIED, 80)
                .parse()
                .unwrap();
            if let Ok(tcp_incoming) = create_tcp_incoming(addr) {
                // Manual HTTP server loop for redirect
                let service = warp::service(hindex);

                tokio::pin!(tcp_incoming);
                while let Some(Ok(stream)) = tcp_incoming.next().await {
                    let io = TokioIo::new(stream);
                    let service_clone = TowerToHyperService::new(service.clone());

                    tokio::spawn(async move {
                        if let Err(err) = http1::Builder::new()
                            .serve_connection(io, service_clone)
                            .await
                        {
                            log::debug!("Error serving connection: {:?}", err);
                        }
                    });
                }
            }
        });
    }

    let mut vindex = Vec::new();
    for domain in args.domains {
        let sem = semaphore.clone();
        let www_root = www_root_dir.clone();
        let index = warp::get()
            .and(warp::header::optional::<String>("accept-encoding"))
            .and(warp::header::<String>("host"))
            .and(warp::path::tail())
            .map(
                move |encoding: Option<String>, uri: String, path: warp::path::Tail| {
                    (
                        encoding,
                        uri,
                        domain.clone(),
                        www_root.to_str().unwrap().to_string(),
                        path,
                        sem.clone(),
                    )
                },
            )
            .and_then(dyn_reply);

        vindex.push(index);
    }

    let mut index: BoxedFilter<_> = vindex.swap_remove(0).boxed();
    for val in vindex {
        index = val.or(index).unify().boxed();
    }

    // Define the route that serves the file status page
    let page_route = warp::path("mina_status")
        .and(warp::get())
        .and_then(mina::serve_status_page);
    let tlsroutes = page_route.or(ws).or(index);

    // Manual HTTPS/TLS server loop
    let service = warp::service(tlsroutes);

    tokio::pin!(tls_incoming);
    while let Some(Ok(stream)) = tls_incoming.next().await {
        let io = TokioIo::new(stream);
        let service_clone = TowerToHyperService::new(service.clone());

        tokio::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .serve_connection(io, service_clone)
                .with_upgrades()
                .await
            {
                log::debug!("Error serving TLS connection: {:?}", err);
            }
        });
    }

    unreachable!()
}

async fn dyn_hreply(
    tuple: (String, String, warp::path::Tail),
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let (uri, domain, tail) = tuple;

    if uri != domain {
        return Err(warp::reject::not_found());
    }

    Ok(Box::new(warp::redirect::redirect(
        warp::http::Uri::from_str(&format!("https://{}/{}", &domain, tail.as_str()))
            .expect("problem with uri?"),
    )))
}

async fn compress(comptype: &str, in_data: &[u8]) -> std::io::Result<Vec<u8>> {
    if comptype == ZSTD {
        let mut encoder = ZstdEncoder::with_quality(Vec::new(), Precise(2));
        encoder.write_all(in_data).await?;
        encoder.shutdown().await?;
        Ok(encoder.into_inner())
    } else {
        let mut encoder = BrotliEncoder::with_quality(Vec::new(), Precise(2));
        encoder.write_all(in_data).await?;
        encoder.shutdown().await?;
        Ok(encoder.into_inner())
    }
}

#[derive(Debug)]
enum ReplyHeaders {
    NONE,
    Zstd,
    Br,
    AllowOrigin,
    ZstdWithAllowOrigin,
    BrWithAllowOrigin,
}

async fn dyn_reply(
    tuple: (
        Option<String>,
        String,
        String,
        String,
        warp::path::Tail,
        Arc<Semaphore>,
    ),
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let (encoding, uri, domain, www_root, tail, semaphore) = tuple;

    if uri != domain {
        return Err(warp::reject::not_found());
    }
    let mut path = tail.as_str();
    if path.is_empty() {
        path = "index.html";
    }
    let file_path = format!("{}/{}/{}", www_root, uri, path);

    let _permit = semaphore.acquire().await.unwrap();

    log::debug!("Avail file permits {}", semaphore.available_permits());
    log::debug!("Accessing {file_path}...");

    match File::open(&file_path).await {
        Ok(mut file) => {
            let parts: Vec<&str> = file_path.split('.').collect();
            let ctype = match parts.last() {
                Some(v) => {
                    let mime = mime::Mime::from_extension(*v);
                    match mime {
                        Some(mime) => format!("{}/{}", mime.basetype(), mime.subtype()),
                        None => match *v {
                            "png" => format!("{}/{}", mime::PNG.basetype(), mime::PNG.subtype()),
                            "jpg" => format!("{}/{}", mime::JPEG.basetype(), mime::JPEG.subtype()),
                            "ico" => format!("{}/{}", mime::ICO.basetype(), mime::ICO.subtype()),
                            "apk" => "application/vnd.android.package-archive".to_string(),
                            _ => format!("{}/{}", mime::ANY.basetype(), mime::ANY.subtype()),
                        },
                    }
                }
                None => format!("{}/{}", mime::ANY.basetype(), mime::ANY.subtype()),
            };

            let mut buffer = Vec::new();
            if (file.read_to_end(&mut buffer).await).is_err() {
                log::debug!("...FAILED!");
                return Ok(Box::new(warp::reply::with_status(
                    "Internal Server Error",
                    StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
            log::debug!("...OK, ctype {ctype}");

            let mut reply_headers = ReplyHeaders::NONE;
            let mut use_br = false;
            let mut use_zstd = false;
            if let Some(encoding) = encoding {
                log::debug!("Encoding: {encoding}");
                if ctype.contains("text") || ctype.contains("json") {
                    if encoding.contains(ZSTD) {
                        if let Ok(compressed_buffer) = compress(ZSTD, &buffer).await {
                            buffer = compressed_buffer;
                            reply_headers = ReplyHeaders::Zstd;
                            use_zstd = true;
                        }
                    } else if encoding.contains(BR) {
                        if let Ok(compressed_buffer) = compress(BR, &buffer).await {
                            buffer = compressed_buffer;
                            reply_headers = ReplyHeaders::Br;
                            use_br = true;
                        }
                    }
                }
            }
            let mut use_allow_origin = false;
            if ctype.contains("json") {
                use_allow_origin = true;
                reply_headers = ReplyHeaders::AllowOrigin;
            }
            if use_zstd && use_allow_origin {
                reply_headers = ReplyHeaders::ZstdWithAllowOrigin;
            } else if use_br && use_allow_origin {
                reply_headers = ReplyHeaders::BrWithAllowOrigin;
            }
            log::debug!("Reply headers: {reply_headers:?}");
            match reply_headers {
                ReplyHeaders::NONE => Ok(Box::new(warp::reply::with_header(
                    warp::reply::Response::new(buffer.into()),
                    "Content-Type",
                    &ctype,
                ))),
                ReplyHeaders::Br => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::Response::new(buffer.into()),
                        "Content-Type",
                        &ctype,
                    ),
                    "Content-Encoding",
                    BR,
                ))),
                ReplyHeaders::Zstd => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::Response::new(buffer.into()),
                        "Content-Type",
                        &ctype,
                    ),
                    "Content-Encoding",
                    ZSTD,
                ))),
                ReplyHeaders::AllowOrigin => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::Response::new(buffer.into()),
                        "Content-Type",
                        &ctype,
                    ),
                    "Access-Control-Allow-Origin",
                    "*",
                ))),
                ReplyHeaders::ZstdWithAllowOrigin => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::with_header(
                            warp::reply::Response::new(buffer.into()),
                            "Content-Type",
                            &ctype,
                        ),
                        "Content-Encoding",
                        ZSTD,
                    ),
                    "Access-Control-Allow-Origin",
                    "*",
                ))),
                ReplyHeaders::BrWithAllowOrigin => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::with_header(
                            warp::reply::Response::new(buffer.into()),
                            "Content-Type",
                            &ctype,
                        ),
                        "Content-Encoding",
                        BR,
                    ),
                    "Access-Control-Allow-Origin",
                    "*",
                ))),
            }
        }
        Err(_) => {
            log::debug!("{file_path} does not exist");
            Ok(Box::new(warp::reply::with_status(
                "Not Found",
                StatusCode::NOT_FOUND,
            )))
        }
    }
}
