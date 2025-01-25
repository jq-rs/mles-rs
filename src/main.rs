/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2024  Mles developers
 */
use async_compression::brotli;
use async_compression::tokio::write::{BrotliEncoder, ZstdEncoder};
use async_compression::Level::Precise;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use http_types::mime;
use log::LevelFilter;
use rustls_acme::caches::DirCache;
use rustls_acme::AcmeConfig;
use serde::{Deserialize, Serialize};
use simple_logger::SimpleLogger;
use siphasher::sip::SipHasher;
use std::collections::VecDeque;
use std::collections::{hash_map::Entry, HashMap};
use std::hash::{Hash, Hasher};
use std::io;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpSocket;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::Semaphore;
use tokio::time;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::wrappers::TcpListenerStream;
use warp::filters::BoxedFilter;
use warp::http::StatusCode;
use warp::ws::Message;
use warp::Filter;
use tokio::sync::Semaphore;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

// Asynchronous handler for the status page
async fn serve_status_page() -> Result<impl warp::Reply, warp::Rejection> {
    let html = generate_status_page().await;
    Ok(warp::reply::html(html))
}

// Asynchronous function to generate the status page
async fn generate_status_page() -> String {
    let files = vec![
        ("Server 1", "/home/ubuntu/www/mles-rs/static/mles.io/mina/server1.txt"),
        ("Server 2", "/home/ubuntu/www/mles-rs/static/mles.io/mina/server2.txt"),
        ("Server 3", "/home/ubuntu/www/mles-rs/static/mles.io/mina/server3.txt"),
        ("Server 4", "/home/ubuntu/www/mles-rs/static/mles.io/mina/server4.txt"),
    ];

    let mut html = String::from(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>qstake mina status</title>
            <meta http-equiv="refresh" content="5">
            <style>
                body {
                    font-family: monospace;
                }
                .status {
                    font-weight: bold;
                    margin: 10px 0;
                }
                .green { color: green; }
                .red { color: red; }
                .grey { color: grey; }
            </style>
        </head>
        <script>
        async function fetchMinaPrice() {
            const now = Date.now();
            const cache = getCache();
            
            // If cached data is valid (within 1 minute), use it
            if (cache && (now - cache.timestamp < 60 * 1000)) {
                updatePriceDisplay(cache.price);
                return;
            }

            // Otherwise, fetch the price from CoinGecko
            try {
                const response = await fetch("https://api.coingecko.com/api/v3/simple/price?ids=mina-protocol&vs_currencies=eur");
                if (!response.ok) {
                    throw new Error(`HTTP error! Status: ${response.status}`);
                }
                const data = await response.json();
                const price = data['mina-protocol']?.eur;

                if (price) {
                    saveCache(price, now);
                    updatePriceDisplay(price);
                } else {
                    updatePriceDisplay(cache.price);
                }
            } catch (error) {
                console.error("Error fetching Mina price:", error);
                updatePriceDisplay(cache.price);
            }
        }

        function getCache() {
            try {
                const cache = JSON.parse(localStorage.getItem('minaPriceCache'));
                return cache && cache.price && cache.timestamp ? cache : null;
            } catch {
                return null;
            }
        }

        function saveCache(price, timestamp) {
            const cache = { price, timestamp };
            localStorage.setItem('minaPriceCache', JSON.stringify(cache));
        }

        function updatePriceDisplay(price) {
            const priceElement = document.getElementById('mina-price');
            if (price) {
                priceElement.innerHTML = `Mina Price: <span class="green">€${price}</span>`;
            } else {
                priceElement.innerHTML = `Mina Price: <span class="red">Unavailable</span>`;
            }
        }

        // Fetch price when the page loads
        document.addEventListener('DOMContentLoaded', () => {
            fetchMinaPrice();
        });
        </script>
        <body>
            <h1>qstake mina status</h1>
        "#,
    );

    for (name, path) in files {
        let status = check_file_status(path).await;
        html.push_str(&format!(
            r#"
            <div class="status">
                {}: <span class="{}">{} ({})</span>
            </div>
            "#,
            name,
            if status.0 { "green" } else { "red" },
            status.1,
            status.2
        ));
    }

    html.push_str(&format!(
            r#"<div id="mina-price" class="status">Mina Price: <span class="grey">Loading...</span></div>"#
    ));

    html.push_str(
        r#"
        </body>
        </html>
        "#,
    );

    html
}

