/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use crate::cache;
use crate::compression::{BR, ZSTD, compress};
use http_types::mime;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;
use warp::Filter;
use warp::filters::BoxedFilter;
use warp::http::{Response, StatusCode};

pub(crate) async fn dyn_hreply(
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

// Wrap a file response with the headers required for browsers to enable
// crossOriginIsolated (needed by SharedArrayBuffer + WebAssembly threads).
// COOP/COEP gate the top-level document; CORP authorises every same-origin
// sub-resource (JS/CSS/WASM) so COEP doesn't block them.
// Also stacks the baseline security headers via with_security_headers so
// every HTTPS reply gets the full hardening set.
fn with_isolation<T: warp::Reply>(reply: T) -> impl warp::Reply {
    warp::reply::with_header(
        warp::reply::with_header(
            warp::reply::with_header(
                with_security_headers(reply),
                "Cross-Origin-Opener-Policy",
                "same-origin",
            ),
            "Cross-Origin-Embedder-Policy",
            "require-corp",
        ),
        "Cross-Origin-Resource-Policy",
        "same-origin",
    )
}

// The security headers go on every file response; the cross-origin ones
// (isolation above, and Access-Control-Allow-Origin on JSON) only with --cors.
fn with_file_headers<T: warp::Reply + 'static>(reply: T, cors: bool) -> Box<dyn warp::Reply> {
    if cors {
        Box::new(with_isolation(reply))
    } else {
        Box::new(with_security_headers(reply))
    }
}

// Baseline security hardening headers applied to every HTTPS file response.
// - HSTS pins the browser to HTTPS for a year so an active MITM on the first
//   subsequent request can't downgrade with a rewritten redirect.
// - nosniff stops the browser from MIME-sniffing responses away from the
//   declared Content-Type (mles-rs falls back to */* for unknown extensions).
// - DENY blocks the site from being loaded in iframes (clickjacking).
// - strict-origin-when-cross-origin keeps full URL paths (which may contain
//   licenseHash/txHash query strings) from leaking to third-party links.
fn with_security_headers<T: warp::Reply>(reply: T) -> impl warp::Reply {
    warp::reply::with_header(
        warp::reply::with_header(
            warp::reply::with_header(
                warp::reply::with_header(
                    reply,
                    "Strict-Transport-Security",
                    "max-age=31536000; includeSubDomains; preload",
                ),
                "X-Content-Type-Options",
                "nosniff",
            ),
            "X-Frame-Options",
            "DENY",
        ),
        "Referrer-Policy",
        "strict-origin-when-cross-origin",
    )
}

fn content_type(file_path: &str) -> String {
    let ext = file_path.rsplit('.').next().unwrap_or("");
    match mime::Mime::from_extension(ext) {
        Some(m) => format!("{}/{}", m.basetype(), m.subtype()),
        None => match ext {
            "png" => format!("{}/{}", mime::PNG.basetype(), mime::PNG.subtype()),
            "jpg" => format!("{}/{}", mime::JPEG.basetype(), mime::JPEG.subtype()),
            "ico" => format!("{}/{}", mime::ICO.basetype(), mime::ICO.subtype()),
            "apk" => "application/vnd.android.package-archive".to_string(),
            _ => format!("{}/{}", mime::ANY.basetype(), mime::ANY.subtype()),
        },
    }
}

fn is_compressible(ctype: &str) -> bool {
    ctype.contains("text")
        || ctype.contains("json")
        || ctype.contains("javascript")
        || ctype.contains("svg")
        || ctype.contains("wasm")
}

