# mles-rs [![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
**Mles** [![crates.io status](https://img.shields.io/crates/v/mles.svg)](https://crates.io/crates/mles)

Mles (_Modern Lightweight channEl Service_) is a client-server data distribution protocol targeted to serve as a lightweight and reliable distributed publish-subscribe data service.

Please check https://mles.io and https://mles.io/blog.html for a generic overview of Mles and [/c/mles](https://lemmy.world/c/mles) for the latest news.

## Mles v2 protocol overview

Mles clients connect to a Mles server using Mles protocol header first frame (uid, channel, optional auth) value triplet on a TLS [1] WebSocket [2] session where the triplet is encapsulated in JavaScript Object Notation (JSON). A Mles client first subscribes to a channel by sending the correct Mles protocol header first frame value triplet (uid, channel, optional auth) where the channel specifies the channel to publish/subscribe. The auth MAY be included in case authentication is needed on top of WebSocket TLS. A Mles client MUST set Sec-WebSocket-Protocol subprotocol to "mles-websocket" [3] to be able to establish a connection with a Mles server successfully.

The Mles server verifies the Mles protocol header first frame and then joins the Mles client to the selected channel. Every channel uses its own context and is independent of other channels: therefore a TLS session per (uid, channel) pair is always used. After joining, the Mles server distributes the value triplet between all clients on the same channel. If a Mles client wants to depart from a channel, it just closes the WebSocket TLS session. If a Mles client does not want to receive any message, it just closes the WebSocket TLS session receive side.

A Mles server MAY save the history of received data, which can be then distributed to new clients when they connect to the Mles server. In case Mles server is restarted, Mles peer server, or even a Mles client, MAY provide history data for the Mles server as a resynchronization for those channels for which it has the history available. This allows distributed data protection for the channel information. A client SHOULD implement packet deduplication to avoid showing duplicate messages to user.

An Mles proxy-client MAY proxy a selected channel data between several Mles servers. Mles servers see proxy sessions as any other Mles client sessions. A proxy MUST implement packet deduplication to avoid forwarding loops for several proxies on the same channel. This allows Mles proxy-client to share and distribute value triplet data in an organized and powerful, yet simple manner between several Mles servers.

Mles clients and servers are independent of IP version and do not use IP broadcast or multicast. A Mles server MAY be configured to use IP anycast.

## Mles protocol details

The protocol header first frame for Mles is as follows:
```json
{
	"uid":"<user identification>",
	"channel":"<selected channel>",
	"auth":"<optional authentication hash>"
}
```

## Server usage
```
Usage: mles [OPTIONS] --domains <DOMAINS> --wwwroot <WWWROOT>

Options:
  -d, --domains <DOMAINS>
          Domain(s) - can be specified multiple times for multiple domains

  -e, --email <EMAIL>
          Contact info for Let's Encrypt certificate registration

  -c, --cache <CACHE>
          Cache directory for Let's Encrypt certificates

  -l, --limit <LIMIT>
          History limit - maximum number of messages to store per channel
          [default: 200]

  -f, --filelimit <FILELIMIT>
          Open files limit - maximum number of concurrent file handles
          [default: 256]

  -w, --wwwroot <WWWROOT>
          Www-root directory for domain(s)
          Structure: /path/static where domain example.io goes to static/example.io

  -s, --staging
          Use Let's Encrypt staging environment
          (see https://letsencrypt.org/docs/staging-environment/)

  -p, --port <PORT>
          TLS port for WebSocket connections
          [default: 443]

  -r, --redirect
          Enable HTTP to HTTPS redirect on port 80

  -C, --compression-cache <COMPRESSION_CACHE>
          Compression cache size in MB for static file serving
          [default: 10]
          Set to 0 to disable caching

      --per-ip-limit <PER_IP_LIMIT>
          Per-IP allowed connections per window (set 0 to disable). Default: 60
          This limits the number of concurrent accepted connections from a single source IP
          during the configured per-ip window. A value of 0 disables per-IP limiting.

      --per-ip-window <PER_IP_WINDOW>
          Per-IP window size in seconds. Default: 60
          The time window used to count connections for the per-ip limit.

  -h, --help
          Print help
```

## Example server
 * Acquire a public Internet server with a static IP + domain
 * Open TLS port 443 in your firewall
 * Ensure that your wwwroot-directory has the static web pages under it e.g. `static/your.domain`
 * Run Mles server as root (due to port 443) with Let's Encrypt caching and debug logging enabled as shown below

```bash
RUST_LOG=debug mles --domains your.domain --cache . --wwwroot static
```

### Multiple domains example

You can serve multiple domains by specifying the `--domains` flag multiple times:

```bash
RUST_LOG=debug mles \
  --domains your.domain \
  --domains www.your.domain \
  --cache . \
  --wwwroot static
```

### With HTTP redirect and compression cache

Enable HTTP to HTTPS redirect and use a 100MB compression cache for better performance:

```bash
RUST_LOG=debug mles \
  --domains your.domain \
  --cache . \
  --wwwroot static \
  --redirect \
  --compression-cache 100
```

### Production example with email contact

For production use, specify contact email for Let's Encrypt:

```bash
mles \
  --domains your.domain \
  --email admin@your.domain \
  --cache /var/cache/mles \
  --wwwroot /var/www \
  --redirect \
  --compression-cache 100 \
  --limit 500 \
  --filelimit 1024
```

## Example clients

An example client session with `websocat` looks like this:

```bash
% websocat wss://mles.io --header "Sec-WebSocket-Protocol: mles-websocket"
{ "uid":"alice", "channel":"example" }
Hello Bob!
```

[mles-client](https://github.com/jq-rs/mles-client) is another example. It also includes a proxy implementation which can be used to connect channels of several servers together in a distributed manner.

## Existing client implementations over Mles (WebSocket) protocol

 * [MlesTalk](https://mles.io/app.html) in JavaScript
 * [mles-client](https://github.com/jq-rs/mles-client)
 * \<please ask to add your client here\>

## Metrics and monitoring

Mles includes a lightweight, thread-safe metrics module intended for internal server observability. The metrics module exposes counts and small snapshots suitable for logging, tests and integration with external monitoring systems.

Key metrics provided:
 - active_connections: current number of connected WebSocket clients.
 - total_messages_sent: cumulative number of messages successfully sent to clients.
 - total_messages_received: cumulative number of messages received from clients.
 - total_channels: current number of active channels tracked by the server.
 - total_errors: cumulative number of error events encountered by the server.
 - total_dropped_connections: cumulative number of connections dropped due to policy (for example, per-IP limits).
 - dropped_by_ip: a small per-IP map of dropped connection counts.

The metrics API is synchronous and intentionally small to keep dependencies minimal and avoid tying the public API to async primitives. Typical usage is to create a `Metrics` instance early in server startup and pass clones to subsystems (WebSocket event loop, TLS acceptor, HTTP handlers, etc.). The server periodically logs a small snapshot of metrics for operational visibility.

On-demand metrics via signal (UNIX)
Mles also supports requesting an on-demand metrics snapshot at runtime on Unix-like systems by sending the SIGUSR1 signal to the process. When the server receives SIGUSR1 it logs an info-level snapshot that includes the same aggregate metrics as the periodic log (connections, channels, messages, errors, dropped connections) and — if present — a compact per-IP dropped-connections map. This is useful for quick operational inspection of rate-limiting or connection problems without restarting or changing configuration.

Example:
```
kill -USR1 <pid>
```

Note: the SIGUSR1 handler is installed only on Unix-like platforms. On non-Unix platforms (for example, Windows) this on-demand signal-triggered snapshot is not available.

Note: the metrics module is meant for in-process visibility and logging. If you need to export metrics to Prometheus, InfluxDB or other backends, adapt the snapshot values to your exporter of choice.

## Graceful shutdown and server handle

Mles provides a server-run API that returns a handle for graceful shutdown and runtime introspection. Use `run_with_shutdown(config)` to start the server and receive a `ServerHandle`. The handle offers:

 - `shutdown()` — request graceful shutdown from another task (cancels the internal shutdown token).
 - `metrics()` — retrieve a `MetricsSnapshot` with current counters suitable for logging or inspection.
 - `shutdown_token()` — retrieve a cloneable cancellation token for coordinating shutdown across tasks.
 - `wait_for_shutdown().await` — await shutdown completion.

Typical usage pattern:

```no_run
use mles::{run_with_shutdown, ServerConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = ServerConfig::new(
        vec!["example.com".to_string()],
        vec!["admin@example.com".to_string()],
        PathBuf::from("/var/www"),
        443,
    )
    .with_limit(200)
    .with_filelimit(2048)
    .with_websocket_config(/* ... */)
    .with_metrics_logging(true);

    let handle = run_with_shutdown(config).await?;

    // spawn a task to listen for CTRL+C and trigger shutdown
    let shutdown_token = handle.shutdown_token();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("failed to listen for ctrl-c");
        shutdown_token.cancel();
    });

    // wait for shutdown to complete
    handle.wait_for_shutdown().await;
    Ok(())
}
```

The graceful shutdown pathway ensures the WebSocket event loop and metric logging task are notified and that the server gives a short period for connections to finish before exiting.

## References

 1. The Transport Layer Security (TLS) Protocol Version 1.3, https://tools.ietf.org/html/rfc8446
 2. The WebSocket Protocol, https://tools.ietf.org/html/rfc6455
 3. IANA registered Mles WebSocket Subprotocol, https://www.iana.org/assignments/websocket
