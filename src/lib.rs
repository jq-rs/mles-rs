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
pub(crate) mod mina;
pub(crate) mod server;
pub(crate) mod types;
pub(crate) mod websocket;

use std::io;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use warp::Filter;

const TASK_BUF: usize = 16;
const FILE_LIMIT: u32 = 256; // Maximum number of open files

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
    /// Web root directory
    pub wwwroot: PathBuf,
    /// Use Let's Encrypt staging environment
    pub staging: bool,
    /// Server port
    pub port: u16,
    /// Use http redirect for port 80
    pub redirect: bool,
}

/// Run the Mles server with the given configuration
pub async fn run(config: ServerConfig) -> io::Result<()> {
    let limit = config.limit;
    let www_root_dir = config.wwwroot;
    let semaphore = Arc::new(Semaphore::new(FILE_LIMIT as usize));

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
        http::spawn_http_redirect_server(config.domains.clone());
    }

    // Create WebSocket handler
    let ws = websocket::create_ws_handler(tx.clone());

    // Create HTTP file serving routes with configured cache size
    let index = http::create_http_file_routes(config.domains, www_root_dir, semaphore.clone());

    // Define the mina status page route
    let page_route = warp::path("mina_status")
        .and(warp::get())
        .and_then(mina::serve_status_page);

    // Combine all routes
    let tlsroutes = page_route.or(ws).or(index);

    // Serve TLS connections
    server::serve_tls(tls_incoming, tlsroutes).await;

    unreachable!()
}
