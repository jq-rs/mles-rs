#[macro_use]
extern crate serde_derive;

<<<<<<< HEAD
=======
//! A simple client to connect Mles server based on Tokio connect.rs example.
//! 
//! This example will connect to a server specified in the argument list and
//! then forward all data read on stdin to the server, printing out all data
//! received on stdout.
//! 
//! Note that this is not currently optimized for performance, especially around
//! buffer management. Rather it's intended to show an example of working with a
//! client.
//!
>>>>>>> e321903f944f5113a4eca4fbb121c6f099f96ede

#![feature(unicode)]

extern crate futures;
extern crate tokio_core;
<<<<<<< HEAD
extern crate serde_cbor;

#[derive(Serialize, Deserialize, Debug)]
pub struct Msg {
    message: Vec<String>,
}

pub fn message_encode(msg: &Msg) -> Vec<u8> {
    let encoded = serde_cbor::to_vec(msg).unwrap();
    encoded
}

pub fn message_decode(slice: &[u8]) -> Msg {
    let value: Msg = serde_cbor::from_slice(slice).unwrap();
    value
}

=======
extern crate ncurses;
>>>>>>> e321903f944f5113a4eca4fbb121c6f099f96ede

use std::env;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::thread;

use futures::{Sink, Future, Stream};
use futures::sync::mpsc;
use tokio_core::reactor::Core;
use tokio_core::io::{Io, EasyBuf, Codec};
use tokio_core::net::TcpStream;

use std::char;
use ncurses::*;

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
            stdout.write_all(buf.as_slice())
        });

        send_stdin.map(|_| ())
        .select(write_stdout.map(|_| ()))
        .then(|_| Ok(()))
    });

    core.run(client).unwrap();
    endwin();
}

struct Bytes;

impl Codec for Bytes {
    type In = EasyBuf;
    type Out = Vec<u8>;

    fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<EasyBuf>> {
        if buf.len() > 0 {
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
<<<<<<< HEAD
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
    let userstr = String::from_utf8(buf.clone()).unwrap();

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
    let channelstr = String::from_utf8(buf.clone()).unwrap();

    let mut msg =  String::from_utf8(user).unwrap();
    msg.push_str("::");
    msg.push_str(String::from_utf8(channel.clone()).unwrap().as_str());
    let mut welcome = "Welcome to ".to_string();
    welcome.push_str(msg.as_str());
    welcome.push_str("!\n");
    stdout.write_all(welcome.as_bytes());

    let mut msgvec: Vec<String> = Vec::new();
    msgvec.push(userstr);
    msgvec.push(channelstr);

    loop {
        let mut buf = vec![0;80];
        let mut msgv: Vec<String> = msgvec.clone();
        let n = match stdin.read(&mut buf) {
            Err(_) |
                Ok(0) => break,
                Ok(n) => n,
        };
        buf.truncate(n);
        msgv.push(String::from_utf8(buf).unwrap());
        let msg = Msg { message: msgv };
        rx = rx.send(message_encode(&msg)).wait().unwrap();
=======
    let locale_conf = LcCategory::all;
    setlocale(locale_conf, "en_US.UTF-8");

    /* Setup ncurses. */
    initscr();
    raw();

    loop {
        let c = getch();
        let c = char::from_u32(c as u32).unwrap();
        let mut buf = vec![0;4];
        c.encode_utf8(&mut buf);
        buf.truncate(c.len_utf8());
        if '\n' == c {
            printw("\n");
        }
        refresh();
        rx = rx.send(buf).wait().unwrap();
>>>>>>> e321903f944f5113a4eca4fbb121c6f099f96ede
    }
}    
