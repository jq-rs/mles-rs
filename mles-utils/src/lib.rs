#[macro_use]
extern crate serde_derive;
extern crate serde_cbor;
extern crate byteorder;

use std::io::{Read, Cursor, Error};
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};

#[derive(Serialize, Deserialize, Debug)]
pub enum KeyUser {
    Key (u64),
    User (String)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Msg {
    pub keyuser: KeyUser,
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
            Msg { keyuser: KeyUser::User("".to_string()), channel: "".to_string(), message: Vec::new() } // return empty vec in case of error
        }
    }
}

pub fn read_n<R>(reader: R, bytes_to_read: u64) -> (Result<usize, Error>, Vec<u8>)
where R: Read,
{
    let mut buf = vec![];
    let mut chunk = reader.take(bytes_to_read);
    let status = chunk.read_to_end(&mut buf);
    (status, buf)
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

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_msg() {
        let orig_msg = Msg { keyuser: KeyUser::User("User".to_string()), channel: "Channel".to_string(), message: "a test msg".to_string().into_bytes() };
        let orig_user = match orig_msg.keyuser {
            KeyUser::User(user) => user,
                _ => "".to_string(),
        };
        let msg = Msg { keyuser: KeyUser::User("User".to_string()), channel: "Channel".to_string(), message: "a test msg".to_string().into_bytes() };
        let cbor_msg = message_encode(&msg);
        let decoded_msg = message_decode(&cbor_msg);
        let decoded_user = match decoded_msg.keyuser {
            KeyUser::User(user) => user,
                _ => "".to_string(),
        };
        assert_eq!(decoded_user, orig_user);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
    }
}

