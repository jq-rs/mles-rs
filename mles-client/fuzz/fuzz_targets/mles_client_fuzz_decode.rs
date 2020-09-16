#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;
use bytes::{BytesMut, BufMut};

mod codecs;
use crate::codecs::msg_decode;

fuzz_target!(|data: &[u8]| {
    // fuzzed code goes here
    let mut buf = BytesMut::with_capacity(data.len());
    buf.put_slice(data);
    let _ = msg_decode(&mut buf);
});
