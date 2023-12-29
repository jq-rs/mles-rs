# mles-rs [![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
**Mles** [![crates.io status](https://img.shields.io/crates/v/mles.svg)](https://crates.io/crates/mles)

Mles (_Modern Lightweight channEl Service_) is a client-server data distribution protocol targeted to serve as a lightweight and reliable distributed publish-subscribe data service. The reference implementation consists of _Mles-utils_, _Mles_ server and _Mles-client/WebSocket-proxy_.

Please check https://mles.io and https://mles.io/blog.html for a generic overview of Mles and [/c/mles](https://lemmy.world/c/mles) for the latest news.

**_Notice: Mles version 1.X is deprecated and will be obsolete from the beginning of year 2024. Please consider upgrading to Mles v2. Please check discussion and details at [/c/mles](https://lemmy.world/c/mles)!_**

## Mles 2.0 protocol overview

Mles clients connect to a Mles server using Mles protocol header first frame (uid, channel, optional auth) value triplet on a TLS [1] WebSocket [2] session where the triplet is JavaScript Object Notation (JSON) encapsulated. A Mles client first subscribes to a channel by sending the correct Mles protocol header first frame value triplet (uid, channel, optional auth) where the _channel_ is the channel to publish/subscribe. The _auth_ can be included in case authentication is needed on top of WebSocket TLS. A Mles client MUST set Sec-WebSocket-Protocol subprotocol to "mles-websocket" [3] to be able to establish a connection to a Mles client with Mles server successfully. 

The Mles server verifies Mles protocol header first frame and then joins the Mles client to the selected channel. Every channel uses its own context and is independent of other channels: therefore a TLS session per (uid, channel) pair is always used. After joining, the Mles server distributes the value triplet between all clients on the same channel. If a Mles client wants to depart from a channel, it just closes the Websocket TLS session. If a Mles client does not want to receive any message, it just closes WebSocket TLS session receive side.

A Mles server MAY contact a Mles peer server. The Mles peer server sees this session as another Mles client session. This allows Mles servers to share and distribute value triplet data in an organized and powerful, yet simple manner between other Mles servers.

A Mles server MAY save the history of received data, which can be then distributed to new clients when they connect to the Mles server. In case Mles server is restarted, Mles peer server, or even a Mles client, MAY provide history data for the Mles server as a resynchronization for those channels that it has the history available. This allows distributed data protection for the channel information.  

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

## Existing client implementations over Mles (WebSocket) protocol

 * [MlesTalk](https://mles.io/app.html) in JavaScript
 * <Please add your client here!>

## References:

 1. The Transport Layer Security (TLS) Protocol Version 1.3, https://tools.ietf.org/html/rfc8446
 2. The WebSocket Protocol, https://tools.ietf.org/html/rfc6455
 3. IANA registered Mles WebSocket Subprotocol, https://www.iana.org/assignments/websocket
