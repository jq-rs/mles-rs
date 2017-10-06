#![warn(missing_docs)]
//! `Mles utils` library is provided for Mles client and server implementations for easy handling of 
//! proper header and message structures.

/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/. 
*
*  Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
* */

#[macro_use]
extern crate serde_derive;
extern crate serde_cbor;
extern crate serde_bytes;
extern crate byteorder;
extern crate siphasher;
extern crate tokio_core;
extern crate tokio_io;
extern crate futures;

mod server;
mod local_db;
mod frame;
mod peer;

use std::io::Cursor;
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};
use siphasher::sip::SipHasher;
use std::hash::{Hash, Hasher};
use std::net::TcpStream;
use std::io::Write;
use std::io::{Read, Error};

use std::net::{IpAddr, SocketAddr};

/// HDRL defines the size of the header including version, length and timestamp
pub(crate) const HDRL: usize = 8; 
/// CIDL defines the size of the connection id
pub(crate) const CIDL:  usize = 4; 
/// KEYL defines the size of the key
pub(crate) const KEYL: usize = 8; 
/// HDRKEYL defines the size of the header + key
pub(crate) const HDRKEYL: usize = HDRL + KEYL;

/// Max message size
pub(crate) const MSGMAXSIZE: usize = 0xffffff;

const KEEPALIVE: u64 = 5;

/// MsgHdr structure
///
/// This structure defines the header of the Mles message including first 'M' byte,
/// length of the encoded data, connection id and SipHash key.
/// Encoded message will always be in network byte order.
///
pub struct MsgHdr {
    thlen:  u32,
    cid:    u32,
    key:    u64,
}

impl MsgHdr {
    /// Create a new MsgHdr object with length, cid and key.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let msghdr = MsgHdr::new(len, cid, key);
    /// ```
    pub fn new(len: u32, cid: u32, key: u64) -> MsgHdr {
        MsgHdr {
            thlen: hdr_set_len(len),
            cid: cid,
            key: key, 
        }
    }

    /// Get type of MsgHdr.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// msghdr.get_type();
    /// assert_eq!('M' as u8, msghdr.get_type());
    /// ```
    pub fn get_type(&self) -> u8 {
        hdr_get_type(self.thlen)
    }

    /// Get MsgHdr length on the line.
    /// 
    pub fn get_hdrkey_len() -> usize {
        HDRKEYL
    }

    /// Set length of MsgHdr.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// msghdr.set_len(515);
    /// ```
    pub fn set_len(&mut self, len: u32) {
        self.thlen = hdr_set_len(len);
    }

    /// Get length of MsgHdr.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// msghdr.set_len(515);
    /// assert_eq!(515, msghdr.get_len());
    /// ```
    pub fn get_len(&self) -> u32 {
        hdr_get_len(self.thlen)
    }

    /// Set cid of MsgHdr.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// msghdr.set_cid(515);
    /// ```
    pub fn set_cid(&mut self, cid: u32) {
        self.cid = cid;
    }

    /// Get cid of MsgHdr.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// msghdr.set_cid(515);
    /// assert_eq!(515, msghdr.get_cid());
    /// ```
    pub fn get_cid(&self) -> u32 {
        self.cid
    }

    /// Set key of MsgHdr.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// msghdr.set_key(515);
    /// ```
    pub fn set_key(&mut self, key: u64) {
        self.key = key;
    }

    /// Get key of MsgHdr.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// msghdr.set_key(515);
    /// assert_eq!(515, msghdr.get_key());
    /// ```
    pub fn get_key(&self) -> u64 {
        self.key
    }

    /// Encode MsgHdr to line format.
    ///
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 0;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// let msgv: Vec<u8> = msghdr.encode();
    /// ```
    pub fn encode(&self) -> Vec<u8> {
        let mut msgv = write_hdr(self.get_len() as usize, self.get_cid());
        msgv.extend(write_key(self.get_key()));
        msgv
    }