// Asynchronous function to check the status of a file
async fn check_file_status(path: &str) -> (bool, String, String) {
    match fs::metadata(path).await {
        Ok(metadata) => {
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    let elapsed_secs = now.as_secs() - duration.as_secs();

                    let content = match fs::read_to_string(path).await {
                        Ok(file_content) => file_content,
                        Err(_) => "Error reading file content".to_string(),
                    };

                    // If the file was modified within the last 60 seconds, it's green
                    if elapsed_secs <= 60 {
                        return (
                            true,
                            format!("OK, {} seconds ago", elapsed_secs),
                            content
                        );
                    } else {
                        return (
                            false,
                            format!("Failed, {} seconds ago", elapsed_secs),
                            content
                        );
                    }
                }
            }
            (false, "File exists but couldn't retrieve timestamp".to_string(), "".to_string())
        }
        Err(_) => (false, "File not found".to_string(), "".to_string()),
    }
}

const BR: &str = "br";
const ZSTD: &str = "zstd";

#[derive(Serialize, Deserialize, Hash)]
struct MlesHeader {
    uid: String,
    channel: String,
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

    /// Www-root directory for domain(s) (e.g. /path/static where domain example.io goes to
    /// static/example.io)
    #[arg(short, long, required = true)]
    wwwroot: PathBuf,

    /// Use Let's Encrypt staging environment
    /// (see https://letsencrypt.org/docs/staging-environment/)
    #[arg(short, long)]
    staging: bool,

    #[arg(short, long, default_value = TLS_PORT, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    /// Use http redirect for port 80
    #[arg(short, long)]
    redirect: bool,
}

