#[macro_use]
extern crate serde_derive;

extern crate futures;
extern crate tokio_core;
extern crate byteorder;

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
use std::io::Cursor;
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};


mod userchannel;
mod messaging;
use messaging::*;

fn read_n<R>(reader: R, bytes_to_read: u64) -> Vec<u8>
where R: Read,
{
    let mut buf = vec![];
    let mut chunk = reader.take(bytes_to_read);
    let status = chunk.read_to_end(&mut buf);
    match status {
        Ok(n) => assert_eq!(bytes_to_read as usize, n),
            _ => panic!("Didn't read enough"),
    }
    buf
 }

fn read_hdr_type(hdr: &[u8]) -> u32 { 
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    num >> 24
}

fn read_hdr_len(hdr: &[u8]) -> u32 { 
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    num & 0xfff
}
                              
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
        let mut stream = socket.try_clone().unwrap();
        let buf = read_n(&stream, 4);
        if read_hdr_type(buf.as_slice()) != 'M' as u32 {
            println!("Incorrect payload type");
            continue;
        }

        println!("Payload len {}", read_hdr_len(buf.as_slice()));
        let buf = read_n(&stream, read_hdr_len(buf.as_slice()) as u64);
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
                    let mut buf = vec![0; 4];
                    for (user, thr_socket) in &users {
                        let stream = thr_socket.try_clone().unwrap();
                        let buf = read_n(&stream, 4);
                        if read_hdr_type(buf.as_slice()) != 'M' as u32 {
                            removals.push(user.clone());
                            break;
                        }
                        let buf = read_n(&stream, read_hdr_len(buf.as_slice()) as u64);
                        let decoded_msg = messaging::message_decode(buf.as_slice());
                        if 0 == decoded_msg.message.len() {
                            continue;
                        }
                        for (another_user, mut thr_sock) in &users {
                            if user != another_user {
                                println!("Msg is {:?}", buf);
                                thr_sock.write(buf.as_slice()).unwrap();
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
