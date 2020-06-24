#![no_main]
use libfuzzer_sys::fuzz_target;
use mles_utils::Msg;

fuzz_target!(|data: &[u8]| {
    // fuzzed code goes here
    let _ = Msg::decode(data);
});
