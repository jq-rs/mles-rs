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
extern crate byteorder;
extern crate siphasher;

use std::io::Cursor;
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};
use siphasher::sip::SipHasher;
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Msg {
    pub uid:     String,
    pub channel: String,
    pub message: Vec<u8>,
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
        let orig_msg = Msg { uid: "User".to_string(), channel: "Channel".to_string(), message: "a test msg".to_string().into_bytes() };
        let cbor_msg = message_encode(&orig_msg);
        let decoded_msg = message_decode(&cbor_msg);
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
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