    /// Decode MsgHdr from line format.
    ///
    ///
    /// # Example
    /// ```
    /// use mles_utils::{MsgHdr};
    ///
    /// let key = 0xf00f;
    /// let cid = MsgHdr::select_cid(key); 
    /// let len = 16;
    ///
    /// let mut msghdr = MsgHdr::new(len, cid, key);
    /// let msgv: Vec<u8> = msghdr.encode();
    /// let msgh = MsgHdr::decode(msgv);
    /// assert_eq!(key, msgh.get_key());
    /// assert_eq!(cid, msgh.get_cid());
    /// assert_eq!(len, msgh.get_len());
    /// ```
    pub fn decode(buf: Vec<u8>) -> MsgHdr {
        MsgHdr::new(read_hdr_len(&buf) as u32, read_cid_from_hdr(&buf), read_key_from_hdr(&buf))
    }
    /// Do a valid hash for Mles over provided UTF-8 String list.
    ///
    /// # Example
    /// ```
    /// use mles_utils::MsgHdr;
    ///
    /// let hashstr1 = "A string".to_string();
    /// let hashstr2 = "Another string".to_string();
    /// let hashable = vec![hashstr1, hashstr2];
    /// let key: u64 = MsgHdr::do_hash(&hashable); 
    /// ```
#[inline]
    pub fn do_hash(t: &[String]) -> u64 {
        let mut s = SipHasher::new();
        for item in t {
            item.hash(&mut s);
        }
        s.finish()
    }

    /// Return a connection id from key.
    ///
    /// # Example
    /// ```
    /// use mles_utils::MsgHdr;
    ///
    /// let cid = MsgHdr::select_cid(0x1000000100000001);
    /// assert_eq!(cid, 0x00000001);
    /// ```
#[inline]
    pub fn select_cid(key: u64) -> u32 {
        key as u32 
    }

    /// Do a valid UTF-8 string from a `SocketAddr`.
    ///
    /// For IPv4 the format is "x.x.x.x:y", where x is u8 and y is u16
    /// For IPv6 the format is "[z:z:z:z:z:z:z:z]:y", where z is u16 in hexadecimal format and y is u16
    ///
    /// # Example
    /// ```
    ///
    /// use std::net::{SocketAddr, IpAddr, Ipv4Addr, Ipv6Addr};
    /// use mles_utils::MsgHdr;
    ///
    /// let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    /// let addrstr = MsgHdr::addr2str(&addr);
    ///
    /// assert_eq!("127.0.0.1:8080", addrstr);
    ///
    /// let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0xff03, 0, 0, 0, 0, 0, 0, 1)), 8077);
    /// let addrstr = MsgHdr::addr2str(&addr);
    ///
    /// assert_eq!("[ff03:0:0:0:0:0:0:1]:8077", addrstr);
    /// ```
#[inline]
    pub fn addr2str(addr: &SocketAddr) -> String {
        let ipaddr = addr.ip();
        match ipaddr {
            IpAddr::V4(v4) => {
                let v4oct = v4.octets();
                let v4str = format!("{}.{}.{}.{}:{}", 
                                    v4oct[0], v4oct[1], v4oct[2], v4oct[3], 
                                    addr.port());
                v4str
            }
            IpAddr::V6(v6) => {
                let v6seg = v6.segments();
                let v6str = format!("[{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}]:{}", 
                                    v6seg[0], v6seg[1], v6seg[2], v6seg[3], 
                                    v6seg[4], v6seg[5], v6seg[6], v6seg[7], 
                                    addr.port());
                v6str
            }
        }
    }
}

fn hdr_set_len(len: u32) -> u32 {
    77 << 24 | len & 0xffffff
}

fn hdr_get_len(thlen: u32) -> u32 {
    thlen & 0xffffff
}

fn hdr_get_type(thlen: u32) -> u8 {
    (thlen >> 24) as u8 
}


/// Msg structure
///
/// This structure defines the Mles interface value triplet (uid, channel, message). 
/// It is eventually serialized and deserialized by CBOR.
///
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Msg {
    uid:     String,
    channel: String,
    #[serde(with = "serde_bytes")]
    message: Vec<u8>,
}

impl Msg {
    /// Create a new Msg object with value triplet.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// ```
#[inline]
    pub fn new(uid: String, channel: String, message: Vec<u8>) -> Msg {
        Msg {
            uid: uid,
            channel: channel,
            message: message
        }
    }

