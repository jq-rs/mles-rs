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

use crate::server::ClientAddr;

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
///
/// The client's address is appended to `X-Forwarded-For`, so an upstream
/// on the same host (which only ever sees mles as its peer) can learn it.
/// An upstream should trust that header only from mles itself: the
/// rightmost entry is the one mles added, anything before it came from the
/// client and may be forged.
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
            .and(warp::ext::optional::<ClientAddr>())
            .and(warp::body::bytes())
            .and_then(
                move |tail: warp::path::Tail,
                      query: String,
                      method: warp::http::Method,
                      headers: warp::http::HeaderMap,
                      client_addr: Option<ClientAddr>,
                      body: Bytes| {
                    let upstream = upstream.clone();
                    let client = client.clone();
                    let headers = with_forwarded_for(headers, client_addr.map(|a| a.0.ip()));
                    async move {
                        forward(
                            client,
                            &upstream,
                            tail.as_str(),
                            &query,
                            method,
                            headers,
                            body,
                        )
                        .await
                        .or_else(|e| {
                            log::error!("[proxy] {}", e);
                            Ok::<_, warp::Rejection>(upstream_unavailable())
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

/// What a client gets when the upstream can't be reached or drops the
/// connection: a JSON 502 it can retry, not a rejection that falls through to
/// the static routes (which answer a POST with "405 HTTP method not allowed").
fn upstream_unavailable() -> Box<dyn warp::Reply> {
    let body = r#"{"error":"The service behind this proxy is unavailable","code":"UPSTREAM_UNAVAILABLE","retryable":true}"#;
    let mut reply = warp::reply::Response::new(body.into());
    *reply.status_mut() = warp::http::StatusCode::BAD_GATEWAY;
    reply.headers_mut().insert(
        warp::http::header::CONTENT_TYPE,
        warp::http::HeaderValue::from_static("application/json"),
    );
    Box::new(reply)
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

const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// The request headers with the client's address appended to
/// `X-Forwarded-For` (incoming entries merged into one header).
fn with_forwarded_for(
    mut headers: warp::http::HeaderMap,
    client_ip: Option<std::net::IpAddr>,
) -> warp::http::HeaderMap {
    if let Some(value) =
        forwarded_for(&headers, client_ip).and_then(|v| warp::http::HeaderValue::from_str(&v).ok())
    {
        headers.insert(X_FORWARDED_FOR, value);
    }
    headers
}

/// The `X-Forwarded-For` value to send upstream: any incoming entries (joined
/// in order, as the standard asks) followed by the client's address. Without
/// a known client address the incoming header is passed on unchanged.
fn forwarded_for(
    headers: &warp::http::HeaderMap,
    client_ip: Option<std::net::IpAddr>,
) -> Option<String> {
    let prior: Vec<&str> = headers
        .get_all(X_FORWARDED_FOR)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect();
    match (prior.is_empty(), client_ip) {
        (true, None) => None,
        (true, Some(ip)) => Some(ip.to_string()),
        (false, None) => Some(prior.join(", ")),
        (false, Some(ip)) => Some(format!("{}, {}", prior.join(", "), ip)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn headers(values: &[&str]) -> warp::http::HeaderMap {
        let mut h = warp::http::HeaderMap::new();
        for v in values {
            h.append(X_FORWARDED_FOR, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn adds_the_client_address() {
        let ip: IpAddr = "81.2.3.4".parse().unwrap();
        assert_eq!(
            forwarded_for(&headers(&[]), Some(ip)).as_deref(),
            Some("81.2.3.4")
        );
    }

    #[test]
    fn appends_after_incoming_entries_so_the_last_one_is_ours() {
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert_eq!(
            forwarded_for(&headers(&["10.0.0.1", "192.0.2.7, 198.51.100.2"]), Some(ip)).as_deref(),
            Some("10.0.0.1, 192.0.2.7, 198.51.100.2, 2001:db8::1")
        );
    }

    #[test]
    fn without_a_client_address_passes_the_header_on() {
        assert_eq!(
            forwarded_for(&headers(&["192.0.2.7"]), None).as_deref(),
            Some("192.0.2.7")
        );
        assert_eq!(forwarded_for(&headers(&[]), None), None);
    }

    #[test]
    fn the_forwarded_request_carries_one_merged_header() {
        let ip: IpAddr = "81.2.3.4".parse().unwrap();
        let h = with_forwarded_for(headers(&["10.0.0.1", "192.0.2.7"]), Some(ip));
        let all: Vec<_> = h.get_all(X_FORWARDED_FOR).iter().collect();
        assert_eq!(all, ["10.0.0.1, 192.0.2.7, 81.2.3.4"]);
    }

    #[tokio::test]
    async fn an_unreachable_upstream_is_a_json_502_not_a_fallthrough() {
        // Nothing listens on port 1: the connection is refused.
        let routes = create_proxy_routes(&[("api/keeper".to_string(), "http://127.0.0.1:1".to_string())], create_client());
        let res = warp::test::request()
            .method("POST")
            .path("/api/keeper/prove/deploy")
            .body("{}")
            .reply(&routes)
            .await;
        assert_eq!(res.status(), 502);
        assert_eq!(res.headers()["content-type"], "application/json");
        let body: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        assert_eq!(body["code"], "UPSTREAM_UNAVAILABLE");
        assert_eq!(body["retryable"], true);
    }

    #[test]
    fn ipv4_clients_on_the_dual_stack_listener_are_plain_ipv4() {
        let mapped: IpAddr = "::ffff:81.2.3.4".parse().unwrap();
        assert_eq!(mapped.to_canonical().to_string(), "81.2.3.4");
    }
}
