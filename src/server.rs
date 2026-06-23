/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use futures_util::StreamExt;
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::Semaphore;
use tokio_stream::wrappers::TcpListenerStream;

const BACKLOG: u32 = 1024;

pub(crate) type IpRateLimiter =
    RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

/// Build a per-IP rate limiter that allows `per_minute` accepted connections
/// per minute with a small burst.
pub(crate) fn create_rate_limiter(per_minute: NonZeroU32) -> Arc<IpRateLimiter> {
    Arc::new(RateLimiter::keyed(Quota::per_minute(per_minute)))
}

fn peer_ip(stream: &TcpStream) -> Option<IpAddr> {
    let addr = stream.peer_addr().ok()?;
    Some(match addr.ip() {
        IpAddr::V6(v6) => v6.to_canonical(),
        v4 => v4,
    })
}

/// Wrap an incoming TCP stream with a per-IP rate limiter. Connections from
/// IPs over the quota are dropped immediately (before TLS handshake), which
/// the client observes as a closed connection.
pub(crate) fn rate_limit_incoming(
    tcp_incoming: TcpListenerStream,
    limiter: Arc<IpRateLimiter>,
) -> futures_util::stream::BoxStream<'static, Result<TcpStream, io::Error>> {
    tcp_incoming
        .filter_map(move |conn| {
            let limiter = limiter.clone();
            async move {
                let stream = match conn {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let Some(ip) = peer_ip(&stream) else {
                    return Some(Ok(stream));
                };
                if limiter.check_key(&ip).is_ok() {
                    Some(Ok(stream))
                } else {
                    log::debug!("rate-limited connection from {}", ip);
                    None
                }
            }
        })
        .boxed()
}

pub(crate) fn create_tcp_incoming(addr: SocketAddr) -> io::Result<TcpListenerStream> {
    let socket = TcpSocket::new_v6()?;
    socket.set_keepalive(true)?;
    socket.set_nodelay(true)?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    let tcp_listener = socket.listen(BACKLOG)?;
    Ok(TcpListenerStream::new(tcp_listener))
}

pub(crate) fn create_tls_incoming<S>(
    domains: Vec<String>,
    email: Vec<String>,
    cache: Option<PathBuf>,
    staging: bool,
    tcp_incoming: S,
    semaphore: Arc<Semaphore>,
) -> impl futures_util::Stream<
    Item = Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin, std::io::Error>,
>
where
    S: futures_util::Stream<Item = Result<TcpStream, io::Error>> + Send + Unpin + 'static,
{
    let tls_incoming = AcmeConfig::new(domains)
        .contact(email.iter().map(|e| format!("mailto:{}", e)))
        .cache_option(cache.map(DirCache::new))
        .directory_lets_encrypt(!staging)
        .tokio_incoming(tcp_incoming, vec![b"http/1.1".to_vec()]);

    // Wrap the incoming connections stream with a filter to enforce a connection limit
    tls_incoming.filter_map(move |conn| {
        let sem = semaphore.clone();
        async move {
            let _permit = sem.acquire().await;
            Some(conn)
        }
    })
}

pub(crate) async fn serve_http<S, F>(tcp_incoming: S, routes: F)
where
    S: futures_util::Stream<Item = Result<TcpStream, io::Error>>,
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
