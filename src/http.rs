/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use crate::compression::{compress, BR, ZSTD};
use crate::types::ReplyHeaders;
use http_types::mime;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;
use warp::filters::BoxedFilter;
use warp::http::StatusCode;
use warp::Filter;

pub async fn dyn_hreply(
    tuple: (String, String, warp::path::Tail),
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let (uri, domain, tail) = tuple;

    if uri != domain {
        return Err(warp::reject::not_found());
    }

    Ok(Box::new(warp::redirect::redirect(
        warp::http::Uri::from_str(&format!("https://{}/{}", &domain, tail.as_str()))
            .expect("problem with uri?"),
    )))
}

pub async fn dyn_reply(
    tuple: (
        Option<String>,
        String,
        String,
        String,
        warp::path::Tail,
        Arc<Semaphore>,
    ),
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let (encoding, uri, domain, www_root, tail, semaphore) = tuple;

    if uri != domain {
        return Err(warp::reject::not_found());
    }
    let mut path = tail.as_str();
    if path.is_empty() {
        path = "index.html";
    }
    let file_path = format!("{}/{}/{}", www_root, uri, path);

    let _permit = semaphore.acquire().await.unwrap();

    log::debug!("Avail file permits {}", semaphore.available_permits());
    log::debug!("Accessing {file_path}...");

    match File::open(&file_path).await {
        Ok(mut file) => {
            let parts: Vec<&str> = file_path.split('.').collect();
            let ctype = match parts.last() {
                Some(v) => {
                    let mime = mime::Mime::from_extension(*v);
                    match mime {
                        Some(mime) => format!("{}/{}", mime.basetype(), mime.subtype()),
                        None => match *v {
                            "png" => format!("{}/{}", mime::PNG.basetype(), mime::PNG.subtype()),
                            "jpg" => format!("{}/{}", mime::JPEG.basetype(), mime::JPEG.subtype()),
                            "ico" => format!("{}/{}", mime::ICO.basetype(), mime::ICO.subtype()),
                            "apk" => "application/vnd.android.package-archive".to_string(),
                            _ => format!("{}/{}", mime::ANY.basetype(), mime::ANY.subtype()),
                        },
                    }
                }
                None => format!("{}/{}", mime::ANY.basetype(), mime::ANY.subtype()),
            };

            let mut buffer = Vec::new();
            if (file.read_to_end(&mut buffer).await).is_err() {
                log::debug!("...FAILED!");
                return Ok(Box::new(warp::reply::with_status(
                    "Internal Server Error",
                    StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
            log::debug!("...OK, ctype {ctype}");

            let mut reply_headers = ReplyHeaders::NONE;
            let mut use_br = false;
            let mut use_zstd = false;
            if let Some(encoding) = encoding {
                log::debug!("Encoding: {encoding}");
                if ctype.contains("text") || ctype.contains("json") {
                    if encoding.contains(ZSTD) {
                        if let Ok(compressed_buffer) = compress(ZSTD, &buffer).await {
                            buffer = compressed_buffer;
                            reply_headers = ReplyHeaders::Zstd;
                            use_zstd = true;
                        }
                    } else if encoding.contains(BR) {
                        if let Ok(compressed_buffer) = compress(BR, &buffer).await {
                            buffer = compressed_buffer;
                            reply_headers = ReplyHeaders::Br;
                            use_br = true;
                        }
                    }
                }
            }
            let mut use_allow_origin = false;
            if ctype.contains("json") {
                use_allow_origin = true;
                reply_headers = ReplyHeaders::AllowOrigin;
            }
            if use_zstd && use_allow_origin {
                reply_headers = ReplyHeaders::ZstdWithAllowOrigin;
            } else if use_br && use_allow_origin {
                reply_headers = ReplyHeaders::BrWithAllowOrigin;
            }
            log::debug!("Reply headers: {reply_headers:?}");
            match reply_headers {
                ReplyHeaders::NONE => Ok(Box::new(warp::reply::with_header(
                    warp::reply::Response::new(buffer.into()),
                    "Content-Type",
                    &ctype,
                ))),
                ReplyHeaders::Br => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::Response::new(buffer.into()),
                        "Content-Type",
                        &ctype,
                    ),
                    "Content-Encoding",
                    BR,
                ))),
                ReplyHeaders::Zstd => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::Response::new(buffer.into()),
                        "Content-Type",
                        &ctype,
                    ),
                    "Content-Encoding",
                    ZSTD,
                ))),
                ReplyHeaders::AllowOrigin => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::Response::new(buffer.into()),
                        "Content-Type",
                        &ctype,
                    ),
                    "Access-Control-Allow-Origin",
                    "*",
                ))),
                ReplyHeaders::ZstdWithAllowOrigin => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::with_header(
                            warp::reply::Response::new(buffer.into()),
                            "Content-Type",
                            &ctype,
                        ),
                        "Content-Encoding",
                        ZSTD,
                    ),
                    "Access-Control-Allow-Origin",
                    "*",
                ))),
                ReplyHeaders::BrWithAllowOrigin => Ok(Box::new(warp::reply::with_header(
                    warp::reply::with_header(
                        warp::reply::with_header(
                            warp::reply::Response::new(buffer.into()),
                            "Content-Type",
                            &ctype,
                        ),
                        "Content-Encoding",
                        BR,
                    ),
                    "Access-Control-Allow-Origin",
                    "*",
                ))),
            }
        }
        Err(_) => {
            log::debug!("{file_path} does not exist");
            Ok(Box::new(warp::reply::with_status(
                "Not Found",
                StatusCode::NOT_FOUND,
            )))
        }
    }
}