    /// Set uid for Msg object.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let mut msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg = msg.set_uid("New uid".to_string());
    ///
    /// assert_eq!("New uid".to_string(), *msg.get_uid());
    /// ```
#[inline]
    pub fn set_uid(mut self, uid: String) -> Msg {
        self.uid = uid;
        self
    }

    /// Set channel for Msg object.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let mut msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg = msg.set_channel("New channel".to_string());
    ///
    /// assert_eq!("New channel".to_string(), *msg.get_channel());
    /// ```
#[inline]
    pub fn set_channel(mut self, channel: String) -> Msg {
        self.channel = channel;
        self
    }

    /// Set message for Msg object.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let mut msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let new_message: Vec<u8> = "New message".to_string().into_bytes();
    /// let msg = msg.set_message(new_message);
    /// ```
#[inline]
    pub fn set_message(mut self, message: Vec<u8>) -> Msg {
        self.message = message;
        self
    }

    /// Get uid for Msg object. See example for set uid.
#[inline]
    pub fn get_uid(&self) -> &String {
        &self.uid
    }

    /// Get channel for Msg object. See example for set channel.
#[inline]
    pub fn get_channel(&self) -> &String {
        &self.channel
    }

    /// Get message for Msg object.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let mut msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg: &Vec<u8> = msg.get_message();
    /// ```
#[inline]
    pub fn get_message(&self) -> &Vec<u8> {
        &self.message
    }

    /// Get message len for Msg object.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let mut msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg_len: usize = msg.get_message_len();
    /// ```
#[inline]
    pub fn get_message_len(&self) -> usize {
        self.message.len()
    }

    /// Encode Msg object to CBOR.
    ///
    /// # Errors
    /// If message cannot be encoded, an empty vector is returned.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let encoded_msg: Vec<u8> = msg.encode();
    /// ```
#[inline]
    pub fn encode(&self) -> Vec<u8> {
        let encoded = serde_cbor::to_vec(self);
        match encoded {
            Ok(encoded) => encoded,
                Err(err) => {
                    println!("Error on encode: {}", err);
                    Vec::new()
                }
        }
    }

