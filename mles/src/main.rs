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

mod userchannel;
mod messaging;

fn main() {
    let address = "127.0.0.1:8080";
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
        let mut result_str = String::new();
        let mut socket = socket.unwrap();
        let mut stream = BufReader::new(socket.try_clone().unwrap());
        let len = stream.read_line(&mut result_str);
        println!("Got string {}", result_str);
        socket.set_read_timeout(option);
        if !spawned.contains_key(&result_str) { 
            let tx = tx.clone();
            let result_str = result_str.clone();
            thread::spawn(move|| {
                let (thr_tx, thr_rx) = channel();
                println!("Spawned: Sending socket");
                let mut users = HashMap::new();
                tx.send(thr_tx.clone()).unwrap();
                println!("Got thr sock");
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
                    let mut bufstr = String::new();
                    for (user, thr_socket) in &users {
                        let mut stream = BufReader::new(thr_socket.try_clone().unwrap());
                        match stream.read_line(&mut bufstr) {
                            Ok(len) => {
                                if(len > 0) {
                                    for (another_user, mut thr_sock) in &users {
                                        if user != another_user {
                                            let mut msg = user.to_string();
                                            msg.push_str(":");
                                            msg.push_str(&bufstr);
                                            thr_sock.write(&msg.into_bytes()).unwrap();
                                        }
                                    }
                                }
                                else {
                                    /* Socket closed, drop user */
                                    removals.push(user.clone());
                                }
                            },
                            Err(_) => {
                                //println!("{:?}", err.kind());
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
            println!("Got thr_feed");
            spawned.insert(result_str.clone(), thr_feed);
        }
        let thr_socket = spawned.get_mut(&result_str).unwrap();
        thr_socket.send(socket).unwrap();
    }
}
