#[macro_use]
extern crate serde_derive;

extern crate futures;
extern crate tokio_core;

use std::thread;
use std::sync::mpsc::channel;
//use futures::stream::Stream;
//use tokio_core::reactor::Core;
//use tokio_core::net::TcpListener;
use std::net::TcpStream;
use std::net::TcpListener;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;
use std::option::Option;
//use std::io::Read;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
//use futures::Future;
use std::sync::{Arc, Mutex};
use std::io::{Error, ErrorKind};
use std::str;

mod userchannel;
mod messaging;
use messaging::*;

fn main() {
    let address = "0.0.0.0:8081";
    let listener = TcpListener::bind(&address).unwrap();
    let mut spawned = HashMap::new();
    let option: Option<Duration> = Some(Duration::from_millis(50));
    let mut cnt = 0;

    let addr = listener.local_addr().unwrap();
    println!("Listening for connections on {}", addr);
    let (tx, rx) = channel();

    for socket in listener.incoming() {
        /* 1. Read incoming msg channel 
         * 2. If it does not exist, spawn new thread 
         * 3. Send socket to thread
         */
        let mut socket = socket.unwrap();
        let mut stream = BufReader::new(socket.try_clone().unwrap());
        let mut buf = vec![0;80];
        let n = match stream.read(&mut buf) {
            Err(_) |
                Ok(0) => continue,
                Ok(n) => n,
        };
        buf.truncate(n);
        println!("Got len {}", n);
        let decoded_msg = messaging::message_decode(buf.as_slice());
        if 0 == decoded_msg.message.len() {
            continue;
        }
        socket.set_nodelay(true);
        socket.set_read_timeout(option);
        if !spawned.contains_key(&decoded_msg.message[1]) { 
            let tx = tx.clone();
            let user = decoded_msg.message[0].clone();
            println!("Got {}", user);
            thread::spawn(move|| {
                let (thr_tx, thr_rx) = channel();
                println!("Spawned: New channel created!");
                let mut users = HashMap::new();
                tx.send(thr_tx.clone()).unwrap();
                loop {
                    let mut removals = Vec::new();
                    match thr_rx.try_recv() {
                        Ok(val) => { 
                            let thr: TcpStream = val;
                            cnt += 1;
                            println!("Adding {}", cnt);
                            users.insert(cnt, thr);
                        },
                        Err(_) => {}
                    }
                    let mut buf = vec![0; 80];
                    for (user, thr_socket) in &users {
                        let mut stream = BufReader::new(thr_socket.try_clone().unwrap());
                        match stream.read(&mut buf) {
                            Ok(len) => {
                                if len > 0 {
                                    for (another_user, mut thr_sock) in &users {
                                        if user != another_user {
                                            buf.truncate(len);
                                            println!("Msg is {:?}", buf);
                                            thr_sock.write(buf.as_slice()).unwrap();
                                        }
                                    }
                                }
                                else {
                                    /* Socket closed, drop user */
                                    removals.push(user.clone());
                                }
                            },
                                Err(_) => {
                                    // println!("Error!");
                                }
                        }
                    }
                    for removal in &removals {
                        users.remove(removal);
                    }
                    if cnt > 0 && users.is_empty() {
                        break;
                    }
                }
            });
            let thr_feed = rx.recv().unwrap();
            println!("Channel {}", decoded_msg.message[1]);
            spawned.insert(decoded_msg.message[1].clone(), thr_feed);
        }
        let thr_socket = spawned.get_mut(&decoded_msg.message[1]).unwrap();
        thr_socket.send(socket).unwrap();
    }
}
