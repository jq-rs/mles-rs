/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2024  Mles developers
 */
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use rustls_acme::caches::DirCache;
use rustls_acme::AcmeConfig;
use serde::{Deserialize, Serialize};
use siphasher::sip::SipHasher;


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
use tokio::net::TcpSocket;
use tokio::sync::mpsc;

use tokio::sync::oneshot;
use tokio::time;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::wrappers::TcpListenerStream;
use warp::filters::BoxedFilter;
use warp::http::StatusCode;
use warp::ws::Message;
use warp::Filter;
use http_types::mime;
use tungstenite::handshake::client::Request;
use tokio_tungstenite::client_async_tls;
use tokio::net::TcpStream;
use base64::{Engine as _, engine::general_purpose};
use tokio_tungstenite::tungstenite::protocol::Message as WebMessage;

mod channels;
mod peers;

#[derive(Serialize, Deserialize, Hash)]
struct MlesHeader {
    uid: String,
    channel: String,
}

enum ConsolidatedError {
    WarpError(warp::Error),
    TungsteniteError(tungstenite::Error),
}

#[derive(Parser, Debug)]
struct Args {
    /// Domain(s)
    #[arg(short, long, required = true)]
    domains: Vec<String>,

    /// Peer(s)
    #[arg(long)]
    peers: Option<Vec<String>>,

    /// Contact info
    #[arg(short, long)]
    email: Vec<String>,

    /// Cache directory
    #[arg(short, long)]
    cache: Option<PathBuf>,

