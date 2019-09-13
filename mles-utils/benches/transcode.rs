#![feature(test)]
extern crate mles_utils;
extern crate test;

#[cfg(test)]
mod tests {
    use mles_utils::*;
    use test::Bencher;

    #[bench]
    fn bench_encode_msg(b: &mut Bencher) {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let msg = "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        b.iter(|| orig_msg.encode(&orig_msg));
    }

    #[bench]
    fn bench_decode_msg(b: &mut Bencher) {
        let uid = "User".to_string();
        let channel = "Channel".to_string();
        let msg = "a test msg".to_string().into_bytes();
        let orig_msg = Msg::new(uid, channel, msg);
        let encoded_msg = orig_msg.encode(&orig_msg);
        b.iter(|| encoded_msg.decode(&encoded_msg));
    }
}
