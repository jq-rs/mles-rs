#![no_main]
use libfuzzer_sys::fuzz_target;
use mles_utils::MsgHdr;
use mles_utils::Msg;

fuzz_target!(|data: &[u8]| {
    // fuzzed code goes here
    let _ = MsgHdr::decode(data.to_vec());
    let _ = Msg::decode(data);
});
