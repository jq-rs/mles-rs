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
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::Semaphore;
use tokio_stream::wrappers::TcpListenerStream;

const BACKLOG: u32 = 1024;
// Wall-clock ceiling on the accept-to-headers-parsed phase, and on the
// keep-alive idle window between requests. Slowloris and its cousins keep
// a connection open by dribbling headers a byte at a time; hyper's http1
// layer only enforces this if a Timer is installed. 60s matches typical
// browser keep-alive reuse windows so we don't force an extra TLS handshake
// on the second click; the per-IP concurrent cap handles the DoS side.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) type IpRateLimiter =
    RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

/// Build a per-IP rate limiter that allows `per_minute` accepted connections
/// per minute with a small burst.
pub(crate) fn create_rate_limiter(per_minute: NonZeroU32) -> Arc<IpRateLimiter> {
    Arc::new(RateLimiter::keyed(Quota::per_minute(per_minute)))
}

fn peer_ip(stream: &TcpStream) -> Option<IpAddr> {
    let addr = stream.peer_addr().ok()?;
    Some(rate_limit_key(addr.ip()))
}

// Normalise the peer address into the key we actually want to rate-limit on.
// Full-address keying is a footgun on IPv6: a typical residential or cloud
// allocation hands the customer an entire /64, giving one attacker 2^64
// distinct "peers" and effectively no rate limit at all. Collapse every
// v6 address to its /64 prefix so a whole allocation shares one quota.
fn rate_limit_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_canonical() {
            IpAddr::V4(v4) => IpAddr::V4(v4),
            IpAddr::V6(v6) => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
        v4 => v4,
    }
}

// Bucket for capping the number of concurrent connections a single source
// (v4 address or v6 /64 prefix) may hold. Governor caps the *arrival* rate,
// which doesn't help against an attacker who opens N connections up to the
// burst allowance and then keeps them idle-alive under the header timeout;
// this bounds the standing load per source.
//
// The counter map is drained on drop: entries at zero are removed, so an
// attacker that cycles IPs can't grow the map without bound. `retain_recent`
// on the rate limiter is a separate GC pass; this map self-manages.
pub(crate) struct PerIpLimiter {
    max_per_key: usize,
    counts: Mutex<HashMap<IpAddr, usize>>,
}

impl PerIpLimiter {
    pub(crate) fn new(max_per_key: NonZeroU32) -> Arc<Self> {
        Arc::new(Self {
            max_per_key: max_per_key.get() as usize,
            counts: Mutex::new(HashMap::new()),
        })
    }

    fn try_acquire(self: &Arc<Self>, key: IpAddr) -> Option<PerIpPermit> {
        let mut m = self.counts.lock().unwrap();
        let c = m.entry(key).or_insert(0);
        if *c >= self.max_per_key {
            return None;
        }
        *c += 1;
        Some(PerIpPermit {
            limiter: self.clone(),
            key,
        })
    }
}

pub(crate) struct PerIpPermit {
    limiter: Arc<PerIpLimiter>,
    key: IpAddr,
}

impl Drop for PerIpPermit {
    fn drop(&mut self) {
        let mut m = self.limiter.counts.lock().unwrap();
        if let Some(c) = m.get_mut(&self.key) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                m.remove(&self.key);
            }
        }
    }
}

// A TCP (or TLS) stream that carries a `PerIpPermit`. The permit is released
// when the stream is dropped — which happens when the connection ends, so
// slow-body / slow-response peers that hold onto a connection also hold onto
// their per-IP slot for the same duration.
pub(crate) struct PermittedStream<T> {
    inner: T,
    _permit: PerIpPermit,
}

