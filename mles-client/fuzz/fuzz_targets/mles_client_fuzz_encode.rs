#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;
use bytes::BytesMut;

mod codecs;
use crate::codecs::msg_encode;

fuzz_target!(|data: &[u8]| {
    // fuzzed code goes here
    let mut buf = BytesMut::new();
    let _ = msg_encode(data.to_vec(), &mut buf);
});
