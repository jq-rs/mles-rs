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
    let mut cnt = 0;
    let option: Option<Duration> = Some(Duration::from_millis(50));

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
<<<<<<< HEAD
        let mut buf = vec![0;80];
        let len = stream.read(&mut buf);
        let decoded_msg = messaging::message_decode(&buf);

        socket.set_nodelay(true);
=======
        let len = stream.read_line(&mut result_str);
        //println!("Got string {}", result_str);
>>>>>>> e321903f944f5113a4eca4fbb121c6f099f96ede
        socket.set_read_timeout(option);
        if !spawned.contains_key(&decoded_msg.message[1]) { 
            let tx = tx.clone();
            thread::spawn(move|| {
                let (thr_tx, thr_rx) = channel();
                println!("Spawned: New channel created!");
                let mut users = HashMap::new();
                tx.send(thr_tx.clone()).unwrap();
                //println!("Got thr sock");
                loop {
                    let mut removals = Vec::new();
                    match thr_rx.try_recv() {
                        Ok(val) => { 
                            let thr: TcpStream = val;
                            cnt += 1;
                            users.insert(cnt, thr);
                        },
                Err(_) => {}
                    }
                    let mut bufstr = vec![0; 80];
                    for (user, thr_socket) in &users {
                        let mut stream = BufReader::new(thr_socket.try_clone().unwrap());
                        match stream.read(&mut bufstr) {
                            Ok(len) => {
<<<<<<< HEAD
                                if len > 0 {
                                    for (another_user, mut thr_sock) in &users {
                                        if user != another_user {
                                            let mut msg = String::new();
                                            let s = str::from_utf8(bufstr.as_slice()).unwrap();
                                            //msg.push_str(":");
=======
                                if(len > 0) {
                                    println!("Len is {:?}", len);
                                    for (another_user, mut thr_sock) in &users {
                                        if user != another_user {
                                            let mut msg = user.to_string();
                                            let s = str::from_utf8(bufstr.as_slice()).unwrap();
                                            msg.push_str(":");
>>>>>>> e321903f944f5113a4eca4fbb121c6f099f96ede
                                            msg.push_str(s);
                                            println!("Msg is {:?}", s);
                                            thr_sock.write(&msg.into_bytes()).unwrap();
                                        }
                                    }
                                }
                                else {
                                    /* Socket closed, drop user */
                                    removals.push(user.clone());
                                }
                            },
<<<<<<< HEAD
                                Err(_) => {
                                    // println!("Error!");
                                }
=======
                            Err(_) => {
                                println!("Error!");
                            }
>>>>>>> e321903f944f5113a4eca4fbb121c6f099f96ede
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
<<<<<<< HEAD
            println!("Channel {}", decoded_msg.message[1]);
            spawned.insert(decoded_msg.message[1].clone(), thr_feed);
=======
            //println!("Got thr_feed");
            spawned.insert(result_str.clone(), thr_feed);
>>>>>>> e321903f944f5113a4eca4fbb121c6f099f96ede
        }
        let thr_socket = spawned.get_mut(&decoded_msg.message[1]).unwrap();
        thr_socket.send(socket).unwrap();
    }
}
