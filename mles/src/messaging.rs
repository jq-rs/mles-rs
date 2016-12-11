//#![feature(custom_derive, plugin)]
//#![plugin(serde_macros)]

extern crate serde;
extern crate serde_cbor;

//use serde_cbor::from_slice;
//use serde_cbor::to_vec;

pub fn message_encode(msg: &Vec<u8>) -> Vec<u8> {
    let encoded = serde_cbor::to_vec(msg).unwrap();
    encoded
}

pub fn message_decode(slice: &[u8]) -> String {
    let value: String = serde_cbor::from_slice(slice).unwrap();
    value
}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_msg() {
        let orig_msg = "Message".to_string().into_bytes();
        let cbor_msg = message_encode(&orig_msg);
        let decoded_msg = message_decode(&cbor_msg);
        println!("Orig msg {}", String::from_utf8(orig_msg).unwrap());
        println!("Decoded msg {}", decoded_msg);
    }
}

