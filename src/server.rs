/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use futures_util::StreamExt;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpSocket;
use tokio::sync::Semaphore;
use tokio_stream::wrappers::TcpListenerStream;

const BACKLOG: u32 = 1024;

pub fn create_tcp_incoming(addr: SocketAddr) -> io::Result<TcpListenerStream> {
    let socket = TcpSocket::new_v6()?;
    socket.set_keepalive(true)?;
    socket.set_nodelay(true)?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    let tcp_listener = socket.listen(BACKLOG)?;
    Ok(TcpListenerStream::new(tcp_listener))
}

pub fn create_tls_incoming(
    domains: Vec<String>,
    email: Vec<String>,
    cache: Option<PathBuf>,
    staging: bool,
    tcp_incoming: TcpListenerStream,
    semaphore: Arc<Semaphore>,
) -> impl futures_util::Stream<
    Item = Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin, std::io::Error>,
> {
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

pub async fn serve_http<F>(tcp_incoming: TcpListenerStream, routes: F)
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

pub async fn serve_tls<S, T, E, F>(tls_incoming: S, routes: F)
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
