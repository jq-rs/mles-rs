#![feature(proc_macro)]
#[macro_use]
extern crate serde_derive;

extern crate futures;
extern crate tokio_core;

use std::thread;
use std::sync::mpsc::channel;
//use futures::stream::Stream;
//use tokio_core::reactor::Core;
//use tokio_core::net::TcpListener;
use std::net::TcpListener;
use std::collections::HashMap;
use std::io::Write;
//use futures::Future;
use std::sync::{Arc, Mutex};

mod userchannel;
mod messaging;

fn main() {
    let address = "127.0.0.1:8080";
    let listener = TcpListener::bind(&address).unwrap();
    let mut cnt = 0;

    let addr = listener.local_addr().unwrap();
    println!("Listening for connections on {}", addr);
    let (tx, rx) = channel();

    let mut channel_db = userchannel::ChannelDb{ channelname: "Rust".to_string(), users: HashMap::new(), values: Vec::new() };

    for socket in listener.incoming() {
        let mut thr_feed;
        cnt += 1;
        if(1 == cnt) {
            let tx = tx.clone();
            thread::spawn(move|| {
                let (thr_tx, thr_rx) = channel();
                println!("Spawned: Sending socket");
                loop {
                    tx.send(thr_tx.clone()).unwrap();
                    let mut thr_sock: std::net::TcpStream = thr_rx.recv().unwrap();
                    println!("Got thr sock");
                    thr_sock.write(b"Hello World\r\n").unwrap();
                }
            });
        }
        thr_feed = rx.recv().unwrap();
        println!("Got thr_feed");
        thr_feed.send(socket.unwrap()).unwrap();
    }
}