fn err500() -> Box<dyn warp::Reply> {
    Box::new(warp::reply::with_status(
        "Internal Server Error",
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

type DynReplyArgs = (
    Option<String>, // accept-encoding
    Option<String>, // if-modified-since
    String,         // host
    String,         // domain
    String,         // www_root
    warp::path::Tail,
    cache::SharedCache,
    bool, // cors
);

pub(crate) async fn dyn_reply(
    tuple: DynReplyArgs,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let (encoding, ims, uri, domain, www_root, tail, cache, cors) = tuple;

    if uri != domain {
        return Err(warp::reject::not_found());
    }
    let mut path = tail.as_str();
    if path.is_empty() {
        path = "index.html";
    }
    let file_path = format!("{}/{}/{}", www_root, uri, path);

    log::debug!("Accessing {file_path}...");

    // Stat first: need mtime for 304 evaluation before we decide to read.
    let metadata = match tokio::fs::metadata(&file_path).await {
        Ok(m) => m,
        Err(_) => {
            log::debug!("{file_path} does not exist");
            return Ok(Box::new(warp::reply::with_status(
                "Not Found",
                StatusCode::NOT_FOUND,
            )));
        }
    };
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let last_modified = httpdate::fmt_http_date(modified);

    // Conditional request: 304 if the client's cached copy is still current.
    // HTTP dates have 1-second resolution, so compare truncated seconds.
    if let Some(ims) = ims.as_deref()
        && let Ok(since) = httpdate::parse_http_date(ims)
        && secs(modified) <= secs(since)
    {
        let resp = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("Last-Modified", &last_modified)
            .body(Vec::<u8>::new())
            .unwrap_or_else(|_| Response::new(Vec::new()));
        return Ok(with_file_headers(resp, cors));
    }

    let ctype = content_type(&file_path);

    let want_zstd = is_compressible(&ctype)
        && encoding.as_deref().map(|e| e.contains(ZSTD)).unwrap_or(false);
    let want_br = is_compressible(&ctype)
        && encoding.as_deref().map(|e| e.contains(BR)).unwrap_or(false);

    let mut body_bytes: Option<Vec<u8>> = None;
    let mut chosen_encoding: Option<&str> = None;

    // Check the compression cache BEFORE touching disk. Previous version always
    // read the file first, throwing away that I/O on every cache hit.
    if want_zstd
        && let Some(hit) = cache.write().await.get(&file_path, ZSTD, modified)
    {
        log::debug!("Cache hit: {file_path} (zstd)");
        body_bytes = Some((*hit).clone());
        chosen_encoding = Some(ZSTD);
    } else if want_br
        && let Some(hit) = cache.write().await.get(&file_path, BR, modified)
    {
        log::debug!("Cache hit: {file_path} (br)");
        body_bytes = Some((*hit).clone());
        chosen_encoding = Some(BR);
    }

    // Cache miss (or non-compressible / no accepted encoding): read the file.
    if body_bytes.is_none() {
        let mut file = match File::open(&file_path).await {
            Ok(f) => f,
            Err(_) => return Ok(err500()),
        };
        let mut raw = Vec::with_capacity(metadata.len() as usize);
        if file.read_to_end(&mut raw).await.is_err() {
            log::debug!("...FAILED!");
            return Ok(err500());
        }
        log::debug!("...OK, ctype {ctype}");

        if want_zstd
            && let Ok(c) = compress(ZSTD, &raw).await
        {
            log::debug!("Cache miss: {file_path} (zstd)");
            cache
                .write()
                .await
                .insert(&file_path, ZSTD, modified, c.clone());
            body_bytes = Some(c);
            chosen_encoding = Some(ZSTD);
        } else if want_br
            && let Ok(c) = compress(BR, &raw).await
        {
            log::debug!("Cache miss: {file_path} (br)");
            cache
                .write()
                .await
                .insert(&file_path, BR, modified, c.clone());
            body_bytes = Some(c);
            chosen_encoding = Some(BR);
        }
        if body_bytes.is_none() {
            body_bytes = Some(raw);
        }
    }

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &ctype)
        .header("Last-Modified", &last_modified);
    if let Some(enc) = chosen_encoding {
        builder = builder.header("Content-Encoding", enc);
    }
    if cors && ctype.contains("json") {
        builder = builder.header("Access-Control-Allow-Origin", "*");
    }

    match builder.body(body_bytes.unwrap()) {
        Ok(r) => Ok(with_file_headers(r, cors)),
        Err(_) => Ok(err500()),
    }
}

pub(crate) fn create_http_redirect_routes(
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

pub(crate) fn create_http_file_routes(
    domains: Vec<String>,
    www_root_dir: PathBuf,
    compression_cache: cache::SharedCache,
    cors: bool,
) -> BoxedFilter<(Box<dyn warp::Reply>,)> {
    let mut vindex = Vec::new();
    for domain in domains {
        let www_root = www_root_dir.clone();
        let cache = compression_cache.clone();
        let index = warp::get()
            .and(warp::header::optional::<String>("accept-encoding"))
            .and(warp::header::optional::<String>("if-modified-since"))
            .and(warp::header::<String>("host"))
            .and(warp::path::tail())
            .map(
                move |encoding: Option<String>,
                      ims: Option<String>,
                      uri: String,
                      path: warp::path::Tail| {
                    (
                        encoding,
                        ims,
                        uri,
                        domain.clone(),
                        www_root.to_str().unwrap().to_string(),
                        path,
                        cache.clone(),
                        cors,
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

pub(crate) fn spawn_http_redirect_server(
    domains: Vec<String>,
    rate_limiter: Option<Arc<crate::server::IpRateLimiter>>,
    semaphore: Arc<Semaphore>,
    per_ip: Arc<crate::server::PerIpLimiter>,
) {
    tokio::spawn(async move {
        let addr = format!("[{}]:{}", Ipv6Addr::UNSPECIFIED, 80)
            .parse()
            .unwrap();
        if let Ok(tcp_incoming) = crate::server::create_tcp_incoming(addr) {
            let hindex = create_http_redirect_routes(domains);
            let policed =
                crate::server::enforce_ip_policy(tcp_incoming, rate_limiter, per_ip);
            crate::server::serve_http(policed, hindex, semaphore).await;
        }
    });
}