pub fn create_http_redirect_routes(
    domains: Vec<String>,
) -> BoxedFilter<(impl warp::Reply,)> {
    let mut http_index = Vec::new();
    for domain in domains {
        let redirect = warp::get()
            .and(warp::header::<String>("host"))
            .and(warp::path::tail())
            .map(move |uri: String, path: warp::path::Tail| (uri, domain.clone(), path))
            .and_then(dyn_hreply);
        http_index.push(redirect);
    }

    let mut hindex: BoxedFilter<_> = http_index.swap_remove(0).boxed();
    for val in http_index {
        hindex = val.or(hindex).unify().boxed();
    }
    hindex
}

pub fn create_http_file_routes(
    domains: Vec<String>,
    www_root_dir: PathBuf,
    semaphore: Arc<Semaphore>,
) -> BoxedFilter<(impl warp::Reply,)> {
    let mut vindex = Vec::new();
    for domain in domains {
        let sem = semaphore.clone();
        let www_root = www_root_dir.clone();
        let index = warp::get()
            .and(warp::header::optional::<String>("accept-encoding"))
            .and(warp::header::<String>("host"))
            .and(warp::path::tail())
            .map(
                move |encoding: Option<String>, uri: String, path: warp::path::Tail| {
                    (
                        encoding,
                        uri,
                        domain.clone(),
                        www_root.to_str().unwrap().to_string(),
                        path,
                        sem.clone(),
                    )
                },
            )
            .and_then(dyn_reply);

        vindex.push(index);
    }

    let mut index: BoxedFilter<_> = vindex.swap_remove(0).boxed();
    for val in vindex {
        index = val.or(index).unify().boxed();
    }
    index
}

pub fn spawn_http_redirect_server(domains: Vec<String>) {
    tokio::spawn(async move {
        let addr = format!("[{}]:{}", Ipv6Addr::UNSPECIFIED, 80)
            .parse()
            .unwrap();
        if let Ok(tcp_incoming) = crate::server::create_tcp_incoming(addr) {
            let hindex = create_http_redirect_routes(domains);
            crate::server::serve_http(tcp_incoming, hindex).await;
        }
    });
}