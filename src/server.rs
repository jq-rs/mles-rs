/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use crate::metrics::Metrics;
use futures_util::{StreamExt, future::ready};
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;
use tokio::net::TcpSocket;
use tokio::sync::Semaphore;
use tokio_stream::wrappers::TcpListenerStream;

const BACKLOG: u32 = 1024;

pub(crate) fn create_tcp_incoming(addr: SocketAddr) -> io::Result<TcpListenerStream> {
    let socket = TcpSocket::new_v6()?;
    socket.set_keepalive(true)?;
    socket.set_nodelay(true)?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    let tcp_listener = socket.listen(BACKLOG)?;
    Ok(TcpListenerStream::new(tcp_listener))
}

pub(crate) fn create_tls_incoming(
    domains: Vec<String>,
    email: Vec<String>,
    cache: Option<PathBuf>,
    staging: bool,
    tcp_incoming: TcpListenerStream,
    semaphore: Arc<Semaphore>,
    metrics: Metrics,
    per_ip_limit: Option<u32>,
    per_ip_window_secs: Option<u64>,
) -> impl futures_util::Stream<
    Item = Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin, std::io::Error>,
> {
    // If per-ip limiting is configured, we'll use those values; otherwise per-ip limiting is disabled.
    let configured_limit = per_ip_limit;
    let configured_window = per_ip_window_secs.unwrap_or(60);

    // Shared map to track per-IP counts and window start time
    // The tuple is (window_start_instant, count)
    // NOTE: use a synchronous std::sync::Mutex here so we can perform the quick
    // check in a non-async closure (we avoid awaiting in the stream filter so the
    // resulting stream remains Unpin for the downstream TLS acceptor).
    let ip_counters: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // First, optionally filter TCP accepts by per-IP rate limiting.
    // Single filtered TCP stream. The closure checks configured_limit at runtime,
    // avoiding distinct closure types from an `if`/`else` expression.
    let filtered_tcp = {
        let ip_counters = ip_counters.clone();
        let metrics = metrics.clone();
        let limit_opt = configured_limit;
        let window_secs = configured_window;
        tcp_incoming.filter_map(move |item| {
            let ip_counters = ip_counters.clone();
            let metrics = metrics.clone();
            // Handle the listener `Result<TcpStream, io::Error>` synchronously and
            // return `ready(...)` to keep the FilterMap non-async (Unpin).
            match item {
                Ok(tcp_stream) => {
                    // If per-IP limiting is disabled, fast-path accept
                    if limit_opt.is_none() {
                        return ready(Some(Ok(tcp_stream)));
                    }

                    let limit = limit_opt.unwrap();

                    // Try to get peer address; if unavailable, accept the stream
                    match tcp_stream.peer_addr() {
                        Ok(addr) => {
                            let ip = addr.ip();
                            let now = Instant::now();

                            // Lock the std mutex briefly (fast, bounded operation)
                            let mut map = ip_counters.lock().unwrap();
                            let entry = map.entry(ip).or_insert((now, 0));

                            // If window expired, reset
                            if now.duration_since(entry.0).as_secs() > window_secs {
                                *entry = (now, 1);
                                ready(Some(Ok(tcp_stream)))
                            } else {
                                // Within window
                                if entry.1 < limit {
                                    entry.1 = entry.1.saturating_add(1);
                                    ready(Some(Ok(tcp_stream)))
                                } else {
                                    // Rate limit exceeded for this source IP
                                    let ip_str = ip.to_string();
                                    log::warn!(
                                        "Per-IP rate limit exceeded for {}, dropping connection",
                                        ip_str
                                    );
                                    // update metrics about dropped connection
                                    metrics.increment_dropped_by_ip(&ip_str);
                                    ready(None)
                                }
                            }
                        }
                        Err(e) => {
                            // If we cannot determine peer address, allow connection but log
                            log::debug!("Could not get peer_addr for incoming connection: {}", e);
                            ready(Some(Ok(tcp_stream)))
                        }
                    }
                }
                Err(e) => {
                    // Propagate listener errors downstream so upstream logic can handle them
                    log::debug!("Listener error while accepting connection: {}", e);
                    ready(Some(Err(e)))
                }
            }
        })
    };

    // Pass the (possibly filtered) TCP stream into ACME/TLS acceptor
    let tls_incoming = AcmeConfig::new(domains)
        .contact(email.iter().map(|e| format!("mailto:{}", e)))
        .cache_option(cache.map(DirCache::new))
        .directory_lets_encrypt(!staging)
        .tokio_incoming(filtered_tcp, vec![b"http/1.1".to_vec()]);

    // Wrap the incoming connections stream with a filter to enforce a global connection limit (semaphore)
    tls_incoming.filter_map(move |conn| {
        let sem = semaphore.clone();
        async move {
            let _permit = sem.acquire().await;
            Some(conn)
        }
    })
}

pub(crate) async fn serve_http<F>(tcp_incoming: TcpListenerStream, routes: F)
where
    F: warp::Filter + Clone + Send + Sync + 'static,
    F::Extract: warp::Reply,
{
    let service = warp::service(routes);

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

pub(crate) async fn serve_tls<S, T, E, F>(tls_incoming: S, routes: F)
where
    S: futures_util::Stream<Item = Result<T, E>>,
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    E: std::fmt::Debug,
    F: warp::Filter + Clone + Send + Sync + 'static,
    F::Extract: warp::Reply,
{
    let service = warp::service(routes);

    tokio::pin!(tls_incoming); // Pin here instead of requiring Unpin
    while let Some(result) = tls_incoming.next().await {
        match result {
            Ok(stream) => {
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
            Err(err) => {
                log::debug!("TLS connection error: {:?}", err);
            }
        }
    }
}
