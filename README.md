# mles-rs [![Build Status](https://travis-ci.org/jq-rs/mles-rs.svg?branch=master)](https://travis-ci.org/jq-rs/mles-rs) [![Build Status](https://ci.appveyor.com/api/projects/status/github/jq-rs/mles-rs?svg=true)](https://ci.appveyor.com/project/jq-rs/mles-rs) [![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
**Mles-utils** [![crates.io status](https://img.shields.io/crates/v/mles-utils.svg)](https://crates.io/crates/mles-utils)
[![Documentation](https://docs.rs/mles-utils/badge.svg)](https://docs.rs/mles-utils/1.0.0/mles_utils/)
**Mles** [![crates.io status](https://img.shields.io/crates/v/mles.svg)](https://crates.io/crates/mles)
**Mles-client** [![crates.io status](https://img.shields.io/crates/v/mles-client.svg)](https://crates.io/crates/mles-client)

Mles (_Modern Lightweight channEl Service_) is a client-server data distribution protocol targeted to serve as a lightweight and reliable distributed publish-subscribe data service. The reference implementation consists of _Mles-utils_, _Mles_ server and _Mles-client/WebSocket-proxy_.

Please check https://mles.io and https://mles.io/blog.html for a generic overview of Mles and [/r/mles](https://reddit.com/r/mles) for the latest news.

## Mles protocol overview

Mles clients connect to a Mles server using Mles protocol header and (uid, channel, message) value triplet on a TCP or TLS [1] session where the triplet is Concise Binary Object Representation (CBOR) [1] encapsulated. A Mles client first subscribes to a channel by sending correct Mles protocol header and value triplet (uid, channel, msg) where the _channel_ is the channel to publish/subscribe. The _msg_ MAY be either empty or include the first published message. The Mles server verifies Mles protocol header and then joins the Mles client to the selected channel. Every channel uses its own context and is independent of other channels: therefore a TCP/TLS session per (uid, channel) pair is always used. After joining, the Mles server distributes the value triplet between all clients on the same channel. If a Mles client wants to depart from a channel, it just closes the TCP/TLS session. If a Mles client does not want to receive any message, it just closes TCP/TLS session receive side. TLS session SHOULD be selected as a session type by default.

Every session between Mles server and client is authenticated using 64-bit SipHash [2]. The 64-bit key is hashed over provided UTF-8 strings. These can be combined from user connection endpoint details and/or a shared key and session (uid, channel) values. This allows Mles server to verify that a connecting Mles client is really connecting from the endpoint where it claims to be connecting with a proper user and channel details. Mles client sessions behind Network Address Translation (NAT) MUST use a shared key without session endpoint detail authentication.

After Mles server has authenticated the session and moved the connected Mles client to its channel context, the SipHash key SHOULD be ignored by the Mles server. After context change, the SipHash key MAY be changed and used by Mles clients within the channel context.

A Mles server MAY contact to a Mles peer server. The Mles peer server sees this session as another Mles client session. This allows Mles servers to share and distribute value triplet data in an organized and powerful, yet simple manner between other Mles servers.

Every connection has also 32-bit connection id (CID) which SHOULD equal to the lowest 4 bytes of SipHash key. If SipHash key is not used as the value, the CID MUST still be configured in a deterministic manner. If a Mles server has a CID as part of an active connection on the same channel, it MUST drop further incoming connections with the same CID. This allows effectively autonomous loop protection in case peer servers are configured into a topology that creates a loop.

A Mles server MAY be configured to have a static CID value with a peer server configuration to create a load-balancing/protection group. All group members MUST have the same static CID configured. The group may also be a ring of servers.

A Mles server MAY save the history of received data, which can be then distributed to new clients when they connect to the Mles server. In case Mles server is restarted, Mles peer server, or even a Mles client, MAY provide history data for the Mles server as a resynchronization for those channels that it has the history available. This allows distributed data protection for the channel information.  

Mles clients and servers are independent of IP version and do not use IP broadcast or multicast. A Mles server MAY be configured to use IP anycast.

Mles clients MAY implement WebSocket [4] proxy which allows to do Mles connections over WebSocket protocol. Such simple example of proxy implementation is available in the reference client.

Mles protocol has Internet Assigned Number Authority (IANA) **port 8077** [3] registered for its use.

## Mles protocol details

The protocol header for Mles is as follows:
```
    0                   1                   2                   3
    0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
   | ASCII 'M'(77) |            Encapsulated data length           |
   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
   |            CID (lowest 4-bytes of initial SipHash)            |
   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
   |                                                               |
   +                          SipHash                              +
   |                                                               |
   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
   |         CBOR encapsulated data (uid, channel, message)        |
   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

CBOR encapsulated data is arranged as follows:
```
uid:     Major type 3, UTF-8 String
channel: Major type 3, UTF-8 String
message: Major type 2, byte string
```
With Rust the above looks as follows:
```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Msg {
    uid:     String,
    channel: String,
    #[serde(with = "serde_bytes")]
    message: Vec<u8>,
}
```

When endpoint details (IPv4 or IPv6 address) is used as part of the CID, the UTF8 string formats are as follows:

 * IPv4: ```a.b.c.d:port```
 * IPv6: ```[a:b::..::c:d]:port```


The ResyncMsg, which is used to share history information to Mles servers is in CBOR:
```
resync_message: Major type 2, byte string
```
On Rust it is defined as follows:
```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
struct MsgVec {
    #[serde(with = "serde_bytes")]
    encoded_msg: Vec<u8>, //this is encoded Msg
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResyncMsg {
    resync_message: Vec<MsgVec>,
}
```

## Mles WebSocket protocol details

A Mles client with WebSocket proxy allows connecting with WebSocket protocol [4] to a Mles proxy which will then forward frames to and from Mles network over the WebSocket protocol. A Mles WebSocket proxy MUST be able to transceive frames encapsulated as CBOR as defined in <b>Mles protocol details</b> section without Mles protocol header. In addition, the proxy SHOULD be able to transceive several channels multiplexed on the same connection over WebSocket. To enable proxy to add a new security layer on top of forwarded frames, the `message` part SHOULD always be initialized with 16-bytes of secure random data per `Msg` before the actual data payload. The Mles client with WebSocket proxy will handle the addition and removal of Mles protocol header. A Mles WebSocket client which connects to the proxy can be easily implemented on top of modern browsers with e.g. JavaScript and its existing CBOR libraries.

A Mles WebSocket client MUST set Sec-WebSocket-Protocol subprotocol to "mles-websocket" [5] to be able to establish a connection to a Mles client with WebSocket proxy successfully. 

## Usage

Reference binaries:
```
mles [peer-address] [--history-limit=N]
```

```
mles-client <server-address> [--use-websockets]
```

Shared key setting for Mles connections behind NAT:
```
MLES_KEY=mles-devel-frank mles [peer-address] [--history-limit=N]
```

```
MLES_KEY=mles-devel-frank mles-client <server-address> [--use-websockets]
```

To use `mles-utils` API, first add this to your `Cargo.toml`:

```toml
[dependencies]
mles-utils = "1.0"
```

Next, add this to your crate:

```rust
extern crate mles_utils;

use mles_utils::*;

fn main() {
    // ...
}
```

Client API example:
```rust
extern crate mles_utils;

use std::net::SocketAddr;
use std::thread;

use mles_utils::*;

fn main() {

    let child = thread::spawn(move || {
        //set server address to connect
        let addr = "127.0.0.1:8077".parse::<SocketAddr>().unwrap();
        //set channel
        let channel = "Channel".to_string();
        //set user
        let uid = "Receiver".to_string();    
        //connect client to server
        let mut conn = MsgConn::new(uid, channel);
        conn = conn.connect(addr);
        
        //blocking read for hello world
        let (conn, msg) = conn.read_message();
        let msg = String::from_utf8_lossy(msg.as_slice());
        assert_eq!("Hello World!", msg);
        println!("Just received: {}", msg);
        conn.close();
    });

    let addr = "127.0.0.1:8077".parse::<SocketAddr>().unwrap();
    let uid = "Sender".to_string();
    //set matching channel
    let channel = "Channel".to_string();
    //set message
    let message = "Hello World!".to_string();

    //send hello world to awaiting client
    let mut conn = MsgConn::new(uid, channel);
    conn = conn.connect_with_message(addr, message.into_bytes());
    conn.close();
    
    //wait receiver and return
    let _res = child.join();
}
```

## Existing client implementations over Mles (WebSocket) protocol

 * [Reference client](https://github.com/jq-rs/mles-rs/tree/master/mles-client) in Rust
 * [mles-webproxy](https://github.com/jq-rs/mles-webproxy) TLS and multichannel capable webproxy in Rust on Warp
 * [MlesTalk](https://mles.io/app.html) in JavaScript
 * <Please add your client here!>

## References:

 1. The Transport Layer Security (TLS) Protocol Version 1.3, https://tools.ietf.org/html/rfc8446
 2. Concise Binary Object Representation (CBOR), https://tools.ietf.org/html/rfc7049
 3. SipHash: a fast short-input PRF, https://131002.net/siphash/, referenced 4.2.2017
 4. IANA registered Mles port #8077, http://www.iana.org/assignments/service-names-port-numbers/service-names-port-numbers.xhtml?search=8077
 5. The WebSocket Protocol, https://tools.ietf.org/html/rfc6455
 6. IANA registered Mles WebSocket Subprotocol, https://www.iana.org/assignments/websocket
