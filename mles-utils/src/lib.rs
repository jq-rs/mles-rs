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
extern crate rand;
extern crate tokio_core;
extern crate tokio_io;
extern crate futures;

mod local_db;
mod frame;

/// Peer module provides peer-related public functions
pub mod peer;

use std::io::Cursor;
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};
use siphasher::sip::SipHasher;
use std::hash::{Hash, Hasher};
use std::net::TcpStream;
use std::io::Write;
use std::io::{Read, Error};

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::io::{ErrorKind};
use std::process;
use std::net::{IpAddr, SocketAddr};
use std::thread;
use std::time::Duration;

use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;
use tokio_io::io;
use tokio_io::AsyncRead;

use futures::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::unbounded;

use local_db::*;
use frame::*;
use peer::*;

/// HDRL defines the size of the header including version, length and timestamp
pub const HDRL: usize = 8; 
/// CIDL defines the size of the connection id
pub const CIDL:  usize = 4; 
/// KEYL defines the size of the key
pub const KEYL: usize = 8; 
/// HDRKEYL defines the size of the header + key
pub const HDRKEYL: usize = HDRL + KEYL;

const KEEPALIVE: u64 = 5;
const HISTLIMIT: usize = 100;

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
    pub fn set_message(mut self, message: Vec<u8>) -> Msg {
        self.message = message;
        self
    }

    /// Get uid for Msg object. See example for set uid.
    pub fn get_uid(&self) -> &String {
        &self.uid
    }

    /// Get channel for Msg object. See example for set channel.
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
    pub fn get_message(&self) -> &Vec<u8> {
        &self.message
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
    pub fn get_key(&self) -> Option<u64> {
        self.key
    }

    /// Connects to the defined address with a message.
    /// 
    pub fn connect_with_msg(mut self, raddr: SocketAddr, msg: Vec<u8>) -> MsgConn {
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
                    keys.push(addr2str(&laddr));
                    keys.push(self.get_uid());
                    keys.push(self.get_channel());
                    let key = do_hash(&keys);
                    self.key = Some(key);
                }
                let encoded_msg = message_encode(&msg);
                let key = self.get_key().unwrap();
                let keyv = write_key(key);
                let mut msgv = write_hdr(encoded_msg.len());
                msgv = write_cid_to_hdr(msgv);
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
    pub fn connect(self, raddr: SocketAddr) -> MsgConn {
        self.connect_with_msg(raddr, Vec::new())
    }

    /// Send a message. Blocks until a message is sent.
    /// 
    /// # Errors
    /// If a message cannot be sent, stream is set to None.
    /// 
    pub fn send_message(mut self, msg: Vec<u8>) -> MsgConn {
        let encoded_msg = message_encode(&Msg::new(self.get_uid(), self.get_channel(), msg));
        let key = self.get_key().unwrap();
        let keyv = write_key(key);
        let mut msgv = write_hdr(encoded_msg.len());
        msgv = write_cid_to_hdr(msgv);
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
            //ignore all connects
            if 0 == payload.len() {
                continue;
            }
            let decoded_message = message_decode(payload.as_slice());
            self.stream = Some(stream);
            return (self, decoded_message.get_message().to_owned());
        }
    }

    /// Closes the connection.
    /// 
    pub fn close(mut self) -> MsgConn {
        if self.stream.is_some() {
            drop(self.stream.unwrap());
        }
        self.stream = None;
        self
    }
}

