#[macro_use]
extern crate serde_derive;


extern crate futures;
extern crate tokio_core;
extern crate serde_cbor;
extern crate byteorder;

pub struct Hdr {
    payload_type: u8,
    payload_res: u8,
    payload_len: u16
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Msg {
    message: Vec<String>,
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

use std::env;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::thread;

use futures::{Sink, Future, Stream};
use futures::sync::mpsc;
use tokio_core::reactor::Core;
use tokio_core::io::{Io, EasyBuf, Codec};
use tokio_core::net::TcpStream;
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};

fn main() {
    // Parse what address we're going to connect to
    //let addr = env::args().nth(1).unwrap_or_else(|| {
    //    panic!("this program requires at least one argument")
    //});
    let addr = "127.0.0.1:8081";
    let addr = addr.parse::<SocketAddr>().unwrap();

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let tcp = TcpStream::connect(&addr, &handle);

    // Handle stdin in a separate thread 
    let (stdin_tx, stdin_rx) = mpsc::channel(0);
    thread::spawn(|| read_stdin(stdin_tx));
    let stdin_rx = stdin_rx.map_err(|_| panic!()); // errors not possible on rx

    let mut stdout = io::stdout();
    let client = tcp.and_then(|stream| {
        let (sink, stream) = stream.framed(Bytes).split();
        let send_stdin = stdin_rx.forward(sink);
        let write_stdout = stream.for_each(move |buf| {
            let decoded = message_decode(buf.as_slice());
            let mut msg = "".to_string();
            if 0 == decoded.message.len() {
                println!("Error happened");
            }
            else {
                msg.push_str(decoded.message[0].as_str());
                msg.push_str(":");
                msg.push_str(decoded.message[2].as_str());
            }
            stdout.write_all(&msg.into_bytes())
        });

        send_stdin.map(|_| ())
        .select(write_stdout.map(|_| ()))
        .then(|_| Ok(()))
    });

    core.run(client).unwrap();
}

struct Bytes;

impl Codec for Bytes {
    type In = EasyBuf;
    type Out = Vec<u8>;

    fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<EasyBuf>> {
        if buf.len() >= 4 { // 4 is header min size
            let len = buf.len();
            Ok(Some(buf.drain_to(len)))
        } else {
            Ok(None)
        }
    }

    fn encode(&mut self, data: Vec<u8>, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.extend(data);
        Ok(())
    }
}

fn read_stdin(mut rx: mpsc::Sender<Vec<u8>>) {
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    /* Set user */
    stdout.write_all(b"User name?\n");
    let mut buf = vec![0; 80];
    let n = match stdin.read(&mut buf) {
        Err(_) |
            Ok(0) => return,
            Ok(n) => n,
    };
    buf.truncate(n-1);
    let user = buf.clone();
    let userstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();

    /* Set channel */
    stdout.write_all(b"Channel?\n");
    let mut buf = vec![0; 80];
    let n = match stdin.read(&mut buf) {
        Err(_) |
            Ok(0) => return,
            Ok(n) => n,
    };
    buf.truncate(n-1);
    let channel = buf.clone();
    let channelstr = String::from_utf8_lossy(buf.clone().as_slice()).into_owned();

    let mut msg = String::from_utf8_lossy(user.as_slice()).into_owned();
    msg += "::";
    let str =  String::from_utf8_lossy(channel.clone().as_slice()).into_owned();
    msg += str.as_str();

    let mut welcome = "Welcome to ".to_string();
    welcome += msg.as_str();
    welcome += "!\n";
    stdout.write_all(welcome.as_bytes());

    let mut msgvec: Vec<String> = Vec::new();
    msgvec.push(userstr);
    msgvec.push(channelstr);
    let msg = message_encode(&Msg { message: msgvec.clone() });
    println!("Payload len {}", msg.len());
    let mut msgv = write_hdr(msg.len());
    msgv.extend(msg);
    println!("Msgv {:?}", msgv);
    rx = rx.send(msgv).wait().unwrap();

    loop {
        let mut buf = vec![0;80];
        let mut msgv: Vec<String> = msgvec.clone();
        let n = match stdin.read(&mut buf) {
            Err(_) |
                Ok(0) => break,
                Ok(n) => n,
        };
        buf.truncate(n);
        let str =  String::from_utf8_lossy(buf.as_slice()).into_owned();
        msgv.push(str);
        let msg = message_encode(&Msg { message: msgv });
        println!("Payload len {}", msg.len());
        let mut msgv = write_hdr(msg.len());
        msgv.extend(msg);
        println!("Msgv {:?}", msgv);
        rx = rx.send(msgv).wait().unwrap();
    }
}

fn write_hdr(len: usize) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let mut msgv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    msgv
}