    /// History limit
    #[arg(short, long, default_value = HISTORY_LIMIT, value_parser = clap::value_parser!(u32).range(1..1_000_000))]
    limit: u32,

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
const TLS_PORT: &str = "443";
const PING_INTERVAL: u64 = 12000;
const BACKLOG: u32 = 1024;

fn create_tcp_incoming(addr: SocketAddr) -> io::Result<TcpListenerStream> {
    let socket = TcpSocket::new_v6()?;
    socket.set_keepalive(true)?;
    socket.set_nodelay(true)?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    let tcp_listener = socket.listen(BACKLOG)?;
    Ok(TcpListenerStream::new(tcp_listener))
}

fn get_random_buf() -> Result<[u8; 16], getrandom::Error> {
    let mut buf = [0u8; 16];
    getrandom::getrandom(&mut buf)?;
    Ok(buf)
}

fn generate_id() -> u64 {
    let rarray = get_random_buf();
    if let Ok(rarray) = rarray {
        let hasher = SipHasher::new_with_key(&rarray);
        return hasher.finish();
    }
    panic!("No random available!")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    simple_logger::init_with_env().unwrap();
    let args = Args::parse();
    let limit = args.limit;
    let www_root_dir = args.wwwroot;

    let (tx, rx) = mpsc::channel::<channels::WsEvent>(TASK_BUF);
    let rx = ReceiverStream::new(rx);

    let addr = format!("[{}]:{}", Ipv6Addr::UNSPECIFIED, args.port)
        .parse()
        .unwrap();
    let tcp_incoming = create_tcp_incoming(addr)?;

    let tls_incoming = AcmeConfig::new(args.domains.clone())
        .contact(args.email.iter().map(|e| format!("mailto:{}", e)))
        .cache_option(args.cache.clone().map(DirCache::new))
        .directory_lets_encrypt(!args.staging)
        .tokio_incoming(tcp_incoming, Vec::new());

    /* Generate node-id and key*/
    let nodeid = generate_id();
    let key = generate_id();
    log::debug!("Node id: 0x{nodeid:x}, key: 0x{key:x}");

    channels::init_channels(rx, limit);

    let tx_clone = tx.clone();
    let ws = warp::ws()
        .and(warp::header::exact(
            "Sec-WebSocket-Protocol",
            ACCEPTED_PROTOCOL,
        ))
        .map(move |ws: warp::ws::Ws| {
            let tx_inner = tx_clone.clone();
            let peers = args.peers.clone();
            ws.on_upgrade(move |websocket| {
                let (tx2, rx2) = mpsc::channel::<Option<Result<Message, ConsolidatedError>>>(WS_BUF);
                let (err_tx, err_rx) = oneshot::channel();
                let mut rx2 = ReceiverStream::new(rx2);
                let (mut ws_tx, mut ws_rx) = websocket.split();
                let h = Arc::new(AtomicU64::new(0));
                let ch = Arc::new(AtomicU64::new(0));
                let ping_cntr = Arc::new(AtomicU64::new(0));
                let pong_cntr = Arc::new(AtomicU64::new(0));

                let _tx_peer = tx_inner.clone();
                //Peering support
                if let Some(peers) = peers {
                    let port = args.port;
                    peers::init_peers(peers, port, nodeid, key);
                }

                let tx = tx_inner.clone();
                let tx2_inner = tx2.clone();
                let h_clone = h.clone();
                let ch_clone = ch.clone();
                let pong_cntr_clone = pong_cntr.clone();
                tokio::spawn(async move {
                    let h = h_clone;
                    let ch = ch_clone;
                    let tx2_spawn = tx2_inner.clone();
                    let pong_cntr_inner = pong_cntr_clone.clone();
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
                                    .send(channels::WsEvent::Init(
                                        h.load(Ordering::SeqCst),
                                        ch.load(Ordering::SeqCst),
                                        tx2_spawn,
                                        err_tx,
                                        msg,
                                        false
                                    ))
                                    .await;
                            }
                            Err(_) => return,
                        }
                    }
                    match err_rx.await {
                        Ok(_) => {}
                        Err(_) => {
                            log::info!("Got error from oneshot!");
                            return;
                        }
                    }
                    let tx2_sign = tx2_inner.clone();
                    while let Some(Ok(msg)) = ws_rx.next().await {
                        let tx2 = tx2_sign.clone();
                        if msg.is_pong() {
                            pong_cntr_inner.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        if msg.is_close() {
                            let _ = tx2.send(None).await;
                            break;
                        }
                        let val = tx
                            .send(channels::WsEvent::Msg(
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
                });

                let tx2_inner = tx2.clone();
                let ping_cntr_inner = ping_cntr.clone();
                let pong_cntr_inner = pong_cntr.clone();
                tokio::spawn(async move {
                    let mut interval = time::interval(Duration::from_millis(PING_INTERVAL));
                    interval.tick().await;

                    let tx2_clone = tx2_inner.clone();
                    loop {
                        let ping_cnt = ping_cntr_inner.fetch_add(1, Ordering::Relaxed);
                        let pong_cnt = pong_cntr_inner.load(Ordering::Relaxed);
                        let tx2 = tx2_clone.clone();
                        if pong_cnt + 2 < ping_cnt {
                            log::info!("No pongs, close");
                            let val = tx2.send(None).await;
                            if let Err(err) = val {
                                log::warn!("Invalid close tx {:?}", err);
                            }
                            break;
                        }
                        interval.tick().await;
                        let val = tx2.send(Some(Ok(Message::ping(Vec::new())))).await;
                        if let Err(err) = val {
                            log::info!("Invalid ping tx {:?}", err);
                            break;
                        }
                    }
                });

                let tx_clone = tx_inner.clone();
                async move {
                    while let Some(Some(Ok(msg))) = rx2.next().await {
                        let val = ws_tx.send(msg).await;
                        if let Err(err) = val {
                            log::info!("Invalid ws tx {:?}", err);
                            break;
                        }
                    }
                    let tx = tx_clone.clone();
                    let h = h.load(Ordering::SeqCst);
                    let ch = ch.load(Ordering::SeqCst);
                    if h != 0 && ch != 0 {
                        let _ = tx.send(channels::WsEvent::Logoff(h, ch)).await;
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
        let www_root = www_root_dir.clone();
        let index = warp::get()
            .and(warp::header::<String>("host"))
            .and(warp::path::tail())
            .map(move |uri: String, path: warp::path::Tail| {
                (
                    uri,
                    domain.clone(),
                    www_root.to_str().unwrap().to_string(),
                    path,
                )
            })
            .and_then(dyn_reply);
        vindex.push(index);
    }

    let mut index: BoxedFilter<_> = vindex.swap_remove(0).boxed();
    for val in vindex {
        index = val.or(index).unify().boxed();
    }

    let tlsroutes = ws.or(index);
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

async fn dyn_reply(
    tuple: (String, String, String, warp::path::Tail),
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let (uri, domain, www_root, tail) = tuple;

    if uri != domain {
        return Err(warp::reject::not_found());
    }
    let mut path = tail.as_str();
    if path.is_empty() {
        path = "index.html";
    }
    let file_path = format!("{}/{}/{}", www_root, uri, path);
    log::debug!("Tail {}", tail.as_str());
    log::debug!("Accessing {file_path}...");

    // Open the file
    match File::open(&file_path).await {
        Ok(mut file) => {
            let parts: Vec<&str> = file_path.split('.').collect();
            let ctype = match parts.last() {
                Some(v) => {
                    let mime = mime::Mime::from_extension(*v);
                    match mime {
                        Some(mime) => mime.basetype().to_string(),
                        None => mime::ANY.basetype().to_string()
                    }
                }
                None => mime::ANY.basetype().to_string()
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
            log::debug!("...OK.");

            // Create a custom response with the file content
            Ok(Box::new(warp::reply::with_header(warp::reply::Response::new(buffer.into()), "Content-Type", &ctype)))
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