    /// Decode CBOR byte string to Msg object.
    ///
    /// # Errors
    /// If message cannot be decoded, a Msg structure with empty items is returned.
    ///
    /// # Example
    /// ```
    /// use mles_utils::Msg;
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let encoded_msg: Vec<u8> = msg.encode();
    /// let decoded_msg: Msg = Msg::decode(&encoded_msg);
    /// ```
#[inline]
    pub fn decode(slice: &[u8]) -> Msg {
        let value = serde_cbor::from_slice(slice);
        match value {
            Ok(value) => { 
                value
            },
            Err(err) => {
                println!("Error on decode: {}", err);
                Msg { uid: "".to_string(), channel: "".to_string(), message: Vec::new() } // return empty vec in case of error
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct MsgVec {
    #[serde(with = "serde_bytes")]
    messages: Vec<u8>,
}

impl MsgVec {
    pub fn new(messages: &Vec<u8>) -> MsgVec {
        MsgVec {
            messages: messages.clone(),
        }
    }

    pub fn get(&self) -> &Vec<u8> {
        &self.messages
    }
}

/// ResyncMsg structure
///
/// This structure defines resynchronization Msg structure that can be used 
/// to resynchronize history state to root server from peers. The resynchronization 
/// message can be sent only during initial connection message and packs the 
/// history into one message that can be taken into account by Mles root server. 
///
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResyncMsg {
    resync_message: Vec<MsgVec>,
}


impl ResyncMsg {
    /// Create a new ResyncMsg object with encoded message vector.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{Msg, ResyncMsg};
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg = msg.encode();
    /// let vec = vec![msg];
    /// let rmsg = ResyncMsg::new(&vec);
    /// ```
#[inline]
    pub fn new(messages: &Vec<Vec<u8>>) -> ResyncMsg {
        let mut rmsg = ResyncMsg {
            resync_message: Vec::new(),
        };
        //transform to correct format 
        for msg in messages {
            rmsg.resync_message.push(MsgVec::new(&msg));
        }
        rmsg
    }

    /// Get the length of the resync message vector
    ///
    /// # Example
    /// ```
    /// use mles_utils::{Msg, ResyncMsg};
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg = msg.encode();
    /// let vec = vec![msg];
    /// let rmsg = ResyncMsg::new(&vec);
    /// assert_eq!(1, rmsg.len());
    /// ```
#[inline]
    pub fn len(&self) -> usize {
        self.resync_message.len()
    }

    /// Get all items of the resync message vector
    ///
    /// # Example
    /// ```
    /// use mles_utils::{Msg, ResyncMsg};
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg = msg.encode();
    /// let vec = vec![msg];
    /// let rmsg = ResyncMsg::new(&vec);
    /// let rvec = rmsg.get_messages();
    /// assert_eq!(vec[0], rvec[0]);
    /// ```
#[inline]
    pub fn get_messages(&self) -> Vec<Vec<u8>> {
        //transform to correct format 
        let mut messages = Vec::new();
        for msg in self.resync_message.iter() {
            let msg = msg.get();
            messages.push(msg.clone());
        }
        messages
    }

    /// Encode ResyncMsg object to CBOR.
    ///
    /// # Errors
    /// If resync message cannot be encoded, an empty vector is returned.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{ResyncMsg, Msg};
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg = msg.encode();
    /// let vec = vec![msg];
    /// let rmsg = ResyncMsg::new(&vec);
    /// let encoded_msg: Vec<u8> = rmsg.encode();
    /// ```
#[inline]
    pub fn encode(&self) -> Vec<u8> {
        let encoded = serde_cbor::to_vec(self);
        match encoded {
            Ok(encoded) => encoded,
                Err(err) => {
                    println!("Error on resync encode: {}", err);
                    Vec::new()
                }
        }
    }

    /// Decode CBOR byte string to ResyncMsg object.
    ///
    /// # Errors
    /// If message cannot be decoded, a ResyncMsg structure with empty items is returned.
    ///
    /// # Example
    /// ```
    /// use mles_utils::{ResyncMsg, Msg};
    ///
    /// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
    /// let msg = msg.encode();
    /// let vec = vec![msg];
    /// let rmsg = ResyncMsg::new(&vec);
    /// let encoded_msg: Vec<u8> = rmsg.encode();
    /// let decoded_msg: ResyncMsg = ResyncMsg::decode(&encoded_msg);
    /// assert_eq!(1, decoded_msg.len());
    /// ```
#[inline]
    pub fn decode(slice: &[u8]) -> ResyncMsg {
        let value = serde_cbor::from_slice(slice);
        match value {
            Ok(value) => value,
                Err(_) => {
                    ResyncMsg { resync_message: Vec::new() } // return empty vec in case of error
                }
        }
    }
}

/// Msg connection structure
///
/// This structure defines the Mles connection for simple synchronous connections. 
///
pub struct MsgConn {
    uid:     String,
    channel: String,
    key:     Option<u64>,
    stream:  Option<TcpStream>,
}

impl MsgConn {

    /// Create a new MsgConn object for a connection.
    ///
    /// # Example
    /// ```
    /// use mles_utils::MsgConn;
    ///
    /// let conn = MsgConn::new("My uid".to_string(), "My channel".to_string());
    /// ```
#[inline]
    pub fn new(uid: String, channel: String) -> MsgConn {
        MsgConn {
            uid: uid,
            channel: channel,
            key: None,
            stream: None
        }
    }

    /// Gets the defined uid.
    ///
    /// # Example
    /// ```
    /// use mles_utils::MsgConn;
    ///
    /// let conn = MsgConn::new("My uid".to_string(), "My channel".to_string());
    /// assert_eq!("My uid".to_string(), conn.get_uid());
    /// ```
#[inline]
    pub fn get_uid(&self) -> String {
        self.uid.clone()
    }

    /// Gets the defined channel.
    ///
    /// # Example
    /// ```
    /// use mles_utils::MsgConn;
    ///
    /// let conn = MsgConn::new("My uid".to_string(), "My channel".to_string());
    /// assert_eq!("My channel".to_string(), conn.get_channel());
    /// ```
#[inline]
    pub fn get_channel(&self) -> String {
        self.channel.clone()
    }

    /// Gets the defined key.
    ///
    /// # Example
    /// ```
    /// use mles_utils::MsgConn;
    ///
    /// //key is set only when connection is initiated..
    /// let conn = MsgConn::new("My uid".to_string(), "My channel".to_string());
    /// assert_eq!(true, conn.get_key().is_none());
    /// ```
#[inline]
    pub fn get_key(&self) -> Option<u64> {
        self.key
    }

    /// Connects to the defined address with a message.
    /// 
#[inline]
    pub fn connect_with_message(mut self, raddr: SocketAddr, msg: Vec<u8>) -> MsgConn {
        let msg = Msg::new(self.get_uid(), self.get_channel(), msg);
        match TcpStream::connect(raddr) {
            Ok(mut stream) => {
                let _val = stream.set_nodelay(true);

                if self.get_key().is_none() {
                    let mut keys = Vec::new();

                    let laddr = match stream.local_addr() {
                        Ok(laddr) => laddr,
                            Err(_) => {
                                let addr = "0.0.0.0:0";
                                addr.parse::<SocketAddr>().unwrap()
                            }
                    };
                    keys.push(MsgHdr::addr2str(&laddr));
                    keys.push(self.get_uid());
                    keys.push(self.get_channel());
                    let key = MsgHdr::do_hash(&keys);
                    self.key = Some(key);
                }
                let encoded_msg = msg.encode();
                let key = self.get_key().unwrap();
                let keyv = write_key(key);
                let mut msgv = write_hdr(encoded_msg.len(), MsgHdr::select_cid(key));
                msgv.extend(keyv);
                msgv.extend(encoded_msg);
                stream.write(msgv.as_slice()).unwrap();
                self.stream = Some(stream);
                self
            },
            Err(_) => {
                println!("Could not connect to server {}", raddr);
                self
            },
        }
    }

    /// Connects to the defined address (without a message).
    /// 
#[inline]
    pub fn connect(self, raddr: SocketAddr) -> MsgConn {
        self.connect_with_message(raddr, Vec::new())
    }

    /// Send a message. Blocks until a message is sent.
    /// 
    /// # Errors
    /// If a message cannot be sent, stream is set to None.
    /// 
#[inline]
    pub fn send_message(mut self, msg: Vec<u8>) -> MsgConn {
        let message = Msg::new(self.get_uid(), self.get_channel(), msg);
        let encoded_msg = message.encode();
        let key = self.get_key().unwrap();
        let keyv = write_key(key);
        let mut msgv = write_hdr(encoded_msg.len(), MsgHdr::select_cid(key));
        msgv.extend(keyv);
        msgv.extend(encoded_msg);
        let mut stream = self.stream.unwrap();
        match stream.write(msgv.as_slice()) {
            Ok(0) => { 
                println!("Send zero");
                self.stream = None;
            },
            Ok(_) => self.stream = Some(stream),
            Err(err) => { 
                println!("Send error {}", err);
                self.stream = None;
            }
        }
        self
    }

    /// Reads a message with non-zero message content. Blocks until a message is received.
    /// 
    /// # Errors
    /// If message cannot be read, an empty message is returned.
    /// 
#[inline]
    pub fn read_message(mut self) -> (MsgConn, Vec<u8>) {
        let stream = self.stream.unwrap();
        loop {
            let tuple = read_n(&stream, HDRKEYL);
            let status = tuple.0;
            match status {
                Ok(0) => {
                    println!("Read failed: eof");
                    self.stream = None;
                    return (self, Vec::new());
                },
                _ => {}
            }
            let buf = tuple.1;
            if 0 == buf.len() {
                continue;
            }
            if read_hdr_type(buf.as_slice()) != 'M' as u32 {
                continue;
            }
            let hdr_len = read_hdr_len(buf.as_slice());
            if 0 == hdr_len {
                continue;
            }
            let tuple = read_n(&stream, hdr_len);
            let status = tuple.0;
            match status {
                Ok(0) => continue,
                _ =>  {}
            }
            let payload = tuple.1;
            if payload.len() != (hdr_len as usize) {
                continue;
            }
            let decoded_message = Msg::decode(payload.as_slice());
            if 0 == decoded_message.get_message_len() {
                continue;
            }
            self.stream = Some(stream);
            return (self, decoded_message.get_message().to_owned());
        }
    }

    /// Closes the connection.
    /// 
#[inline]
    pub fn close(mut self) -> MsgConn {
        if self.stream.is_some() {
            drop(self.stream.unwrap());
        }
        self.stream = None;
        self
    }

}

#[inline]
pub(crate) fn read_hdr_type(hdr: &[u8]) -> u32 {
    if hdr.len() < HDRL {
        return 0;
    }
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    num >> 24
}

fn read_hdr_len(hdr: &[u8]) -> usize { 
    if hdr.len() < HDRL {
        return 0;
    }
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    (num & 0xfff) as usize
}

fn write_hdr(len: usize, cid: u32) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let mut msgv = vec![];
    let mut cidv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    cidv.write_u32::<BigEndian>(cid).unwrap();
    msgv.extend(cidv);
    msgv
}

fn write_hdr_without_cid(len: usize) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let mut msgv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    msgv
}


#[inline]
pub(crate) fn write_len_to_hdr(len: usize, mut hdrv: Vec<u8>) -> Vec<u8> {
    if hdrv.len() < HDRL {
        return vec![];
    }
    let tail = hdrv.split_off(HDRL-CIDL);
    let mut nhdrv = write_hdr_without_cid(len);
    nhdrv.extend(tail);
    nhdrv
}

fn write_key(val: u64) -> Vec<u8> {
    let key = val;
    let mut msgv = vec![];
    msgv.write_u64::<BigEndian>(key).unwrap();
    msgv
}

fn write_hdr_with_key(len: usize, key: u64) -> Vec<u8> {
    let mut hdrv = write_hdr(len, MsgHdr::select_cid(key));
    hdrv.extend(write_key(key));
    hdrv
}


fn read_key_from_hdr(keyv: &[u8]) -> u64 {
    if keyv.len() < HDRKEYL {
        return 0;
    }
    let mut buf = Cursor::new(&keyv[HDRL..]);
    buf.read_u64::<BigEndian>().unwrap()
}

fn read_cid_from_hdr(hdrv: &[u8]) -> u32 {
    if hdrv.len() < HDRL {
        return 0;
    }
    let mut buf = Cursor::new(&hdrv[(HDRL-CIDL)..]);
    buf.read_u32::<BigEndian>().unwrap()
}

/// Check if a peer is defined
///
/// # Example
/// ```
/// use mles_utils::has_peer;
///
/// let sockaddr = None;
/// assert_eq!(false, has_peer(&sockaddr));
/// ```
#[inline]
pub fn has_peer(peer: &Option<SocketAddr>) -> bool {
    peer::has_peer(peer)
}

fn read_n<R>(reader: R, bytes_to_read: usize) -> (Result<usize, Error>, Vec<u8>)
where R: Read,
{
    let mut buf = vec![];
    let mut chunk = reader.take(bytes_to_read as u64);
    let status = chunk.read_to_end(&mut buf);
    (status, buf)
}

/// Run an Mles server
///
/// # Example
/// ```
/// use std::thread;
/// use std::net::{IpAddr, Ipv4Addr};
/// use std::net::{SocketAddr, ToSocketAddrs};
/// use mles_utils::server_run;
///
/// let uid = "User".to_string();
/// let channel = "Channel".to_string();
/// let message = "Hello World!".to_string();
/// let address = "127.0.0.1:8077".to_string();
/// let address = address.parse().unwrap();
/// let child = thread::spawn(move || server_run(address, None, "".to_string(), "".to_string(), 100, 0));
/// drop(child);
/// ```
#[inline]
pub fn server_run(address: SocketAddr, peer: Option<SocketAddr>, keyval: String, keyaddr: String,  hist_limit: usize, debug_flags: u64) {
    server::run(address, peer, keyval, keyaddr, hist_limit, debug_flags);
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::thread;
    use std::time::Duration;
    use super::*;

    #[test]
    fn test_encode_decode_msg() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let msg =  "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        let decoded_msg = Msg::decode(&encoded_msg);
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
    }

    #[test]
    fn test_encode_decode_resync_msg() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let msg =  "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        let uid2 = "User two".to_string();
        let channel2 = "Channel two".to_string();
        let msg2 =  "a test msg two".to_string().into_bytes();
        let orig_msg2 = Msg::new(uid2, channel2, msg2);
        let encoded_msg2 = orig_msg2.encode();
        let vec = vec![encoded_msg, encoded_msg2];
        let rmsg = ResyncMsg::new(&vec);
        let encoded_resync_msg: Vec<u8> = rmsg.encode();
        let decoded_resync_msg: ResyncMsg = ResyncMsg::decode(&encoded_resync_msg);
        let mut cnt = 0;
        for msg in decoded_resync_msg.get_messages() {
            let decoded_msg = Msg::decode(&msg);
            if 0 == cnt {
                assert_eq!(decoded_msg.uid, orig_msg.uid);
                assert_eq!(decoded_msg.channel, orig_msg.channel);
                assert_eq!(decoded_msg.message, orig_msg.message);
            }
            else {
                assert_eq!(decoded_msg.uid, orig_msg2.uid);
                assert_eq!(decoded_msg.channel, orig_msg2.channel);
                assert_eq!(decoded_msg.message, orig_msg2.message);
            }
            cnt += 1;
        }
    }

