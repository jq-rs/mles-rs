/**
 *   Mles-utils to be used with Mles client or server.
 *   Copyright (C) 2017  Mles developers
 *
 *   This program is free software: you can redistribute it and/or modify
 *   it under the terms of the GNU General Public License as published by
 *   the Free Software Foundation, either version 3 of the License, or
 *   (at your option) any later version.
 *
 *   This program is distributed in the hope that it will be useful,
 *   but WITHOUT ANY WARRANTY; without even the implied warranty of
 *   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *   GNU General Public License for more details.
 *
 *   You should have received a copy of the GNU General Public License
 *   along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */
#[macro_use]
extern crate serde_derive;
extern crate serde_cbor;
extern crate serde_json;
extern crate byteorder;
extern crate siphasher;

use std::io::Cursor;
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};
use siphasher::sip::SipHasher;
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Msg {
    uid:     String,
    channel: String,
    message: Vec<u8>,
}

impl Msg {
    pub fn new(uid: String, channel: String, message: Vec<u8>) -> Msg {
        Msg {
            uid: uid,
            channel: channel,
            message: message
        }
    }

    pub fn set_uid(mut self, uid: String) -> Msg {
        self.uid = uid;
        self
    }

    pub fn set_channel(mut self, channel: String) -> Msg {
        self.channel = channel;
        self
    }

    pub fn set_message(mut self, message: Vec<u8>) -> Msg {
        self.message = message;
        self
    }

    pub fn get_uid(&self) -> &String {
        &self.uid
    }

    pub fn get_channel(&self) -> &String {
        &self.channel
    }

    pub fn get_message(&self) -> &Vec<u8> {
        &self.message
    }
}

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

pub fn read_hdr_type(hdr: &[u8]) -> u32 { 
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    num >> 24
}

pub fn read_hdr_len(hdr: &[u8]) -> usize { 
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    (num & 0xfff) as usize
}

pub fn write_hdr(len: usize) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let mut msgv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    msgv
}

pub fn write_key(val: u64) -> Vec<u8> {
    let key = val;
    let mut msgv = vec![];
    msgv.write_u64::<BigEndian>(key).unwrap();
    msgv
}

pub fn read_key(keyv: &Vec<u8>) -> u64 {
    let mut buf = Cursor::new(&keyv[..]);
    let num = buf.read_u64::<BigEndian>().unwrap();
    num
}

pub fn do_hash<T: Hash>(t: &T) -> u64 {
    let mut s = SipHasher::new();
    t.hash(&mut s);
    s.finish()
}

#[cfg(test)]

mod tests {
    use std::net::SocketAddr;
    use super::*;

    #[test]
    fn test_encode_decode_msg() {
        let uid =  "User".to_string();
        let channel =  "Channel".to_string();
        let msg =  "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let cbor_msg = message_encode(&orig_msg);
        let decoded_msg = message_decode(&cbor_msg);
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
        let orig_key = do_hash(&addr);
        let keyv = write_key(orig_key);
        let key = read_key(&keyv);
        assert_eq!(orig_key, key);
    }
}

