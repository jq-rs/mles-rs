#![warn(missing_docs)]
//! `Mles utils` library is provided for Mles client and server implementations for easy handling of 
//! proper header and message structures.

/**
 *   Mles-utils to be used with Mles client or server.
 *
 *   Copyright (C) 2017 Juhamatti Kuusisaari / Mles developers
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
extern crate serde_bytes;
extern crate byteorder;
extern crate siphasher;


use std::io::Cursor;
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};
use siphasher::sip::SipHasher;
use std::hash::{Hash, Hasher};

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

/// Encode Msg object to CBOR.
///
/// # Example
/// ```
/// use mles_utils::{Msg, message_encode};
///
/// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
/// let encoded_msg: Vec<u8> = message_encode(&msg);
/// ```
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
/// # Example
/// ```
/// use mles_utils::{Msg, message_encode, message_decode};
///
/// let msg = Msg::new("My uid".to_string(), "My channel".to_string(), Vec::new());
/// let encoded_msg: Vec<u8> = message_encode(&msg);
/// let decoded_msg: Msg = message_decode(&encoded_msg);
/// ```
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
/// # Example
/// ```
/// use mles_utils::read_hdr_type;
///
/// let hdr: Vec<u8> = vec![77,1,2,3];
/// let hdr_type = read_hdr_type(hdr.as_slice());
/// assert_eq!('M' as u32, hdr_type);
/// ```
pub fn read_hdr_type(hdr: &[u8]) -> u32 { 
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    num >> 24
}

/// Read received buffer header len.
///
/// # Example
/// ```
/// use mles_utils::read_hdr_len;
///
/// let hdr: Vec<u8> = vec![77,1,2,3];
/// let hdr_len = read_hdr_len(hdr.as_slice());
/// assert_eq!(515, hdr_len);
/// ```
pub fn read_hdr_len(hdr: &[u8]) -> usize { 
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
/// let hdr_len = read_hdr_len(hdr.as_slice());
/// assert_eq!(515, hdr_len);
/// ```
pub fn write_hdr(len: usize) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let mut msgv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    msgv
}

/// Write a valid key to network byte order.
///
/// # Example
/// ```
/// use mles_utils::{write_key, do_hash};
///
/// let hashstr = "A string".to_string();
/// let hashable = vec![&hashstr];
/// let key = do_hash(&hashable); 
/// let keyhdr: Vec<u8> = write_key(key);
/// ```
pub fn write_key(val: u64) -> Vec<u8> {
    let key = val;
    let mut msgv = vec![];
    msgv.write_u64::<BigEndian>(key).unwrap();
    msgv
}

/// Read a key from buffer.
///
/// # Example
/// ```
/// use mles_utils::{write_key, read_key, do_hash};
///
/// let hashstr = "Another string".to_string();
/// let hashable = vec![&hashstr];
/// let key = do_hash(&hashable); 
/// let keyhdr: Vec<u8> = write_key(key);
/// let read_key = read_key(&keyhdr);
/// assert_eq!(key, read_key);
/// ```
pub fn read_key(keyv: &Vec<u8>) -> u64 {
    let mut buf = Cursor::new(&keyv[..]);
    let num = buf.read_u64::<BigEndian>().unwrap();
    num
}

/// Do a valid hash for Mles over provided list.
///
/// # Example
/// ```
/// use mles_utils::do_hash;
///
/// let hashstr1 = "A string".to_string();
/// let hashstr2 = "Another string".to_string();
/// let hashable = vec![&hashstr1, &hashstr2];
/// let key: u64 = do_hash(&hashable); 
/// ```
pub fn do_hash<T: Hash>(t: &Vec<&T>) -> u64 {
    let mut s = SipHasher::new();
    for item in t {
        item.hash(&mut s);
    }
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
        let orig_key = do_hash(&vec![&addr]);
        let keyv = write_key(orig_key);
        let key = read_key(&keyv);
        assert_eq!(orig_key, key);
    }
}

