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
pub(crate) mod server;
pub(crate) mod types;
pub(crate) mod websocket;

use std::io;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use warp::Filter;

const TASK_BUF: usize = 16;
const DEFAULT_CACHE_SIZE_MB: usize = 10;
const METRICS_LOG_INTERVAL_SECS: u64 = 60;

// Re-export WebSocket types for public API
pub use websocket::{WebSocketConfig, Metrics, MetricsSnapshot};

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
}

/// V2.9+ Configuration with enhanced features
#[derive(Debug, Clone)]
pub struct ServerConfigV2 {
    /// Base configuration
    pub base: ServerConfig,
    /// WebSocket configuration
    pub websocket_config: WebSocketConfig,
    /// Enable periodic metrics logging
    pub enable_metrics_logging: bool,
}

impl ServerConfigV2 {
    /// Create a new ServerConfigV2 with required fields
    pub fn new(
        domains: Vec<String>,
        email: Vec<String>,
        wwwroot: PathBuf,
        port: u16,
    ) -> Self {
        Self {
            base: ServerConfig {
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
            },
            websocket_config: WebSocketConfig::default(),
            enable_metrics_logging: true,
        }
    }

    /// Set the history limit
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.base.limit = limit;
        self
    }

    /// Set the file limit
    pub fn with_filelimit(mut self, filelimit: usize) -> Self {
        self.base.filelimit = filelimit;
        self
    }

    /// Enable HTTP redirect for port 80
    pub fn with_redirect(mut self, redirect: bool) -> Self {
        self.base.redirect = redirect;
        self
    }

    /// Set Let's Encrypt staging mode
    pub fn with_staging(mut self, staging: bool) -> Self {
        self.base.staging = staging;
        self
    }

    /// Set ACME cache directory
    pub fn with_cache(mut self, cache: PathBuf) -> Self {
        self.base.cache = Some(cache);
        self
    }

    /// Set cache size for compressed files
    pub fn with_max_cache_size_mb(mut self, size: usize) -> Self {
        self.base.max_cache_size_mb = Some(size);
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

    /// Convert from legacy ServerConfig
    pub fn from_v1(config: ServerConfig) -> Self {
        Self {
            base: config,
            websocket_config: WebSocketConfig::default(),
            enable_metrics_logging: true,
        }
    }
}

/// Server handle for graceful shutdown and metrics access (v2.9+)
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

/// Run the Mles server with the given configuration (v2.8 API - deprecated)
///
/// # Deprecated
/// This function is maintained for backwards compatibility.
/// New code should use `run_v2()` which returns a `ServerHandle` for graceful shutdown.
#[deprecated(since = "2.9.0", note = "Use run_v2() for graceful shutdown support")]
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

    // Create basic metrics but don't expose them (v2.8 behavior)
    let metrics = Metrics::new();
    let shutdown_token = CancellationToken::new(); // Never cancelled in v2.8 API

    // Spawn WebSocket event loop (v2.8 style - no shutdown)
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

    // Serve TLS connections (v2.8 style - blocks forever)
    server::serve_tls(tls_incoming, tlsroutes).await;

    unreachable!()
}

