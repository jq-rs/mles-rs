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
pub(crate) mod metrics;
pub(crate) mod server;
pub(crate) mod types;
pub(crate) mod websocket;

use std::io;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use warp::Filter;

const TASK_BUF: usize = 16;
const DEFAULT_CACHE_SIZE_MB: usize = 10;

// Re-export metrics and WebSocket types for public API
pub use metrics::{Metrics, MetricsSnapshot};
pub use websocket::WebSocketConfig;

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

    /// WebSocket runtime configuration
    pub websocket_config: WebSocketConfig,
    /// Enable periodic metrics logging task
    pub enable_metrics_logging: bool,
    /// Optional per-IP allowed connections per window. If `None`, per-IP limiting is disabled.
    pub per_ip_limit: Option<u32>,
    /// Per-IP window size in seconds. Only used when `per_ip_limit` is Some(_).
    pub per_ip_window_secs: Option<u64>,
}

impl ServerConfig {
    /// Create a new ServerConfig with reasonable defaults.
    pub fn new(domains: Vec<String>, email: Vec<String>, wwwroot: PathBuf, port: u16) -> Self {
        Self {
            domains,
            email,
            cache: None,
            limit: 100,
            filelimit: 1024,
            wwwroot,
            staging: false,
            port,
            redirect: false,
            max_cache_size_mb: None,

            websocket_config: WebSocketConfig::default(),
            enable_metrics_logging: true,
            per_ip_limit: Some(60),
            per_ip_window_secs: Some(60),
        }
    }

    /// Set the history limit
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }

    /// Set the file limit
    pub fn with_filelimit(mut self, filelimit: usize) -> Self {
        self.filelimit = filelimit;
        self
    }

    /// Enable HTTP redirect for port 80
    pub fn with_redirect(mut self, redirect: bool) -> Self {
        self.redirect = redirect;
        self
    }

    /// Set Let's Encrypt staging mode
    pub fn with_staging(mut self, staging: bool) -> Self {
        self.staging = staging;
        self
    }

    /// Set ACME cache directory
    pub fn with_cache(mut self, cache: PathBuf) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Set cache size for compressed files
    pub fn with_max_cache_size_mb(mut self, size: usize) -> Self {
        self.max_cache_size_mb = Some(size);
        self
    }

    /// Set WebSocket configuration
    pub fn with_websocket_config(mut self, config: WebSocketConfig) -> Self {
        self.websocket_config = config;
        self
    }

    /// Enable or disable periodic metrics logging
    pub fn with_metrics_logging(mut self, enable: bool) -> Self {
        self.enable_metrics_logging = enable;
        self
    }

    /// Set per-IP allowed connections in the configured window.
    /// Use `None` to disable per-IP limiting.
    pub fn with_per_ip_limit(mut self, limit: Option<u32>) -> Self {
        self.per_ip_limit = limit;
        self
    }

    /// Set per-IP window size in seconds. Only used when `per_ip_limit` is Some(_).
    pub fn with_per_ip_window_secs(mut self, secs: Option<u64>) -> Self {
        self.per_ip_window_secs = secs;
        self
    }
}

/// Server handle for graceful shutdown and metrics access
pub struct ServerHandle {
    shutdown_token: CancellationToken,
    metrics: Metrics,
}

impl ServerHandle {
    /// Trigger graceful shutdown of the server
    pub fn shutdown(&self) {
        log::info!("Initiating graceful shutdown");
        self.shutdown_token.cancel();
    }

    /// Get current server metrics
    pub fn metrics(&self) -> MetricsSnapshot {
        self.metrics.get_stats()
    }

    /// Check if shutdown has been requested
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_token.is_cancelled()
    }

    /// Wait for shutdown to be requested
    pub async fn wait_for_shutdown(&self) {
        self.shutdown_token.cancelled().await;
    }

    /// Get a clone of the shutdown token for coordinating with other tasks
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown_token.clone()
    }
}

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

    // Create basic metrics but don't expose them
    let metrics = Metrics::new();
    let shutdown_token = CancellationToken::new();

    // Spawn WebSocket event loop (no shutdown)
    websocket::spawn_event_loop(rx, limit, metrics.clone(), shutdown_token);

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
        metrics.clone(),
        None,
        None,
    );

    // Spawn HTTP redirect server if requested
    if config.redirect {
        http::spawn_http_redirect_server(config.domains.clone());
    }

    // Create WebSocket handler with default config
    let ws = websocket::create_ws_handler(tx.clone(), WebSocketConfig::default(), metrics);

    let compression_cache = cache::create_cache(max_cache_size_mb);
    // Create HTTP file serving routes with configured cache size
    let index = http::create_http_file_routes(
        config.domains,
        www_root_dir,
        semaphore.clone(),
        compression_cache,
    );

    // Combine all routes
    let tlsroutes = ws.or(index);

    // Serve TLS connections (blocks forever)
    server::serve_tls(tls_incoming, tlsroutes).await;

    unreachable!()
}

/// Run the Mles server with graceful shutdown support
///
/// Returns a `ServerHandle` that can be used to:
/// - Trigger graceful shutdown
/// - Query server metrics
/// - Monitor server state
pub async fn run_with_shutdown(config: ServerConfig) -> io::Result<ServerHandle> {
    run_with_shutdown_token(config, CancellationToken::new()).await
}