/// Encode Msg object to CBOR.
///
/// # Errors
/// If message cannot be encoded, an empty vector is returned.
///
/// # Example
/// ```
/// use mles_utils::{Msg, message_encode};
///
/// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
/// let encoded_msg: Vec<u8> = message_encode(&msg);
/// ```
#[inline]
pub fn message_encode(msg: &Msg) -> Vec<u8> {
    let encoded = serde_cbor::to_vec(msg);
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
/// use mles_utils::{Msg, message_encode, message_decode};
///
/// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
/// let encoded_msg: Vec<u8> = message_encode(&msg);
/// let decoded_msg: Msg = message_decode(&encoded_msg);
/// ```
#[inline]
pub fn message_decode(slice: &[u8]) -> Msg {
    let value = serde_cbor::from_slice(slice);
    match value {
        Ok(value) => value,
        Err(err) => {
            println!("Error on decode: {}", err);
            Msg { uid: "".to_string(), channel: "".to_string(), message: Vec::new() } // return empty vec in case of error
        }
    }
}

/// Read received buffer header type.
///
/// # Errors
/// If input vector length is smaller than needed, zero is returned.
///
/// # Example
/// ```
/// use mles_utils::read_hdr_type;
///
/// let hdr: Vec<u8> = vec![77,1,2,3,0,0,0,0];
/// let hdr_type = read_hdr_type(&hdr);
/// assert_eq!('M' as u32, hdr_type);
/// ```
#[inline]
pub fn read_hdr_type(hdr: &[u8]) -> u32 { 
    if hdr.len() < HDRL {
        return 0;
    }
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    num >> 24
}

/// Read received buffer header len.
///
/// # Errors
/// If input vector length is smaller than needed, zero is returned.
///
/// # Example
/// ```
/// use mles_utils::read_hdr_len;
///
/// let hdr: Vec<u8> = vec![77,1,2,3,0,0,0,0];
/// let hdr_len = read_hdr_len(&hdr);
/// assert_eq!(515, hdr_len);
/// ```
#[inline]
pub fn read_hdr_len(hdr: &[u8]) -> usize { 
    if hdr.len() < HDRL {
        return 0;
    }
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    (num & 0xfff) as usize
}

/// Write a valid Mles header with specified length to network byte order.
///
/// # Example
/// ```
/// use mles_utils::{write_hdr, read_hdr_len};
///
/// let hdr = write_hdr(515);
/// let hdr_len = read_hdr_len(&hdr);
/// assert_eq!(515, hdr_len);
/// ```
#[inline]
pub fn write_hdr(len: usize) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let cid = 0 as u32;
    let mut msgv = vec![];
    let mut cidv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    cidv.write_u32::<BigEndian>(cid).unwrap();
    msgv.extend(cidv);
    msgv
}

/// Write a valid Mles header with specified length to network byte order without cid.
///
/// # Example
/// ```
/// use mles_utils::{write_hdr_without_cid, read_hdr_len, HDRL, CIDL};
///
/// let hdr = write_hdr_without_cid(515);
/// assert_eq!(HDRL-CIDL, hdr.len());
/// ```
#[inline]
pub fn write_hdr_without_cid(len: usize) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let mut msgv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    msgv
}

/// Return a random connection id.
///
/// # Example
/// ```
/// use mles_utils::select_cid;
///
/// let cid = select_cid();
/// assert!(cid >= 0x1 && cid <= 0x7fffffff);
/// ```
#[inline]
pub fn select_cid() -> u32 {
    let mut rnd: u32 = rand::random();
    rnd >>= 1; //skip values larger than 0x7fffffff
    if 0 == rnd { //skip zero
        rnd = 1;
    }
    rnd
}

/// Write a random connection id in network byte order.
///
/// # Example
/// ```
/// use mles_utils::{write_cid, select_cid, CIDL};
///
/// let cidv = write_cid(select_cid());
/// assert_eq!(CIDL, cidv.len());
/// ```
#[inline]
pub fn write_cid(cid: u32) -> Vec<u8> {
    let mut cidv = vec![];
    assert!(cid >= 0x1 && cid <= 0x7fffffff);
    cidv.write_u32::<BigEndian>(cid).unwrap();
    cidv
}

/// Write a random connection id in network byte order to the header.
///
/// # Example
/// ```
/// use mles_utils::{write_hdr, write_cid_to_hdr};
///
/// let mut hdr = write_hdr(515);
/// let hdr = write_cid_to_hdr(hdr);
/// ```
#[inline]
pub fn write_cid_to_hdr(mut hdrv: Vec<u8>) -> Vec<u8> {
    if hdrv.len() < HDRL {
        return vec![];
    }
    let tail = hdrv.split_off(HDRL);
    hdrv.truncate(HDRL - CIDL); //drop existing cid
    hdrv.extend(write_cid(select_cid())); //add new cid
    hdrv.extend(tail);
    hdrv
}


