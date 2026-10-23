/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */

pub(crate) mod auth;
pub(crate) mod cache;
pub(crate) mod mina;
pub(crate) mod compression;
pub(crate) mod http;
pub(crate) mod proxy;
pub(crate) mod server;
pub(crate) mod types;
pub(crate) mod websocket;

use std::io;
use std::net::Ipv6Addr;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use warp::Filter;
use warp::filters::BoxedFilter;

const TASK_BUF: usize = 16;
const DEFAULT_CACHE_SIZE_MB: usize = 24;
// Governor's keyed store never evicts on its own — call `retain_recent()`
// periodically so an attacker who cycles through many source IPs (or IPv6
// /64 prefixes) can't grow the state map without bound.
const RATE_LIMIT_GC_INTERVAL: Duration = Duration::from_secs(60);

/// Configuration for the Mles server
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Domain(s) to serve
    pub domains: Vec<String>,
    /// Contact email(s) for Let's Encrypt
    pub email: Vec<String>,
    /// Cache directory for ACME certificates
    pub cache: Option<PathBuf>,
    /// History limit for message queue
    pub limit: u32,
    /// Open files limit
    pub filelimit: usize,
    /// Web root directory
    pub wwwroot: PathBuf,
    /// Use Let's Encrypt staging environment
    pub staging: bool,
    /// Server port
    pub port: u16,
    /// Use http redirect for port 80
    pub redirect: bool,
    /// Cache size for compressed files in MB
    pub max_cache_size_mb: Option<usize>,
    /// Reverse-proxy rules: list of (url_prefix, upstream_base_url) pairs.
    /// Requests to `/{prefix}/...` are forwarded to `{upstream}/...`.
    pub proxies: Vec<(String, String)>,
    /// Per-IP TCP-accept rate limit (connections per minute). `None` disables.
    /// Applies to both the TLS listener and the HTTP redirect listener.
    pub rate_limit: Option<NonZeroU32>,
    /// Maximum concurrent live connections per source (v4 address or v6 /64
    /// prefix). Bounds an attacker who exhausts the arrival-rate burst then
    /// keeps connections idle-alive under the header-read timeout.
    pub per_ip_conn_limit: NonZeroU32,
    /// Add cross-origin headers to files served from the web root: COOP,
    /// COEP and CORP on every file (cross-origin isolation, needed by
    /// SharedArrayBuffer and WebAssembly threads), and
    /// `Access-Control-Allow-Origin: *` on JSON files.
    pub cors: bool,
}

/// Run the Mles server with the given configuration
pub async fn run(config: ServerConfig) -> io::Result<()> {
    let limit = config.limit;
    let www_root_dir = config.wwwroot;
    let filelimit = config.filelimit;
    let max_cache_size_mb = match config.max_cache_size_mb {
        Some(size) => size,
        None => DEFAULT_CACHE_SIZE_MB,
    };
    // Two independent semaphores so a stall on the redirect port can't
    // exhaust the connection budget that TLS clients rely on.
    let tls_semaphore = Arc::new(Semaphore::new(filelimit));
    let redirect_semaphore = Arc::new(Semaphore::new(filelimit));

    // Create WebSocket event channel
    let (tx, rx) = mpsc::channel::<types::WsEvent>(TASK_BUF);
    let rx = ReceiverStream::new(rx);

    // Spawn WebSocket event loop
    websocket::spawn_event_loop(rx, limit);

    // Create TCP listener
    let addr = format!("[{}]:{}", Ipv6Addr::UNSPECIFIED, config.port)
        .parse()
        .unwrap();
    let tcp_incoming = server::create_tcp_incoming(addr)?;

    // Optionally wrap with a per-IP TCP-accept rate limiter. Shared between the
    // TLS listener and the HTTP redirect listener so an abusive IP can't bypass
    // by hammering port 80.
    let rate_limiter = config.rate_limit.map(server::create_rate_limiter);

    // Governor doesn't age out cold keys — do it ourselves.
    if let Some(limiter) = rate_limiter.clone() {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(RATE_LIMIT_GC_INTERVAL);
            loop {
                interval.tick().await;
                limiter.retain_recent();
            }
        });
    }

    // Per-IP concurrent-connection cap — separate instances for TLS and the
    // HTTP redirect so a flood on one port doesn't consume the other's quota.
    let tls_per_ip = server::PerIpLimiter::new(config.per_ip_conn_limit);
    let redirect_per_ip = server::PerIpLimiter::new(config.per_ip_conn_limit);

    let tcp_incoming =
        server::enforce_ip_policy(tcp_incoming, rate_limiter.clone(), tls_per_ip);

    // Create TLS incoming stream
    let tls_incoming = server::create_tls_incoming(
        config.domains.clone(),
        config.email,
        config.cache,
        config.staging,
        tcp_incoming,
    );

    // Spawn HTTP redirect server if requested
    if config.redirect {
        http::spawn_http_redirect_server(
            config.domains.clone(),
            rate_limiter.clone(),
            redirect_semaphore,
            redirect_per_ip,
        );
    }

    // Create WebSocket handler
    let ws = websocket::create_ws_handler(tx.clone());

    let compression_cache = cache::create_cache(max_cache_size_mb);
    // Create HTTP file serving routes with configured cache size. Per-request
    // gating happens at the connection layer (serve_tls holds one permit per
    // live connection) — HTTP/1.1 serves requests serially on a connection,
    // so the connection cap already bounds concurrent file I/O.
    let index = http::create_http_file_routes(
        config.domains,
        www_root_dir,
        compression_cache,
        config.cors,
    );

    // Define the mina status page route
    let page_route = warp::path("mina_status")
        .and(warp::get())
        .and_then(mina::serve_status_page);

    // Combine all routes
    let http_routes: BoxedFilter<(Box<dyn warp::Reply>,)> = if config.proxies.is_empty() {
        index.boxed()
    } else {
        let client = proxy::create_client();
        let proxy_routes = proxy::create_proxy_routes(&config.proxies, client);
        proxy_routes.or(index).unify().boxed()
    };
    let tlsroutes = page_route.or(ws).or(http_routes);

    // Serve TLS connections
    server::serve_tls(tls_incoming, tlsroutes, tls_semaphore).await;

    unreachable!()
}
