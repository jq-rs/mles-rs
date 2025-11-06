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
```
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
  -d, --domains <DOMAINS>      Domain(s)
  -e, --email <EMAIL>          Contact info
  -c, --cache <CACHE>          Cache directory
  -l, --limit <LIMIT>          History limit [default: 200]
  -f, --filelimit <FILELIMIT>  Open files limit [default: 256]
  -w, --wwwroot <WWWROOT>      Www-root directory for domain(s) (e.g. /path/static where domain example.io goes to static/example.io)
  -s, --staging                Use Let's Encrypt staging environment (see https://letsencrypt.org/docs/staging-environment/)
  -p, --port <PORT>            [default: 443]
  -r, --redirect               Use http redirect for port 80
  -h, --help                   Print help
```
## Example server
 * Acquire a public Internet server with a static IP + domain
 * Open TLS port 443 of firewall
 * Ensure that your wwwroot-directory has the static web pages under it e.g. static/your.domain
 * Run Mles server as root (due to port 443) with Let's Encrypt caching and debug logging enabled as shown below

`RUST_LOG=debug mles --domains your.domain --cache . --wwwroot static` 

You can have several domains listed e.g. `--domains your.domain --domains www.your.domain`.

## Example clients

An example client session with `websocat` looks like this:

```
% websocat wss://mles.io --header "Sec-WebSocket-Protocol: mles-websocket"
{ "uid":"alice", "channel":"example" }
Hello Bob!
```
[mles-client](https://github.com/jq-rs/mles-client) is another example. It also includes a proxy implementation which can be used to connect channels of several servers together in a distributed manner.

## Existing client implementations over Mles (WebSocket) protocol

 * [MlesTalk](https://mles.io/app.html) in JavaScript
 * [mles-client](https://github.com/jq-rs/mles-client)
 * \<please ask to add your client here\>

## References:

 1. The Transport Layer Security (TLS) Protocol Version 1.3, https://tools.ietf.org/html/rfc8446
 2. The WebSocket Protocol, https://tools.ietf.org/html/rfc6455
 3. IANA registered Mles WebSocket Subprotocol, https://www.iana.org/assignments/websocket