/// Write a valid key to network byte order.
///
/// # Example
/// ```
/// use mles_utils::{write_key, do_hash};
///
/// let hashstr = "A string".to_string();
/// let hashable = vec![hashstr];
/// let key = do_hash(&hashable); 
/// let keyhdr: Vec<u8> = write_key(key);
/// ```
#[inline]
pub fn write_key(val: u64) -> Vec<u8> {
    let key = val;
    let mut msgv = vec![];
    msgv.write_u64::<BigEndian>(key).unwrap();
    msgv
}

/// Write a valid Mles header with specified length and key to network byte order.
///
/// # Example
/// ```
/// use mles_utils::{write_hdr_with_key, read_hdr_len, read_key_from_hdr, do_hash};
///
/// let hashstr = "Yet another string".to_string();
/// let hashable = vec![hashstr];
/// let key = do_hash(&hashable); 
/// let hdr = write_hdr_with_key(515, key);
/// let hdr_len = read_hdr_len(&hdr);
/// assert_eq!(515, hdr_len);
/// let keyx = read_key_from_hdr(&hdr);
/// assert_eq!(key, keyx);
/// ```
#[inline]
pub fn write_hdr_with_key(len: usize, key: u64) -> Vec<u8> {
    let mut hdrv = write_hdr(len);
    hdrv.extend(write_key(key));
    hdrv
}

/// Read a key from buffer.
///
/// # Errors
/// If input vector length is smaller than needed, zero is returned.
///
/// # Example
/// ```
/// use mles_utils::{write_key, read_key, do_hash};
///
/// let hashstr = "Another string".to_string();
/// let hashable = vec![hashstr];
/// let key = do_hash(&hashable); 
/// let keyhdr: Vec<u8> = write_key(key);
/// let read_key = read_key(&keyhdr);
/// assert_eq!(key, read_key);
/// ```
#[inline]
pub fn read_key(keyv: &[u8]) -> u64 {
    if keyv.len() < KEYL {
        return 0;
    }
    let mut buf = Cursor::new(&keyv[..]);
    buf.read_u64::<BigEndian>().unwrap()
}

/// Read a key from header.
///
/// # Errors
/// If input vector length is smaller than needed, zero is returned.
///
/// # Example
/// ```
/// use mles_utils::{write_hdr, write_key, read_key_from_hdr, do_hash};
///
/// let hashstr = "Another string".to_string();
/// let hashable = vec![hashstr];
/// let key = do_hash(&hashable); 
/// let mut hdr: Vec<u8> = write_hdr(12);
/// let keyhdr: Vec<u8> = write_key(key);
/// hdr.extend(keyhdr);
/// let read_key = read_key_from_hdr(&hdr);
/// assert_eq!(key, read_key);
/// ```
#[inline]
pub fn read_key_from_hdr(keyv: &[u8]) -> u64 {
    if keyv.len() < HDRKEYL {
        return 0;
    }
    let mut buf = Cursor::new(&keyv[HDRL..]);
    buf.read_u64::<BigEndian>().unwrap()
}

/// Read a connection id from header.
///
/// # Errors
/// If input vector length is smaller than needed, zero is returned.
///
/// # Example
/// ```
/// use mles_utils::{write_hdr_with_key, write_cid_to_hdr, read_cid_from_hdr};
///
/// let mut hdr: Vec<u8> = write_hdr_with_key(12, 0x3f3f3); //cid set to zero
/// let read_cid = read_cid_from_hdr(&hdr);
/// assert_eq!(0, read_cid);
///
/// hdr = write_cid_to_hdr(hdr);
/// let read_cid = read_cid_from_hdr(&hdr);
/// assert!(read_cid >= 0x1 && read_cid <= 0x7fffffff);
///
/// ```
#[inline]
pub fn read_cid_from_hdr(hdrv: &[u8]) -> u32 {
    if hdrv.len() < HDRL {
        return 0;
    }
    let mut buf = Cursor::new(&hdrv[(HDRL-CIDL)..]);
    buf.read_u32::<BigEndian>().unwrap()
}