    #[test]
    fn test_set_get_msg() {
        let uid =  "User".to_string();
        let channel =  "Channel".to_string();
        let msg =  "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new("".to_string(), channel.to_string(), Vec::new());
        let orig_msg = orig_msg.set_uid(uid.clone());
        let orig_msg = orig_msg.set_channel(channel.clone());
        let orig_msg = orig_msg.set_message(msg.clone());
        assert_eq!(&uid, orig_msg.get_uid());
        assert_eq!(&channel, orig_msg.get_channel());
        assert_eq!(&msg, orig_msg.get_message());
    }

    #[test]
    fn test_cid() {
        let orig_key = 0xffeffe;
        let hdrv = write_hdr_with_key(64, orig_key);
        let orig_len = hdrv.len();
        let key = read_key_from_hdr(&hdrv);
        assert_eq!(orig_key, key);
        let read_cid = read_cid_from_hdr(&hdrv);
        assert_eq!(orig_key as u32, read_cid);
        let key = read_key_from_hdr(&hdrv);
        assert_eq!(orig_key, key);
        let len = hdrv.len();
        assert_eq!(orig_len, len);
    }

    #[test]
    fn test_msgconn_send_read() {
        let sec = Duration::new(1,0);
        let addr = "127.0.0.1:8077";
        let addr = addr.parse::<SocketAddr>().unwrap();
        let raddr = addr.clone();
        let uid = "User".to_string();
        let uid2 = "User two".to_string();
        let channel = "Channel".to_string();
        let message = "Hello World!".to_string();
         
        //create server
        let child = thread::spawn(move || server_run(addr, None, "".to_string(), "".to_string(), 100, 0));
        thread::sleep(sec);

        //send hello world
        let mut conn = MsgConn::new(uid2.clone(), channel.clone());
        conn = conn.connect_with_message(raddr, message.into_bytes());
        conn.close();

        //read hello world
        let mut conn = MsgConn::new(uid.clone(), channel.clone());
        conn = conn.connect(raddr);
        let (conn, msg) = conn.read_message();
        let msg = String::from_utf8_lossy(msg.as_slice());
        assert_eq!("Hello World!", msg);

        //close connection
        conn.close();

        //drop server
        drop(child);
    }

