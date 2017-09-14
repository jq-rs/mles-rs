# mles-rs [![Build Status](https://travis-ci.org/jq-rs/mles-rs.svg?branch=master)](https://travis-ci.org/jq-rs/mles-rs) [![Build Status](https://ci.appveyor.com/api/projects/status/github/jq-rs/mles-rs?svg=true)](https://ci.appveyor.com/api/projects/status/github/jq-rs/mles-rs?svg=true) [![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
**Mles-utils** [![crates.io status](https://img.shields.io/crates/v/mles-utils.svg)](https://crates.io/crates/mles-utils)
**Mles** [![crates.io status](https://img.shields.io/crates/v/mles.svg)](https://crates.io/crates/mles)
**Mles-client** [![crates.io status](https://img.shields.io/crates/v/mles-client.svg)](https://crates.io/crates/mles-client)

Mles (_Modern Lightweight channEl Service_) is a client-server data distribution protocol targeted to serve as a lightweight and reliable distributed publish/subscribe data service. The reference implementation consists of _Mles-utils_, _Mles_ server and _Mles-client/WebSocket-proxy_.

## Mles protocol overview

Mles clients connect to an Mles server using Mles protocol header and (uid, channel, message) value triplet on a TCP session where the triplet is Concise Binary Object Representation (CBOR) [1] encapsulated. An Mles client first subscribes to a channel by sending correct Mles protocol header and value triplet (uid, channel, msg) where _channel_ is the channel to publish/subscribe. The _msg_ MAY be either empty or include the first published message. The Mles server verifies Mles protocol header and then joins the Mles client to the selected channel. After joining, the Mles server distributes the value triplet between all clients on the same channel. If an Mles client wants to depart from a channel, it just closes the TCP session. If an Mles client does not want to receive any message, it just closes TCP session receive side.

An Mles server MAY save the history of received data, which can be then distributed to new clients when they connect to the Mles server. Every channel uses its own context and is independent of other channels: therefore a TCP session per (uid, channel) pair is always used.

Every session between Mles server and client is authenticated using 64-bit SipHash [2]. The 64-bit key is hashed over provided UTF-8 strings. These can be combined from user connection endpoint details and/or a shared key and session (uid, channel) values. This allows Mles server to verify that a connecting Mles client is really connecting from the endpoint where it claims to be connecting with proper user and channel details. Mles client sessions behind Network Address Translation (NAT) can use a shared key without session endpoint detail authentication.

After Mles server has authenticated the session and moved the connected Mles client to its channel context, the SipHash key SHOULD be ignored by the Mles server. After context change, the SipHash key MAY be changed and used by Mles clients within the channel context.

An Mles server MAY contact to an Mles peer server. The Mles peer server sees this session as another Mles client session. This allows Mles servers to share and distribute value triplet data in an organized and powerful, but yet simple manner between other Mles servers. 

Every connection has also 32-bit connection id (CID) which equals to the lowest 4 bytes of SipHash key. If a Mles server has a CID as part of an active connection, it MUST drop further incoming connections with the same CID. This allows effectively autonomous loop protection in case peer servers are configured into a topology that creates a loop.

In case Mles server is restarted, Mles peer server MAY provide history data for the Mles server as a resynchronization for those channels that is has the history available. This allows distributed data protection for the channel information.  

Mles clients and servers are independent of IP version and do not use IP broadcast or multicast. An Mles server MAY be configured to use IP anycast.

Mles clients MAY implement WebSocket [4] proxy which allows to do Mles connections over WebSocket protocol. Such simple example proxy implementation is available in the reference client.

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
## Usage

```
mles [peer-address] [--history-limit=N]
```

```
mles-client <server-address> [--use-websockets]
```
Server usage example:
```
MLES_KEY=validkey target/debug/mles
```

Client usage example:
```
MLES_KEY=validkey target/debug/mles-client 127.0.0.1
```

Client API usage example:
```
        //set server address to connect
        let addr = "127.0.0.1:8077".parse::<SocketAddr>().unwrap();
        let addr2 = addr.clone();
        //set users
        let uid = "User".to_string();
        let uid2 = "User two".to_string();
        //set channel
        let channel = "Channel".to_string();
        let channel2 = channel.clone();
        //set message
        let message = "Hello World!".to_string();

        let child = thread::spawn(move || {
            //connect client to server
            let mut conn = MsgConn::new(uid, channel);
            conn = conn.connect(addr);
        
            //blocking read for hello world
            let (conn, msg) = conn.read_message();
            let msg = String::from_utf8_lossy(msg.as_slice());
            assert_eq!("Hello World!", msg);
            conn.close();
        });
    
        //send hello world to awaiting client
        let mut conn = MsgConn::new(uid2, channel2);
        conn = conn.connect_with_message(addr2, message.into_bytes());
        conn.close();
```


## References:

 1. Concise Binary Object Representation (CBOR), https://tools.ietf.org/html/rfc7049
 2. SipHash: a fast short-input PRF, https://131002.net/siphash/, referenced 4.2.2017
 3. Mles registered port #8077, http://www.iana.org/assignments/service-names-port-numbers/service-names-port-numbers.xhtml?search=8077
 4. The WebSocket Protocol, https://tools.ietf.org/html/rfc6455
