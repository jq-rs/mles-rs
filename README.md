# mles-rs [![Build Status](https://travis-ci.org/jq-rs/mles-rs.svg?branch=master)](https://travis-ci.org/jq-rs/mles-rs) [![Build Status](https://ci.appveyor.com/api/projects/status/github/jq-rs/mles-rs?svg=true)](https://ci.appveyor.com/api/projects/status/github/jq-rs/mles-rs?svg=true)
**Mles-utils** [![crates.io status](https://img.shields.io/crates/v/mles-utils.svg)](https://crates.io/crates/mles-utils)
**Mles** [![crates.io status](https://img.shields.io/crates/v/mles.svg)](https://crates.io/crates/mles)
**Mles-client** [![crates.io status](https://img.shields.io/crates/v/mles-client.svg)](https://crates.io/crates/mles-client)

Mles is a client-server data distribution protocol targeted to serve as a lightweight and reliable distributed publish/subscribe database service. It consists of _Mles-utils_, _Mles-client_ and _Mles_ server implementations.

## Mles protocol overview

Mles clients connect to an Mles server using Mles protocol header and (uid, channel, message) value triple on a TCP session where the triple is Concise Binary Object Representation (CBOR) [1] encapsulated. An Mles client first subscribes to a channel by sending correct Mles protocol header and value triple (uid, channel, msg) where _channel_ is the channel to publish/subscribe. The _msg_ should be empty. The Mles server verifies Mles protocol header and then joins the Mles client to the selected channel. After joining, the Mles server distributes the value triple between all clients on the same channel. If an Mles client wants to depart from a channel, it just closes the TCP session.

An Mles server may save the history of received data, which can be then distributed to new clients when they connect to the Mles server. Every channel uses its own context and is independent of other channels; therefore a TCP session per channel is always used.

Every session between Mles server and client is authenticated using 64-bit SipHash [2]. This allows Mles server to verify that connecting Mles client is really connecting from the endpoint where it claims to be connecting. Additionally, a shared secret key may be used as part of SipHash calculation between Mles server and Mles client. The shared key allows only those Mles clients to connect to Mles server which know the shared key. Mles client sessions behind Network Address Translation (NAT) may use a shared key without session authentication. After Mles server has authenticated the session and moved the connected Mles client to its channel context, SipHash key should be ignored by the Mles server. After context change, SipHash key may be changed and used by Mles clients within the channel context.

An Mles server may contact to an Mles peer server. The Mles peer server sees this session as another Mles client session. This allows Mles servers to share and distribute value triple data in an organized and powerful, but yet simple manner between other Mles servers.

Mles clients and servers are independent of IP version and do not use IP broadcast or multicast. An Mles server may be configured to use IP anycast.

Mles protocol has Internet Assigned Number Authority (IANA) **port 8077** [3] registered for its use.

## Mles protocol details

The protocol header for Mles is as follows:
```
    0                   1                   2                   3
    0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
   | ASCII 'M'(77) |            Encapsulated data length           |
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
message: Major type 2, bytestring
```
With Rust the above looks as follows:
```
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Msg {
    pub uid:     String,
    pub channel: String,
    pub message: Vec<u8>,
}
```
## References:

 1. Concise Binary Object Representation (CBOR), https://tools.ietf.org/html/rfc7049
 2. SipHash: a fast short-input PRF, https://131002.net/siphash/, referenced 4.2.2017
 3. Mles registered port #8077, http://www.iana.org/assignments/service-names-port-numbers/service-names-port-numbers.xhtml?search=8077
 