    #[test]
    fn test_msgconn_read_send() {
        let sec = Duration::new(1,0);
        let addr = "127.0.0.1:8076";
        let addr = addr.parse::<SocketAddr>().unwrap();
        let raddr = addr.clone();
        let uid = "User".to_string();
        let uid2 = "User two".to_string();
        let channel = "Channel".to_string();
        let message = "Hello World!".to_string();
         
        //create server
        let child = thread::spawn(move || server_run(addr, None, "".to_string(), "".to_string(), 100, 0));
        thread::sleep(sec);

        //read connect
        let mut conn = MsgConn::new(uid.clone(), channel.clone());
        conn = conn.connect(raddr);

        //send hello world
        let mut sconn = MsgConn::new(uid2.clone(), channel.clone());
        sconn = sconn.connect_with_message(raddr, message.into_bytes());
        sconn.close();

        //read hello world
        let (conn, msg) = conn.read_message();
        let msg = String::from_utf8_lossy(msg.as_slice());
        assert_eq!("Hello World!", msg);

        //close connection
        conn.close();

        //drop server
        drop(child);
    }

    #[test]
    fn test_msgconn_peer_send_read() {
        let sec = Duration::new(1,0);
        let addr = "127.0.0.1:8075";
        let addr = addr.parse::<SocketAddr>().unwrap();
        let paddr = "127.0.0.1:8074";
        let paddr = paddr.parse::<SocketAddr>().unwrap();
        let praddr = paddr.clone();
        let uid = "User".to_string();
        let uid2 = "User two".to_string();
        let channel = "Channel".to_string();
        let message = "Hello World!".to_string();
         
        //create server
        let child = thread::spawn(move || server_run(addr, None, "".to_string(), "".to_string(), 100, 0));
        thread::sleep(sec);

        //create peer server
        let pchild = thread::spawn(move || server_run(paddr, Some(addr), "".to_string(), "".to_string(), 100, 0));
        thread::sleep(sec);

        //send hello world
        let mut conn = MsgConn::new(uid.clone(), channel.clone());
        conn = conn.connect_with_message(praddr, message.into_bytes());
        conn.close();

        //read hello world
        let mut conn = MsgConn::new(uid2.clone(), channel.clone());
        conn = conn.connect(praddr);
        let (conn, msg) = conn.read_message();
        let msg = String::from_utf8_lossy(msg.as_slice());
        assert_eq!("Hello World!", msg);

        //close connection
        conn.close();

        //drop peer server
        drop(pchild);

        //drop server
        drop(child);
    }