const ACCEPTED_PROTOCOL: &str = "mles-websocket";
const TASK_BUF: usize = 16;
const WS_BUF: usize = 128;
const HISTORY_LIMIT: &str = "200";
const MAX_FILES_OPEN: &str = "256";
const TLS_PORT: &str = "443";
const PING_INTERVAL: u64 = 24000;
const BACKLOG: u32 = 1024;

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
        let mut msg_db: HashMap<u64, VecDeque<Message>> = HashMap::new();
        let mut ch_db: HashMap<u64, HashMap<u64, Sender<Option<Result<Message, warp::Error>>>>> =
            HashMap::new();
        while let Some(event) = rx.next().await {
            match event {
                WsEvent::Init(h, ch, tx2, err_tx, msg) => {
                    if !ch_db.contains_key(&ch) {
                        ch_db.entry(ch).or_default();
                    }
                    if let Some(uid_db) = ch_db.get_mut(&ch) {
                        if let Entry::Vacant(e) = uid_db.entry(h) {
                            msg_db.entry(ch).or_default();
                            e.insert(tx2.clone());
                            log::info!("Added {h:x} into {ch:x}.");

                            let val = err_tx.send(h);
                            if let Err(err) = val {
                                log::info!("Got err tx msg err {err}");
                            }

                            for (_, tx) in uid_db.iter().filter(|(&xh, _)| xh != h) {
                                let res = tx.send(Some(Ok(msg.clone()))).await;
                                if let Err(err) = res {
                                    log::info!("Got tx msg err {err}");
                                }
                            }

                            let queue = msg_db.get_mut(&ch);
                            if let Some(queue) = queue {
                                for qmsg in &*queue {
                                    let res = tx2.send(Some(Ok(qmsg.clone()))).await;
                                    if let Err(err) = res {
                                        log::info!("Got tx snd qmsg err {err}");
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

                let tx = tx_inner.clone();
                let tx2_inner = tx2.clone();
                let h_clone = h.clone();
                let ch_clone = ch.clone();
                let ping_cntr_clone = ping_cntr.clone();
                tokio::spawn(async move {
                    let h = h_clone;
                    let ch = ch_clone;
                    let tx2_spawn = tx2_inner.clone();
                    let ping_cntr_inner = ping_cntr_clone.clone();
                    if let Some(Ok(msg)) = ws_rx.next().await {
                        if !msg.is_text() {
                            return;
                        }
                        let msghdr: Result<MlesHeader, serde_json::Error> =
                            serde_json::from_str(msg.to_str().unwrap());
                        match msghdr {
                            Ok(msghdr) => {
                                let mut hasher = SipHasher::new();
                                msghdr.hash(&mut hasher);
                                h.store(hasher.finish(), Ordering::SeqCst);
                                let hasher = SipHasher::new();
                                ch.store(hasher.hash(msghdr.channel.as_bytes()), Ordering::SeqCst);
                                let _ = tx
                                    .send(WsEvent::Init(
                                        h.load(Ordering::SeqCst),
                                        ch.load(Ordering::SeqCst),
                                        tx2_spawn,
                                        err_tx,
                                        msg,
                                    ))
                                    .await;
                            }
                            Err(_) => return,
                        }
                    }
                    match err_rx.await {
                        Ok(_) => {}
                        Err(_) => {
                            log::info!("Duplicate entry, closing!");
                            h.store(0, Ordering::SeqCst);
                            ch.store(0, Ordering::SeqCst);
                            let _val = tx2_inner.send(Some(Ok(Message::close()))).await;
                            return;
                        }
                    }
                    while let Some(Ok(msg)) = ws_rx.next().await {
                        if 0 == h.load(Ordering::SeqCst) {
                            log::warn!("Invalid h, bailing");
                            break;
                        }
                        if msg.is_close() {
                            break;
                        }
                        ping_cntr_inner.store(0, Ordering::Relaxed);
                        if msg.is_pong() {
                            log::info!(
                                "Got pong for {:x} of {:x}",
                                h.load(Ordering::SeqCst),
                                ch.load(Ordering::SeqCst)
                            );
                            continue;
                        }
                        let val = tx
                            .send(WsEvent::Msg(
                                h.load(Ordering::SeqCst),
                                ch.load(Ordering::SeqCst),
                                msg,
                            ))
                            .await;
                        if let Err(err) = val {
                            log::warn!("Invalid tx {:?}", err);
                            break;
                        }
                    }
                    let _ = tx2_inner.send(None).await;
                });

                let tx2_inner = tx2.clone();
                let ping_cntr_inner = ping_cntr.clone();
                let h_clone = h.clone();
                let ch_clone = ch.clone();
                tokio::spawn(async move {
                    let h = h_clone;
                    let ch = ch_clone;
                    let mut interval = time::interval(Duration::from_millis(PING_INTERVAL));
                    interval.tick().await;

                    let tx2_clone = tx2_inner.clone();
                    loop {
                        interval.tick().await;
                        let val = tx2_clone.send(Some(Ok(Message::ping(Vec::new())))).await;
                        if val.is_err() {
                            break;
                        }
                        if 0 == h.load(Ordering::SeqCst) {
                            break;
                        }
                        let ping_cnt = ping_cntr_inner.fetch_add(1, Ordering::Relaxed);
                        if ping_cnt == 1 || ping_cnt == 2 {
                            log::debug!(
                                "Missed pong for {:x} of {:x}",
                                h.load(Ordering::SeqCst),
                                ch.load(Ordering::SeqCst)
                            );
                        }
                        if ping_cnt > 2 {
                            log::debug!(
                                "No pongs for {:x} of {:x}, close",
                                h.load(Ordering::SeqCst),
                                ch.load(Ordering::SeqCst)
                            );
                            break;
                        }
                    }
                    let _ = tx2_inner.send(None).await;
                });

                async move {
                    while let Some(Some(Ok(msg))) = rx2.next().await {
                        if msg.is_close() {
                            break;
                        }
                        let val = ws_tx.send(msg).await;
                        if let Err(err) = val {
                            log::info!("Invalid ws tx {:?}", err);
                            break;
                        }
                    }
                    let _val = ws_tx.send(Message::close()).await;

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
                warp::serve(hindex).run_incoming(tcp_incoming).await;
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
        .and_then(serve_status_page);

    let tlsroutes = page_route.or(ws).or(index);
    warp::serve(tlsroutes).run_incoming(tls_incoming).await;

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
        let params = brotli::EncoderParams::default().text_mode();
        let mut encoder = BrotliEncoder::with_quality_and_params(Vec::new(), Precise(2), params);
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

    // Open the file
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

            // Read the file content into a Vec<u8>
            let mut buffer = Vec::new();
            if (file.read_to_end(&mut buffer).await).is_err() {
                log::debug!("...FAILED!");
                return Ok(Box::new(warp::reply::with_status(
                    "Internal Server Error",
                    StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
            log::debug!("...OK, ctype {ctype}.");

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
            // Handle the case where the file doesn't exist or other errors
            log::debug!("{file_path} does not exist.");
            Ok(Box::new(warp::reply::with_status(
                "Not Found",
                StatusCode::NOT_FOUND,
            )))
        }
    }
}