/// Do a valid hash for Mles over provided UTF-8 String list.
///
/// # Example
/// ```
/// use mles_utils::do_hash;
///
/// let hashstr1 = "A string".to_string();
/// let hashstr2 = "Another string".to_string();
/// let hashable = vec![hashstr1, hashstr2];
/// let key: u64 = do_hash(&hashable); 
/// ```
#[inline]
pub fn do_hash(t: &[String]) -> u64 {
    let mut s = SipHasher::new();
    for item in t {
        item.hash(&mut s);
    }
    s.finish()
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
/// use mles_utils::addr2str;
///
/// let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
/// let addrstr = addr2str(&addr);
///
/// assert_eq!("127.0.0.1:8080", addrstr);
///
/// let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0xff03, 0, 0, 0, 0, 0, 0, 1)), 8077);
/// let addrstr = addr2str(&addr);
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
/// let raddr = "127.0.0.1:8077";
/// let raddr: Vec<_> = raddr.to_socket_addrs()
///                    .unwrap_or_else(|_| vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)].into_iter())
///                    .collect();
/// let raddr = *raddr.first().unwrap();
/// let raddr = Some(raddr).unwrap();
/// assert_ne!(0, raddr.port());
/// let uid = "User".to_string();
/// let channel = "Channel".to_string();
/// let message = "Hello World!".to_string();
        
/// let child = thread::spawn(|| server_run(":8077", "".to_string(), "".to_string(), None, 100));
/// drop(child);
/// ```
pub fn server_run(port: &str, keyval: String, keyaddr: String, peer: Option<SocketAddr>, hist_limit: usize) {
    let mut history_limit = HISTLIMIT;
    if 0 != hist_limit {
        history_limit = hist_limit;
    }


    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let address = "0.0.0.0".to_string() + port;
    let address = address.parse().unwrap();
    let socket = match TcpListener::bind(&address, &handle) {
        Ok(listener) => listener,
        Err(err) => {
            println!("Error: {}", err);
            process::exit(1);
        },
    };
    println!("Listening on: {}", address);

    let mles_db_hash: HashMap<String, MlesDb> = HashMap::new();
    let mles_db = Rc::new(RefCell::new(mles_db_hash));
    let channel_db = Rc::new(RefCell::new(HashMap::new()));

    let mut cnt = 0;

    let srv = socket.incoming().for_each(move |(stream, addr)| {
        let _val = stream.set_nodelay(true)
                         .map_err(|_| panic!("Cannot set to no delay"));
        let _val = stream.set_keepalive(Some(Duration::new(KEEPALIVE, 0)))
                         .map_err(|_| panic!("Cannot set keepalive"));
        cnt += 1;

        println!("New Connection: {}", addr);
        let paddr = match stream.peer_addr() {
                Ok(paddr) => paddr,
                Err(_) => {
                    let addr = "0.0.0.0:0";
                    addr.parse::<SocketAddr>().unwrap()
                }
        };
        let mut is_addr_set = false;
        let mut keys = Vec::new();
        if !keyval.is_empty() {
            keys.push(keyval.clone());
        }
        else {
            keys.push(addr2str(&paddr));
            is_addr_set = true;
            if !keyaddr.is_empty() {
                keys.push(keyaddr.clone());
            }
        }

        let (reader, writer) = stream.split();
        let (tx, rx) = unbounded();

        let (tx_peer_for_msgs, rx_peer_for_msgs) = unbounded();

        let frame = io::read_exact(reader, vec![0;HDRKEYL]);
        let frame = frame.and_then(move |(reader, hdr_key)| process_hdr(reader, hdr_key));
        let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| { 
            let tframe = io::read_exact(reader, vec![0;hdr_len]);
            tframe.and_then(move |(reader, message)| process_msg(reader, hdr_key, message))
        });

        // verify key
        let frame = frame.and_then(move |(reader, hdr_key, message)| process_key(reader, hdr_key, message, keys));

        let tx_inner = tx.clone();
        let channel_db_inner = channel_db.clone();
        let mles_db_inner = mles_db.clone();
        let keyaddr_inner = keyaddr.clone();
        let socket_once = frame.and_then(move |(reader, mut hdr_key, message, decoded_message)| {
                let channel = decoded_message.get_channel().clone();
                let mut mles_db_once = mles_db_inner.borrow_mut();
                let mut channel_db = channel_db_inner.borrow_mut();

                //pick the verified cid from header and use it as an identifier
                let cid = read_cid_from_hdr(&hdr_key);

                if !mles_db_once.contains_key(&channel) {
                    let chan = channel.clone();

                    //if peer is set, create peer channel thread
                    if peer::has_peer(&peer) {
                        let mut msg = hdr_key.clone();
                        msg.extend(message.clone());
                        let peer = peer.unwrap();
                        thread::spawn(move || peer_conn(history_limit, peer, is_addr_set, keyaddr_inner, chan, msg, &tx_peer_for_msgs));
                    }

                    let mut mles_db_entry = MlesDb::new(history_limit);
                    mles_db_entry.add_channel(cid, tx_inner.clone());
                    mles_db_once.insert(channel.clone(), mles_db_entry);

                }
                else if let Some(mles_db_entry) = mles_db_once.get_mut(&channel) {
                    if mles_db_entry.check_for_duplicate_cid(cid) {
                        println!("Duplicate cid {:x} detected", cid);
                        return Err(Error::new(ErrorKind::BrokenPipe, "duplicate cid"));
                    }
                    mles_db_entry.add_channel(cid, tx_inner.clone());
                    if peer::has_peer(&peer) {
                        if let Some(channelpeer_entry) = mles_db_entry.get_peer_tx() {
                            //sending tx to peer
                            let _res = channelpeer_entry.send(tx_inner.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
                        }
                        else {
                            println!("Cannot find channel peer for channel {}", channel);
                        }
                    }
                    else {
                        // send history to client if peer is not set
                        for msg in mles_db_entry.get_messages() {
                            let _res = tx_inner.send(msg.clone()).map_err(|err| {println!("Send history failed: {}", err); ()});
                        }
                    }
                }
                else {
                    println!("Channel {} not found", channel);
                    return Err(Error::new(ErrorKind::BrokenPipe, "internal error"));
                }

                if let Some(mles_db_entry) = mles_db_once.get_mut(&channel) {
                    // add to history if no peer
                    if !peer::has_peer(&peer) {
                        hdr_key.extend(message);
                        mles_db_entry.add_message(hdr_key);
                    }
                    mles_db_entry.add_tx_db(tx_inner.clone());
                }
                channel_db.insert(cnt, (cid, channel.clone()));
                println!("User {}:{:x} joined.", cnt, cid);
                Ok((reader, cid, channel))
        });

        let mles_db_inner = mles_db.clone();
        let socket_next = socket_once.and_then(move |(reader, cid, channel)| {
            let channel_next = channel.clone();
            let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;HDRKEYL]);
                let frame = frame.and_then(move |(reader, hdr_key)| process_hdr_dummy_key(reader, hdr_key));

                let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| {
                    let tframe = io::read_exact(reader, vec![0;hdr_len]);
                    tframe.and_then(move |(reader, message)| process_msg(reader, hdr_key, message)) 
                });

                let mles_db = mles_db_inner.clone();
                let channel = channel_next.clone();
                frame.map(move |(reader, mut hdr_key, message)| {
                    hdr_key.extend(message);

                    let mut mles_db_borrow = mles_db.borrow_mut();
                    if let Some(mles_db_entry) = mles_db_borrow.get_mut(&channel) {
                        // add to history if no peer
                        if !peer::has_peer(&peer) {
                            mles_db_entry.add_message(hdr_key.clone());
                        }

                        if let Some(channels) = mles_db_entry.get_channels() {
                            for (ocid, tx) in channels.iter() {
                                if *ocid != cid {
                                    let _res = tx.send(hdr_key.clone()).map_err(|_| { 
                                        //just ignore failures for now
                                        () 
                                    });
                                }
                            }
                        }
                    }
                    else {
                        println!("Cannot distribute channel {}", channel);
                    }

                    reader
                })
            })
        });

        let mles_db_inner = mles_db.clone();
        let peer_writer = rx_peer_for_msgs.for_each(move |(peer_cid, channel, peer_tx, tx_orig_chan)| {
            let mut mles_db_once = mles_db_inner.borrow_mut();
            if let Some(mut mles_db_entry) = mles_db_once.get_mut(&channel) {  
                //setting peer tx
                mles_db_entry.add_channel(peer_cid, peer_tx);  
                mles_db_entry.set_peer_tx(tx_orig_chan.clone());
                //sending all tx's to (possibly restarted) peer
                for tx_entry in mles_db_entry.get_tx_db() {
                    let _res = tx_orig_chan.send(tx_entry.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
                }
            }
            else {
                println!("Cannot find peer channel {}", channel);
            }
            Ok(())
        });
        handle.spawn(peer_writer.then(|_| {
            Ok(())
        }));

        let socket_writer = rx.fold(writer, |writer, msg| {
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });

        let mles_db_conn = mles_db.clone();
        let channel_db_conn = channel_db.clone();
        let socket_reader = socket_next.map_err(|_| ());
        let connection = socket_reader.map(|_| ()).select(socket_writer.map(|_| ()));
        handle.spawn(connection.then(move |_| {
            let mut mles_db = mles_db_conn.borrow_mut();
            let mut channel_db = channel_db_conn.borrow_mut();
            let mut chan_to_rem: Option<u32> = None;
            let mut chan_drop = false;
            if let Some(&mut (cid, ref channel)) = channel_db.get_mut(&cnt) {
                if let Some(mles_db_entry) = mles_db.get_mut(channel) {
                    mles_db_entry.rem_channel(cid);
                    chan_to_rem = Some(cid);
                    if 0 == mles_db_entry.get_channels_len() {
                        mles_db_entry.clear_tx_db();
                        if 0 == mles_db_entry.get_history_limit() {
                            chan_drop = true;
                        }
                    }
                }
                if chan_drop {
                    mles_db.remove(channel);
                }
            }
            if let Some(cid) = chan_to_rem {
                channel_db.remove(&cid);
                println!("Connection {} for user {}:{:x} closed.", addr, cnt, cid);
            }
            Ok(())
        }));
        Ok(())
    });

    // execute server                               
    let _res = core.run(srv).map_err(|err| { println!("Main: {}", err); ()});
}

