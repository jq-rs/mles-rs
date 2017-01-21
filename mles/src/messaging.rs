extern crate serde_cbor;

#[derive(Serialize, Deserialize, Debug)]
pub struct Msg {
    pub message: Vec<String>,
}

pub fn message_encode(msg: &Msg) -> Vec<u8> {
    let encoded = serde_cbor::to_vec(msg);
    match encoded {
        Ok(encoded) => encoded,
        Err(err) => {
            println!("Error on encode: {}", err);
            Vec::new()
        }
    }
}

pub fn message_decode(slice: &[u8]) -> Msg {
    let value = serde_cbor::from_slice(slice);
    match value {
        Ok(value) => value,
        Err(err) => {
            println!("Error on decode: {}", err);
            Msg { message: Vec::new() } // return empty vec in case of error
        }
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_msg() {
        let msg = "Message inside to vectors".to_string();
        let orig_msg = msg.clone();
        let hold = vec![msg];
        let vectors = Msg { message: hold };
        let cbor_msg = message_encode(&vectors);
        let decoded_msg = message_decode(&cbor_msg);
        println!("Orig msg {}", orig_msg);
        println!("Cbor {:?}", cbor_msg);
        for decoded_msg in &value {
            println!("Decoded msg {}", decoded_msg);
            assert_eq!(orig_msg.as_str(), decoded_msg);
        }
    }
}