/// Run the Mles server with enhanced configuration (v2.9+ API)
///
/// Returns a `ServerHandle` that can be used to:
/// - Trigger graceful shutdown
/// - Query server metrics
/// - Monitor server state
///
/// # Example
///
/// ```no_run
/// use mles::{run_v2, ServerConfigV2};
/// use std::path::PathBuf;
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     let config = ServerConfigV2::new(
///         vec!["example.com".to_string()],
///         vec!["admin@example.com".to_string()],
///         PathBuf::from("/var/www"),
///         443,
///     );
///
///     let handle = run_v2(config).await?;
///
///     // Wait for CTRL+C
///     tokio::signal::ctrl_c().await?;
///     handle.shutdown();
///
///     Ok(())
/// }
/// ```
pub async fn run_v2(config: ServerConfigV2) -> io::Result<ServerHandle> {
    run_v2_with_shutdown(config, CancellationToken::new()).await
}

/// Run the Mles server with external shutdown coordination (v2.9+ API)
///
/// This allows coordinating shutdown with other parts of your application
/// by providing an external `CancellationToken`.
pub async fn run_v2_with_shutdown(
    config: ServerConfigV2,
    shutdown_token: CancellationToken,
) -> io::Result<ServerHandle> {
    let limit = config.base.limit;
    let www_root_dir = config.base.wwwroot.clone();
    let filelimit = config.base.filelimit;
    let max_cache_size_mb = match config.base.max_cache_size_mb {
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
        "Starting Mles server v2.9 on port {} with {} file limit, WebSocket ping interval: {}ms",
        config.base.port,
        filelimit,
        ws_config.ping_interval_ms
    );

    // Spawn WebSocket event loop with shutdown support
    websocket::spawn_event_loop(rx, limit, metrics.clone(), shutdown_token.clone());

    // Spawn metrics logging task if enabled
    if config.enable_metrics_logging {
        let metrics_clone = metrics.clone();
        let shutdown_metrics = shutdown_token.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(METRICS_LOG_INTERVAL_SECS));
            interval.tick().await; // Skip first immediate tick

            loop {
                tokio::select! {
                    _ = shutdown_metrics.cancelled() => {
                        log::info!("Metrics logging task shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        let stats = metrics_clone.get_stats();
                        log::info!(
                            "Server Metrics - Connections: {}, Channels: {}, Messages Sent: {}, Messages Received: {}, Errors: {}",
                            stats.active_connections,
                            stats.total_channels,
                            stats.total_messages_sent,
                            stats.total_messages_received,
                            stats.total_errors
                        );
                    }
                }
            }
        });
    }

    // Create TCP listener
    let addr = format!("[{}]:{}", Ipv6Addr::UNSPECIFIED, config.base.port)
        .parse()
        .unwrap();
    let tcp_incoming = server::create_tcp_incoming(addr)?;

    // Create TLS incoming stream
    let tls_incoming = server::create_tls_incoming(
        config.base.domains.clone(),
        config.base.email,
        config.base.cache,
        config.base.staging,
        tcp_incoming,
        semaphore.clone(),
    );

    // Spawn HTTP redirect server if requested
    if config.base.redirect {
        log::info!("Starting HTTP redirect server on port 80");
        http::spawn_http_redirect_server(config.base.domains.clone());
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
        config.base.domains.clone(),
        www_root_dir.clone(),
        semaphore.clone(),
        compression_cache,
    );

    log::info!(
        "Serving static files from: {}",
        www_root_dir.display()
    );

    // Combine all routes
    let tlsroutes = ws.or(index);

    // Create server handle
    let handle = ServerHandle {
        shutdown_token: shutdown_token.clone(),
        metrics: metrics.clone(),
    };

    log::info!(
        "Mles server started successfully on {}:{}",
        config.base.domains.join(", "),
        config.base.port
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

    #[test]
    fn test_server_config_v2_builder() {
        let config = ServerConfigV2::new(
            vec!["example.com".to_string()],
            vec!["admin@example.com".to_string()],
            PathBuf::from("/var/www"),
            443,
        )
        .with_limit(200)
        .with_filelimit(2048)
        .with_redirect(true)
        .with_staging(true);

        assert_eq!(config.base.limit, 200);
        assert_eq!(config.base.filelimit, 2048);
        assert_eq!(config.base.redirect, true);
        assert_eq!(config.base.staging, true);
    }

    #[test]
    fn test_websocket_config_default() {
        let ws_config = WebSocketConfig::default();
        assert_eq!(ws_config.ping_interval_ms, 24000);
        assert_eq!(ws_config.max_missed_pongs, 3);
        assert_eq!(ws_config.rate_limit_messages_per_sec, 100);
    }

    #[test]
    fn test_v1_to_v2_conversion() {
        let v1_config = ServerConfig {
            domains: vec!["example.com".to_string()],
            email: vec!["admin@example.com".to_string()],
            cache: None,
            limit: 100,
            filelimit: 1024,
            wwwroot: PathBuf::from("/var/www"),
            staging: false,
            port: 443,
            redirect: false,
            max_cache_size_mb: None,
        };

        let v2_config = ServerConfigV2::from_v1(v1_config);
        assert_eq!(v2_config.base.limit, 100);
        assert_eq!(v2_config.base.port, 443);
        assert_eq!(v2_config.enable_metrics_logging, true);
    }
}
