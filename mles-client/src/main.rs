
//! A simple client to connect Mles server.
//! 
//! This example will connect to a server specified in the argument list and
//! then forward all data read on stdin to the server, printing out all data
//! received on stdout.
//! 
//! Note that this is not currently optimized for performance, especially around
//! buffer management. Rather it's intended to show an example of working with a
//! client.
//!

extern crate futures;
extern crate tokio_core;

use std::env;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::thread;

use futures::{Sink, Future, Stream};
use futures::sync::mpsc;
use tokio_core::reactor::Core;
use tokio_core::io::{Io, EasyBuf, Codec};
use tokio_core::net::TcpStream;

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
    let mut stdin = io::stdin();
    loop {
        let mut buf = vec![0; 80];
        let n = match stdin.read(&mut buf) {
            Err(_) |
                Ok(0) => break,
                Ok(n) => n,
        };
        buf.truncate(n);
        rx = rx.send(buf).wait().unwrap();
    }
}

