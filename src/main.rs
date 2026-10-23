/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use clap::Parser;
use log::LevelFilter;
use mles::ServerConfig;
use simple_logger::SimpleLogger;
use std::io;
use std::num::NonZeroU32;
use std::path::PathBuf;

const HISTORY_LIMIT: &str = "200";
const MAX_FILES_OPEN: &str = "256";
const TLS_PORT: &str = "443";
// Room for a corporate NAT or IPv6 /64 with many legit users, but well
// below the connection-semaphore cap so no single source can starve others.
const PER_IP_CONN_LIMIT: &str = "32";

#[derive(Parser, Debug)]
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

    #[arg(short, long, default_value = TLS_PORT, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    /// Use http redirect for port 80
    #[arg(short, long)]
    redirect: bool,

    /// Compression cache size in MB
    #[arg(short = 'C', long)]
    compression_cache: Option<usize>,

    /// Reverse-proxy rules in the form prefix=http://upstream:port
    /// Example: --proxy keeper=http://localhost:8080 --proxy verify=http://localhost:8081
    #[arg(short = 'P', long = "proxy")]
    proxy: Vec<String>,

    /// Per-IP TCP-accept rate limit (connections/minute). Applies to ports 80
    /// and the TLS port. Connections beyond the limit are dropped before TLS.
    /// Omit to disable.
    #[arg(long = "rate-limit", value_parser = clap::value_parser!(u32).range(1..))]
    rate_limit: Option<u32>,

    /// Maximum concurrent live connections per source IP (or IPv6 /64 prefix).
    /// Bounds an attacker who exhausts the rate-limit burst then keeps
    /// connections open under the header-read timeout. Applies to both TLS
    /// and HTTP redirect listeners with independent counters.
    #[arg(long = "per-ip-limit", default_value = PER_IP_CONN_LIMIT, value_parser = clap::value_parser!(u32).range(1..))]
    per_ip_limit: u32,

    /// Add cross-origin headers to files served from the web root: COOP,
    /// COEP and CORP on every file (cross-origin isolation, needed by
    /// SharedArrayBuffer and WebAssembly threads), and
    /// `Access-Control-Allow-Origin: *` on JSON files. Off by default.
    #[arg(long)]
    cors: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Warn)
        .env()
        .init()
        .unwrap();

    let args = Args::parse();

    let proxies: Vec<(String, String)> = args
        .proxy
        .iter()
        .filter_map(|s| {
            let mut parts = s.splitn(2, '=');
            let prefix = parts.next()?.trim().to_string();
            let upstream = parts.next()?.trim().to_string();
            Some((prefix, upstream))
        })
        .collect();

    let config = ServerConfig {
        domains: args.domains,
        email: args.email,
        cache: args.cache,
        limit: args.limit,
        filelimit: args.filelimit as usize,
        wwwroot: args.wwwroot,
        staging: args.staging,
        port: args.port,
        redirect: args.redirect,
        max_cache_size_mb: args.compression_cache,
        proxies,
        rate_limit: args.rate_limit.and_then(NonZeroU32::new),
        per_ip_conn_limit: NonZeroU32::new(args.per_ip_limit)
            .expect("clap enforces per-ip-limit >= 1"),
        cors: args.cors,
    };

    mles::run(config).await
}
