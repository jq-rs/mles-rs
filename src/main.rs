/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use clap::Parser;
use log::LevelFilter;
use mles::{ServerConfigV2, WebSocketConfig};
use simple_logger::SimpleLogger;
use std::io;
use std::path::PathBuf;

const HISTORY_LIMIT: &str = "200";
const MAX_FILES_OPEN: &str = "256";
const TLS_PORT: &str = "443";
const DEFAULT_PING_INTERVAL: &str = "24000";
const DEFAULT_MAX_MISSED_PONGS: &str = "3";
const DEFAULT_RATE_LIMIT: &str = "100";
const DEFAULT_COMPRESSION_CACHE_MB: &str = "10";

fn compression_cache_parser(s: &str) -> Result<usize, String> {
    let value: usize = s.parse().map_err(|_| format!("'{}' is not a valid number", s))?;
    if value > 10000 {
        Err(format!("compression cache size must be between 0 and 10000, got {}", value))
    } else {
        Ok(value)
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about = "Mles - Minimal Live Event Server", long_about = None)]
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

    /// Www-root directory for domain(s)
    #[arg(short, long, required = true)]
    wwwroot: PathBuf,

    /// Use Let's Encrypt staging environment
    #[arg(short, long)]
    staging: bool,

    /// Server port
    #[arg(short, long, default_value = TLS_PORT, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    /// Use http redirect for port 80
    #[arg(short, long)]
    redirect: bool,

    /// Compression cache size in MB (0 to disable)
    #[arg(short = 'C', long, default_value = DEFAULT_COMPRESSION_CACHE_MB, value_parser = compression_cache_parser)]
    compression_cache: usize,

    /// WebSocket ping interval in milliseconds
    #[arg(long, default_value = DEFAULT_PING_INTERVAL, value_parser = clap::value_parser!(u64).range(1000..))]
    ping_interval: u64,

    /// Maximum missed pongs before closing connection
    #[arg(long, default_value = DEFAULT_MAX_MISSED_PONGS, value_parser = clap::value_parser!(u64).range(1..10))]
    max_missed_pongs: u64,

    /// Rate limit: maximum messages per second per connection
    #[arg(long, default_value = DEFAULT_RATE_LIMIT, value_parser = clap::value_parser!(u32).range(1..10000))]
    rate_limit: u32,

    /// Disable periodic metrics logging
    #[arg(long)]
    no_metrics_logging: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Warn)
        .env()
        .init()
        .unwrap();

    let args = Args::parse();

    // Create WebSocket configuration
    let ws_config = WebSocketConfig {
        ping_interval_ms: args.ping_interval,
        max_missed_pongs: args.max_missed_pongs,
        rate_limit_messages_per_sec: args.rate_limit,
        rate_limit_window_secs: 1,
    };

    // Create server configuration using v2.9 API
    let config = ServerConfigV2::new(
        args.domains.clone(),
        args.email,
        args.wwwroot.clone(),
        args.port,
    )
    .with_limit(args.limit)
    .with_filelimit(args.filelimit as usize)
    .with_redirect(args.redirect)
    .with_staging(args.staging)
    .with_websocket_config(ws_config)
    .with_metrics_logging(!args.no_metrics_logging);

    // Set cache if provided
    let config = if let Some(cache) = args.cache {
        config.with_cache(cache)
    } else {
        config
    };

    // Set compression cache size (0 disables caching)
    let config = if args.compression_cache > 0 {
        config.with_max_cache_size_mb(args.compression_cache)
    } else {
        config
    };

    log::info!(
        "Starting Mles v2.9 server on {}:{} serving {}",
        args.domains.join(", "),
        args.port,
        args.wwwroot.display()
    );

    log::info!(
        "WebSocket config: ping={}ms, max_pongs={}, rate_limit={}/s",
        args.ping_interval,
        args.max_missed_pongs,
        args.rate_limit
    );

    // Start server with v2.9 API
    let handle = mles::run_v2(config).await?;

    // Setup graceful shutdown on CTRL+C
    let shutdown_handle = handle.shutdown_token();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                log::info!("Received CTRL+C, initiating graceful shutdown...");
                shutdown_handle.cancel();
            }
            Err(err) => {
                log::error!("Failed to listen for CTRL+C: {}", err);
            }
        }
    });

    // Wait for shutdown to complete
    handle.wait_for_shutdown().await;
    log::info!("Server shutdown complete");

    // Give a moment for final cleanup
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    Ok(())
}
