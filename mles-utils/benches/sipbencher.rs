#![feature(test)]
extern crate test;
extern crate mles_utils;

#[cfg(test)]
mod tests {
    use test::Bencher;
    use mles_utils::*;

    #[bench]
    fn bench_encode_key_from_string(b: &mut Bencher) {
        let mut vec = Vec::new();
        let addr = "127.0.0.1:8077".to_string();
        vec.push(addr);
        b.iter(|| do_hash(&vec));
    }

    #[bench]
    fn bench_encode_key_from_vec(b: &mut Bencher) {
        let mut vec = Vec::new();
        for val in 0..100 {
            let addr = "127.0.0.1:".to_string() + &val.to_string();
            vec.push(addr);
        }
        b.iter(|| do_hash(&vec));
    }
}
 
