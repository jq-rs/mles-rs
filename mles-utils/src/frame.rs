/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/.
*
*  Copyright (C) 2017-2018  Mles developers
* */
use std::io::{Error, ErrorKind};

use bytes::{Bytes, BytesMut};
use tokio::io;
use tokio::net::TcpStream;

use super::*;

pub(crate) fn process_hdr_dummy_key(
    reader: io::ReadHalf<TcpStream>,
    hdr_key: BytesMut,
) -> Result<(io::ReadHalf<TcpStream>, BytesMut, usize), Error> {
    process_hdr(reader, hdr_key)
}

pub(crate) fn process_hdr(
    reader: io::ReadHalf<TcpStream>,
    hdr: BytesMut,
) -> Result<(io::ReadHalf<TcpStream>, BytesMut, usize), Error> {
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

pub(crate) fn process_msg(
    reader: io::ReadHalf<TcpStream>,
    hdr_key: BytesMut,
    message: BytesMut,
) -> Result<(io::ReadHalf<TcpStream>, BytesMut, BytesMut), Error> {
    if message.is_empty() {
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"));
    }
    Ok((reader, hdr_key, message))
}

pub(crate) fn process_key(
    reader: io::ReadHalf<TcpStream>,
    hdr_key: BytesMut,
    message: BytesMut,
    mut keys: Vec<String>,
) -> Result<(io::ReadHalf<TcpStream>, Vec<Bytes>, Msg), Error> {
    //decode message(s)
    let messages = message_decode(message, hdr_key);
    if messages.is_empty() {
        println!("Incorrect msg");
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect msg"));
    }
    let mut hdrkey = messages[0].clone();
    let message = hdrkey.split_off(HDRKEYL);
    //read key from header
    let key = read_key_from_hdr(&hdrkey);

    let decoded_message = Msg::decode(message.as_ref());
    keys.push(decoded_message.get_uid().to_string());
    keys.push(decoded_message.get_channel().to_string());

    let hkey = MsgHdr::do_hash(&keys);
    if hkey != key {
        println!("Incorrect key {:x} != {:x}", hkey, key);
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect remote key"));
    }
    Ok((reader, messages, decoded_message))
}

fn message_decode(message: BytesMut, mut hdr_key: BytesMut) -> Vec<Bytes> {
    //try first decoding as a resync
    let mut msgs = Vec::new();
    let decoded_resync_message: ResyncMsg = ResyncMsg::decode(message.as_ref());
    if 0 != decoded_resync_message.len() {
        for msg in decoded_resync_message.get_messages() {
            msgs.push(Bytes::from(msg));
        }
    } else {
        hdr_key.unsplit(message);
        let msg = hdr_key.freeze();
        msgs.push(msg);
    }
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_decode() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let mut keys = Vec::new();
        keys.push(uid.clone());
        keys.push(channel.clone());
        let key = MsgHdr::do_hash(&keys);
        let mut hdr: BytesMut = write_hdr(122, MsgHdr::select_cid(key));
        let keyhdr: BytesMut = write_key(key);
        hdr.extend(keyhdr);
        let hdrkey = hdr.clone();
        let msg = "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        let messages = message_decode(BytesMut::from(encoded_msg), BytesMut::from(hdrkey));
        assert_eq!(1, messages.len());
        let mut message = messages[0].clone();
        let msg = message.split_off(HDRKEYL);
        let decoded_msg = Msg::decode(msg.as_ref());
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
    }

    #[test]
    fn test_message_resync_decode() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let mut keys = Vec::new();
        keys.push(uid.clone());
        keys.push(channel.clone());
        let key = MsgHdr::do_hash(&keys);
        let mut hdr: BytesMut = write_hdr(122, MsgHdr::select_cid(key));
        let keyhdr: BytesMut = write_key(key);
        hdr.extend(keyhdr);
        let hdrkey = hdr.clone();
        let msg = "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        hdr.extend(encoded_msg);
        let vec = vec![hdr.clone().to_vec()];
        let rmsg = ResyncMsg::new(&vec);
        let encoded_resync_msg: Vec<u8> = rmsg.encode();
        let messages = message_decode(BytesMut::from(encoded_resync_msg), BytesMut::from(hdrkey));
        assert_eq!(1, messages.len());
        let mut message = messages[0].clone();
        let msg = message.split_off(HDRKEYL);
        let decoded_msg = Msg::decode(msg.as_ref());
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
    }

    #[test]
    fn test_message_resync_multi_decode() {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let mut keys = Vec::new();
        keys.push(uid.clone());
        keys.push(channel.clone());
        let key = MsgHdr::do_hash(&keys);
        let mut hdr: BytesMut = write_hdr(122, MsgHdr::select_cid(key));
        let keyhdr: BytesMut = write_key(key);
        hdr.extend(keyhdr);
        let hdrkey = hdr.clone().to_vec();
        let msg = "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode();
        hdr.extend(encoded_msg);
        let vec = vec![hdr.clone().to_vec(), hdr.clone().to_vec()];
        let rmsg = ResyncMsg::new(&vec);
        let encoded_resync_msg: Vec<u8> = rmsg.encode();
        let messages = message_decode(BytesMut::from(encoded_resync_msg), BytesMut::from(hdrkey));
        assert_eq!(2, messages.len());
        let mut message = messages[0].clone();
        let msg = message.split_off(HDRKEYL);
        let decoded_msg = Msg::decode(msg.as_ref());
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
        let mut message = messages[1].clone();
        let msg = message.split_off(HDRKEYL);
        let decoded_msg = Msg::decode(msg.as_ref());
        assert_eq!(decoded_msg.uid, orig_msg.uid);
        assert_eq!(decoded_msg.channel, orig_msg.channel);
        assert_eq!(decoded_msg.message, orig_msg.message);
    }
}
