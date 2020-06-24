#![no_main]
use libfuzzer_sys::fuzz_target;
use mles_utils::MsgHdr;

fuzz_target!(|data: &[u8]| {
    // fuzzed code goes here
    let mut vector: Vec<u8> = vec!['M' as u8];
    vector.extend_from_slice(data);
    let _ = MsgHdr::decode(vector);
});
