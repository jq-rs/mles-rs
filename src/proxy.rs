/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use warp::Filter;
use warp::filters::BoxedFilter;

pub type HyperClient = Client<HttpConnector, Full<Bytes>>;

pub fn create_client() -> HyperClient {
    Client::builder(TokioExecutor::new()).build(HttpConnector::new())
}

/// Builds a filter that matches and strips a multi-segment path prefix like "api/keeper".
fn prefix_filter(prefix: &str) -> BoxedFilter<()> {
    let mut f: BoxedFilter<()> = warp::any().boxed();
    for seg in prefix.split('/').filter(|s| !s.is_empty()) {
        let seg = seg.to_string();
        f = f.and(warp::path(seg)).boxed();
    }
    f
}

/// Build proxy routes for a list of (prefix, upstream_url) pairs.
///
/// Requests to `/{prefix}/...` are forwarded to `{upstream}/...` (prefix stripped).
/// Prefix may contain multiple segments, e.g. "api/keeper".
pub fn create_proxy_routes(
    rules: &[(String, String)],
    client: HyperClient,
) -> BoxedFilter<(Box<dyn warp::Reply>,)> {
    let mut combined: Option<BoxedFilter<(Box<dyn warp::Reply>,)>> = None;

    for (prefix, upstream) in rules {
        let upstream = upstream.clone();
        let client = client.clone();

        let route = prefix_filter(prefix)
            .and(warp::path::tail())
            .and(
                warp::query::raw()
                    .or(warp::any().map(String::new))
                    .unify(),
            )
            .and(warp::method())
            .and(warp::header::headers_cloned())
            .and(warp::body::bytes())
            .and_then(
                move |tail: warp::path::Tail,
                      query: String,
                      method: warp::http::Method,
                      headers: warp::http::HeaderMap,
                      body: Bytes| {
                    let upstream = upstream.clone();
                    let client = client.clone();
                    async move {
                        forward(client, &upstream, tail.as_str(), &query, method, headers, body)
                            .await
                            .map_err(|e| {
                                log::error!("[proxy] {}", e);
                                warp::reject::reject()
                            })
                    }
                },
            )
            .map(|reply: Box<dyn warp::Reply>| reply)
            .boxed();

        combined = Some(match combined {
            None => route,
            Some(prev) => prev.or(route).unify().boxed(),
        });
    }

    combined.expect("create_proxy_routes called with empty rules")
}

async fn forward(
    client: HyperClient,
    upstream: &str,
    tail: &str,
    query: &str,
    method: warp::http::Method,
    headers: warp::http::HeaderMap,
    body: Bytes,
) -> Result<Box<dyn warp::Reply>, String> {
    let path = if tail.is_empty() {
        String::new()
    } else {
        format!("/{tail}")
    };
    let url = if query.is_empty() {
        format!("{upstream}{path}")
    } else {
        format!("{upstream}{path}?{query}")
    };

    let uri: hyper::Uri = url.parse().map_err(|e| format!("bad uri: {e}"))?;

    let mut req_builder = hyper::Request::builder()
        .method(method)
        .uri(uri);

    for (name, value) in &headers {
        if name == hyper::header::HOST {
            continue;
        }
        req_builder = req_builder.header(name, value);
    }

    let request = req_builder
        .body(Full::new(body))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(request)
        .await
        .map_err(|e| format!("upstream error: {e}"))?;

    let status = resp.status();
    let resp_headers = resp.headers().clone();

    let bytes: Bytes = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("collect body: {e}"))?
        .to_bytes();

    let mut reply = warp::reply::Response::new(bytes.into());
    *reply.status_mut() = status;
    let reply_headers = reply.headers_mut();
    for (name, value) in &resp_headers {
        if name == hyper::header::TRANSFER_ENCODING {
            continue;
        }
        reply_headers.insert(name.clone(), value.clone());
    }

    Ok(Box::new(reply) as Box<dyn warp::Reply>)
}