/// Run the Mles server with external shutdown coordination
///
/// This allows coordinating shutdown with other parts of your application
/// by providing an external `CancellationToken`.
pub async fn run_with_shutdown_token(
    config: ServerConfig,
    shutdown_token: CancellationToken,
) -> io::Result<ServerHandle> {
    // Use the merged ServerConfig fields directly (no `base` indirection).
    let limit = config.limit;
    let www_root_dir = config.wwwroot.clone();
    let filelimit = config.filelimit;
    let max_cache_size_mb = match config.max_cache_size_mb {
        Some(size) => size,
        None => DEFAULT_CACHE_SIZE_MB,
    };
    let semaphore = Arc::new(Semaphore::new(filelimit));

    // Create WebSocket event channel
    let (tx, rx) = mpsc::channel::<types::WsEvent>(TASK_BUF);
    let rx = ReceiverStream::new(rx);

    // Create metrics and WebSocket config
    let metrics = Metrics::new();
    let ws_config = config.websocket_config.clone();

    log::info!(
        "Starting Mles server on port {} with {} file limit, WebSocket ping interval: {}ms",
        config.port,
        filelimit,
        ws_config.ping_interval_ms
    );

    // Spawn WebSocket event loop with shutdown support
    websocket::spawn_event_loop(rx, limit, metrics.clone(), shutdown_token.clone());

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
        metrics.clone(),
        config.per_ip_limit,
        config.per_ip_window_secs,
    );

    // Spawn HTTP redirect server if requested
    if config.redirect {
        log::info!("Starting HTTP redirect server on port 80");
        http::spawn_http_redirect_server(config.domains.clone());
    }

    // Create WebSocket handler with config and metrics
    let ws = websocket::create_ws_handler(tx.clone(), ws_config, metrics.clone());

    let compression_cache = cache::create_cache(max_cache_size_mb);
    log::info!(
        "Initialized compression cache with {}MB limit",
        max_cache_size_mb
    );

    // Create HTTP file serving routes with configured cache size
    let index = http::create_http_file_routes(
        config.domains.clone(),
        www_root_dir.clone(),
        semaphore.clone(),
        compression_cache,
    );

    log::info!("Serving static files from: {}", www_root_dir.display());

    // Combine all routes
    let tlsroutes = ws.or(index);

    // Create server handle
    let handle = ServerHandle {
        shutdown_token: shutdown_token.clone(),
        metrics: metrics.clone(),
    };

    log::info!(
        "Mles server started successfully on {}:{}",
        config.domains.join(", "),
        config.port
    );

    // Spawn the server task
    let shutdown_server = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = shutdown_server.cancelled() => {
                log::info!("Server task received shutdown signal");
                // Give some time for connections to close gracefully
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            _ = server::serve_tls(tls_incoming, tlsroutes) => {
                log::error!("TLS server unexpectedly terminated");
            }
        }
    });

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn test_server_config_builder() {
        let config = ServerConfig::new(
            vec!["example.com".to_string()],
            vec!["admin@example.com".to_string()],
            PathBuf::from("/var/www"),
            443,
        )
        .with_limit(200)
        .with_filelimit(2048)
        .with_redirect(true)
        .with_staging(true);

        assert_eq!(config.limit, 200);
        assert_eq!(config.filelimit, 2048);
        assert_eq!(config.redirect, true);
        assert_eq!(config.staging, true);
    }

    #[test]
    fn test_websocket_config_default() {
        let ws_config = WebSocketConfig::default();
        assert_eq!(ws_config.ping_interval_ms, 24000);
        assert_eq!(ws_config.max_missed_pongs, 3);
        assert_eq!(ws_config.rate_limit_messages_per_sec, 100);
    }

    #[test]
    fn test_per_ip_builder_fields() {
        let config = ServerConfig::new(
            vec!["example.com".to_string()],
            vec!["admin@example.com".to_string()],
            PathBuf::from("/var/www"),
            443,
        )
        .with_per_ip_limit(Some(120))
        .with_per_ip_window_secs(Some(60));

        assert_eq!(config.per_ip_limit, Some(120));
        assert_eq!(config.per_ip_window_secs, Some(60));
    }

    /// Unit test that simulates a simple per-IP limiter and ensures a dropped-connections
    /// metric (AtomicU64) increments when a source exceeds the allowed count.
    #[test]
    fn test_per_ip_drop_metrics_increment() {
        // Simple in-memory per-ip counter with window reset semantics.
        let per_ip_limit = 3u32;
        let window_secs = 2u64;

        // dropped metric that would be shared with server internals in production
        let dropped_metric = AtomicU64::new(0);

        let mut counters: HashMap<String, (Instant, u32)> = HashMap::new();

        // helper that simulates an incoming connection from `ip`
        let mut simulate_conn = |ip: &str| {
            let now = Instant::now();
            let entry = counters.entry(ip.to_string()).or_insert((now, 0));
            if now.duration_since(entry.0).as_secs() >= window_secs {
                *entry = (now, 1);
                true // accepted
            } else {
                if entry.1 < per_ip_limit {
                    entry.1 = entry.1.saturating_add(1);
                    true // accepted
                } else {
                    // would be dropped; increment metric
                    dropped_metric.fetch_add(1, Ordering::SeqCst);
                    false // dropped
                }
            }
        };

        // Simulate 5 connections from the same IP in quick succession
        let ip = "203.0.113.5";
        assert_eq!(simulate_conn(ip), true); // 1
        assert_eq!(simulate_conn(ip), true); // 2
        assert_eq!(simulate_conn(ip), true); // 3
        // Now should exceed
        assert_eq!(simulate_conn(ip), false); // dropped
        assert_eq!(simulate_conn(ip), false); // dropped

        assert_eq!(dropped_metric.load(Ordering::SeqCst), 2);

        // Wait for the window to expire and simulate again -> should be accepted and counter reset
        std::thread::sleep(Duration::from_secs(window_secs + 1));
        assert_eq!(simulate_conn(ip), true);
    }
}