#[cfg(test)]
mod tests {
    use std::net::{SocketAddr, ToSocketAddrs};
    use std::net::{IpAddr, Ipv4Addr};
    use super::*;

    #[test]
    fn test_encode_decode_msg() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let msg =  "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = message_encode(&orig_msg);
        let decoded_msg = message_decode(&encoded_msg);
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
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
    fn test_hash() {
        let addr = "127.0.0.1:8077";
        let addr = addr.parse::<SocketAddr>().unwrap();
        let orig_key = do_hash(&vec![addr2str(&addr)]);
        let keyv = write_key(orig_key);
        let key = read_key(&keyv);
        assert_eq!(orig_key, key);
    }

    #[test]
    fn test_cid() {
        let orig_key = 0xffeffe;
        let mut hdrv = write_hdr_with_key(64, orig_key);
        let orig_len = hdrv.len();
        let key = read_key_from_hdr(&hdrv);
        assert_eq!(orig_key, key);
        let read_cid = read_cid_from_hdr(&hdrv);
        assert_eq!(0, read_cid);
        hdrv = write_cid_to_hdr(hdrv);
        let read_cid = read_cid_from_hdr(&hdrv);
        assert_ne!(0, read_cid);
        let key = read_key_from_hdr(&hdrv);
        assert_eq!(orig_key, key);
        let len = hdrv.len();
        assert_eq!(orig_len, len);
    }

    #[test]
    fn test_msgconn_api() {
        let sec = Duration::new(1,0);
        let raddr = "127.0.0.1:8077";
        let raddr: Vec<_> = raddr.to_socket_addrs()
            .unwrap_or_else(|_| vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)].into_iter())
            .collect();
        let raddr = *raddr.first().unwrap();
        let raddr = Some(raddr).unwrap();
        assert_ne!(0, raddr.port());
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let message = "Hello World!".to_string();
         
        //create server
        let child = thread::spawn(|| server_run(":8077", "".to_string(), "".to_string(), None, 100));
        thread::sleep(sec);

        //send hello world
        let mut conn = MsgConn::new(uid.clone(), channel.clone());
        conn = conn.connect_with_msg(raddr, message.into_bytes());
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

}