    #[test]
    fn test_msgconn_peer_read_send() {
        let sec = Duration::new(1,0);
        let addr = "127.0.0.1:8073";
        let addr = addr.parse::<SocketAddr>().unwrap();
        let paddr = "127.0.0.1:8072";
        let paddr = paddr.parse::<SocketAddr>().unwrap();
        let praddr = paddr.clone();
        let uid = "User".to_string();
        let uid2 = "User two".to_string();
        let channel = "Channel".to_string();
        let message = "Hello World!".to_string();
         
        //create server
        let child = thread::spawn(move || server_run(addr, None, "".to_string(), "".to_string(), 100, 0));
        thread::sleep(sec);

        //create peer server
        let pchild = thread::spawn(move || server_run(paddr, Some(addr), "".to_string(), "".to_string(), 100, 0));
        thread::sleep(sec);

        //read connect
        let mut conn = MsgConn::new(uid.clone(), channel.clone());
        conn = conn.connect(praddr);

        //send hello world
        let mut sconn = MsgConn::new(uid2.clone(), channel.clone());
        sconn = sconn.connect_with_message(praddr, message.into_bytes());
        sconn.close();

        //read hello world
        let (conn, msg) = conn.read_message();
        let msg = String::from_utf8_lossy(msg.as_slice());
        assert_eq!("Hello World!", msg);

        //close connection
        conn.close();

        //drop peer server
        drop(pchild);

        //drop server
        drop(child);
    }

    #[test]
    fn test_msgconn_basic_read_send() {
        let sec = Duration::new(1,0);
        //set server address to connect
        let addr = "127.0.0.1:8071".parse::<SocketAddr>().unwrap();
        //create server
        let serv = thread::spawn(move || server_run(addr, None, "".to_string(), "".to_string(), 0, 0));
        thread::sleep(sec);
         
        let child = thread::spawn(|| {
            let uid = "User two".to_string();
            let channel = "Channel".to_string();
            let addr = "127.0.0.1:8071".parse::<SocketAddr>().unwrap();
            //connect client to server
            let mut conn = MsgConn::new(uid, channel);
            conn = conn.connect(addr);

            //blocking read for hello world
            let (conn, msg) = conn.read_message();
            let msg = String::from_utf8_lossy(msg.as_slice());
            assert_eq!("Hello World!", msg);
            conn.close();
        });
        thread::sleep(sec);

        let addr = "127.0.0.1:8071".parse::<SocketAddr>().unwrap();
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let message = "Hello World!".to_string();
    
        //send hello world to awaiting client
        let mut conn = MsgConn::new(uid, channel);
        conn = conn.connect_with_message(addr, message.into_bytes());
        conn.close();

        let _res = child.join();

        drop(serv);
    }
}



