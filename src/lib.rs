/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */

pub(crate) mod auth;
pub(crate) mod cache;
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
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use warp::Filter;
use warp::filters::BoxedFilter;

const TASK_BUF: usize = 16;
const DEFAULT_CACHE_SIZE_MB: usize = 24;

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
    let semaphore = Arc::new(Semaphore::new(filelimit));

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

    let tcp_incoming: futures_util::stream::BoxStream<'static, _> = match rate_limiter.clone() {
        Some(limiter) => server::rate_limit_incoming(tcp_incoming, limiter),
        None => futures_util::StreamExt::boxed(tcp_incoming),
    };

    // Create TLS incoming stream
    let tls_incoming = server::create_tls_incoming(
        config.domains.clone(),
        config.email,
        config.cache,
        config.staging,
        tcp_incoming,
        semaphore.clone(),
    );

    // Spawn HTTP redirect server if requested
    if config.redirect {
        http::spawn_http_redirect_server(config.domains.clone(), rate_limiter.clone());
    }

    // Create WebSocket handler
    let ws = websocket::create_ws_handler(tx.clone());

    let compression_cache = cache::create_cache(max_cache_size_mb);
    // Create HTTP file serving routes with configured cache size
    let index = http::create_http_file_routes(
        config.domains,
        www_root_dir,
        semaphore.clone(),
        compression_cache,
    );

    // Combine all routes
    let http_routes: BoxedFilter<(Box<dyn warp::Reply>,)> = if config.proxies.is_empty() {
        index.boxed()
    } else {
        let client = proxy::create_client();
        let proxy_routes = proxy::create_proxy_routes(&config.proxies, client);
        proxy_routes.or(index).unify().boxed()
    };
    let tlsroutes = ws.or(http_routes);

    // Serve TLS connections
    server::serve_tls(tls_incoming, tlsroutes).await;

    unreachable!()
}
