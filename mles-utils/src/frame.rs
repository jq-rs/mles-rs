/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/. 
*
*  Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
* */
extern crate tokio_core;
extern crate tokio_io;
extern crate futures;

use std::io::{Error, ErrorKind};

use tokio_core::net::TcpStream;
use tokio_io::io;

use super::*;

pub fn process_hdr_dummy_key(reader: io::ReadHalf<TcpStream>, hdr_key: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, usize), Error> {
    process_hdr(reader, hdr_key)
}

pub fn process_hdr(reader: io::ReadHalf<TcpStream>, hdr: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, usize), Error> {
    if hdr.is_empty() {
        return Err(Error::new(ErrorKind::BrokenPipe, "broken pipe"));
    }
    if read_hdr_type(&hdr) != 'M' as u32 {
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header"));
    }
    let hdr_len = read_hdr_len(&hdr);
    if 0 == hdr_len {
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header len"));
    }
    Ok((reader, hdr, hdr_len))
}

pub fn process_msg(reader: io::ReadHalf<TcpStream>, hdr_key: Vec<u8>, message: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, Vec<u8>), Error> { 
    if message.is_empty() { 
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"));
    }
    Ok((reader, hdr_key, message))
}

pub fn process_key(reader: io::ReadHalf<TcpStream>, hdr_key: Vec<u8>, message: Vec<u8>, mut keys: Vec<String>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, bool, Vec<u8>, Msg, ResyncMsg), Error> { 
    //read hash from message
    let key = read_key_from_hdr(&hdr_key);

    //decode message
    let (decoded_message, decoded_resync_message, is_resync) = message_decode(&message);
    //create hash for verification
    keys.push(decoded_message.get_uid().to_string());
    keys.push(decoded_message.get_channel().to_string());

    let hkey = do_hash(&keys);
    if hkey != key {
        println!("Incorrect key {:x} != {:x}", hkey, key);
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect remote key"));
    }
    Ok((reader, hdr_key, is_resync, message, decoded_message, decoded_resync_message))
}

fn message_decode(message: &Vec<u8>) -> (Msg, ResyncMsg, bool) {
    //try first decoding as a resync
    let decoded_resync_message: ResyncMsg = ResyncMsg::decode(message.as_slice());
    let decoded_rvec = decoded_resync_message.first();
    if !decoded_rvec.is_empty() {
        let decoded_message = Msg::decode(decoded_rvec.as_slice());
        return (decoded_message, decoded_resync_message, true);
    }
    let decoded_message = Msg::decode(message.as_slice());
    (decoded_message, decoded_resync_message, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_decode() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let msg =  "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        let (decoded_msg, decoded_resync_message, is_resync) = message_decode(&encoded_msg);
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
        assert_eq!(0, decoded_resync_message.len());
        assert_eq!(false, is_resync);
    }

    #[test]
    fn test_message_resync_decode() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let msg =  "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        let vec = vec![encoded_msg];
        let rmsg = ResyncMsg::new(&vec);
        let encoded_resync_msg: Vec<u8> = rmsg.encode();
        let (decoded_msg, decoded_resync_message, is_resync) = message_decode(&encoded_resync_msg);
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
        assert_eq!(1, decoded_resync_message.len());
        assert_eq!(true, is_resync);
    }

    #[test]
    fn test_message_resync_multi_decode() {
        let uid = "Resync User".to_string();
        let channel = "Resync Channel".to_string();
        let msg =  "a resync test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        let mut vec = Vec::new();
        for _ in 0..99 {
            vec.push(encoded_msg.clone());
        }
        vec.push(encoded_msg);
        let rmsg = ResyncMsg::new(&vec);
        let encoded_resync_msg: Vec<u8> = rmsg.encode();
        let (decoded_msg, decoded_resync_message, is_resync) = message_decode(&encoded_resync_msg);
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
        assert_eq!(100, decoded_resync_message.len());
        assert_eq!(true, is_resync);
    }
}