impl<T: AsyncRead + Unpin> AsyncRead for PermittedStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for PermittedStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// Apply per-IP policy to an incoming TCP stream: an optional Governor
/// arrival-rate check (dropped before TLS if enforced) followed by a
/// mandatory concurrent-connection cap. Accepted streams are wrapped in a
/// [`PermittedStream`] whose owned permit is released when the connection
/// terminates, so slow-body and idle-keep-alive peers count against their
/// per-key quota for as long as they hold the socket.
pub(crate) fn enforce_ip_policy<S>(
    tcp_incoming: S,
    rate_limiter: Option<Arc<IpRateLimiter>>,
    per_ip: Arc<PerIpLimiter>,
) -> futures_util::stream::BoxStream<'static, Result<PermittedStream<TcpStream>, io::Error>>
where
    S: futures_util::Stream<Item = Result<TcpStream, io::Error>> + Send + 'static,
{
    tcp_incoming
        .filter_map(move |conn| {
            let rate_limiter = rate_limiter.clone();
            let per_ip = per_ip.clone();
            async move {
                let stream = match conn {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                // Fall back to an unspecified key when peer_addr() fails — a
                // single shared bucket for those cases, not a bypass. Rare
                // in practice (usually means the socket already closed).
                let key = peer_ip(&stream).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

                if let Some(limiter) = rate_limiter.as_ref()
                    && limiter.check_key(&key).is_err()
                {
                    log::debug!("rate-limited connection from {}", key);
                    return None;
                }

                let permit = match per_ip.try_acquire(key) {
                    Some(p) => p,
                    None => {
                        log::debug!("per-ip cap reached for {}", key);
                        return None;
                    }
                };

                Some(Ok(PermittedStream {
                    inner: stream,
                    _permit: permit,
                }))
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

pub(crate) fn create_tls_incoming<S, T>(
    domains: Vec<String>,
    email: Vec<String>,
    cache: Option<PathBuf>,
    staging: bool,
    tcp_incoming: S,
) -> impl futures_util::Stream<
    Item = Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin, std::io::Error>,
>
where
    S: futures_util::Stream<Item = Result<T, io::Error>> + Send + Unpin + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    AcmeConfig::new(domains)
        .contact(email.iter().map(|e| format!("mailto:{}", e)))
        .cache_option(cache.map(DirCache::new))
        .directory_lets_encrypt(!staging)
        .tokio_incoming(tcp_incoming, vec![b"http/1.1".to_vec()])
}

pub(crate) async fn serve_http<S, T, F>(tcp_incoming: S, routes: F, semaphore: Arc<Semaphore>)
where
    S: futures_util::Stream<Item = Result<T, io::Error>>,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: warp::Filter + Clone + Send + Sync + 'static,
    F::Extract: warp::Reply,
{
    let service = warp::service(routes);

    tokio::pin!(tcp_incoming);
    while let Some(Ok(stream)) = tcp_incoming.next().await {
        // Acquire the connection permit before spawning so the accept loop
        // back-pressures against `filelimit`. The previous version acquired
        // inside a filter_map and dropped the permit the instant the future
        // resolved — so it never actually capped anything.
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break, // semaphore closed → shutdown
        };
        let io = TokioIo::new(stream);
        let service_clone = TowerToHyperService::new(service.clone());

        tokio::spawn(async move {
            let _permit = permit; // held for the whole connection lifetime
            if let Err(err) = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT)
                .serve_connection(io, service_clone)
                .await
            {
                // is_timeout() covers HeaderTimeout, which fires on every
                // idle keep-alive close and would swamp the log otherwise.
                if !err.is_timeout() {
                    log::debug!("Error serving connection: {:?}", err);
                }
            }
        });
    }
}

pub(crate) async fn serve_tls<S, T, E, F>(tls_incoming: S, routes: F, semaphore: Arc<Semaphore>)
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
                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break, // semaphore closed → shutdown
                };
                let io = TokioIo::new(stream);
                let service_clone = TowerToHyperService::new(service.clone());

                tokio::spawn(async move {
                    let _permit = permit; // held for the whole connection lifetime
                    if let Err(err) = http1::Builder::new()
                        .timer(TokioTimer::new())
                        .header_read_timeout(HEADER_READ_TIMEOUT)
                        .serve_connection(io, service_clone)
                        .with_upgrades()
                        .await
                    {
                        if !err.is_timeout() {
                            log::debug!("Error serving TLS connection: {:?}", err);
                        }
                    }
                });
            }
            Err(err) => {
                log::debug!("TLS connection error: {:?}", err);
            }
        }
    }
}
