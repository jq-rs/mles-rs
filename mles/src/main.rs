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

fn read_n<R>(reader: R, bytes_to_read: u64) -> (Result<usize, Error>, Vec<u8>)
where R: Read,
{
    let mut buf = vec![];
    let mut chunk = reader.take(bytes_to_read);
    let status = chunk.read_to_end(&mut buf);
    (status, buf)
 }

fn read_hdr_type(hdr: &[u8]) -> u32 { 
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    num >> 24
}

fn read_hdr_len(hdr: &[u8]) -> usize { 
    let mut buf = Cursor::new(&hdr[..]);
    let num = buf.read_u32::<BigEndian>().unwrap();
    (num & 0xfff) as usize
}

fn write_hdr(len: usize) -> Vec<u8> {
    let hdr = (('M' as u32) << 24) | len as u32;
    let mut msgv = vec![];
    msgv.write_u32::<BigEndian>(hdr).unwrap();
    msgv
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
    let (removedtx, removedrx) = channel();

    for socket in listener.incoming() {
        /* Check first has anybody removed channels */
        let mut remrx = true;
        while remrx {
            match removedrx.try_recv() {
                Ok(val) => { 
                    let removed_channel: String = val;
                    println!("Removing unused channel {}", removed_channel.as_str());
                    spawned.remove(&removed_channel);
                    remrx = true;
                },
                Err(_) => { remrx = false; }
            }
        }
        /* 1. Read incoming msg channel 
         * 2. If it does not exist, spawn new thread 
         * 3. Send socket to thread
         */
        let mut socket = socket.unwrap();
        let mut stream = socket.try_clone().unwrap();
        let tuple = read_n(&stream, 4);
        let status = tuple.0;
        match status {
            Ok(0) => {
               continue;
            },
            Ok(_) => {},
            _ => {
               continue;
            },
        }
        let buf = tuple.1;
        if 0 == buf.len() {
            continue;
        }
        if read_hdr_type(buf.as_slice()) != 'M' as u32 {
            println!("Incorrect payload type");
            continue;
        }
        println!("Payload len {}", read_hdr_len(buf.as_slice()));
        let tuple = read_n(&stream, read_hdr_len(buf.as_slice()) as u64);
        let status = tuple.0;
        match status {
            Ok(0) => {
               continue;
            },
            Ok(_) => {},
            _ => {
               continue;
            },
        }
        let buf = tuple.1;
        println!("Msgv {:?}", buf);
        let decoded_msg = messaging::message_decode(buf.as_slice());
        if 0 == decoded_msg.channel.len() {
            continue;
        }
        socket.set_nodelay(true);
        socket.set_read_timeout(option);
        if !spawned.contains_key(decoded_msg.channel.as_str()) { 
            let tx = tx.clone();
            let removedtx = removedtx.clone();
            let this_channel = decoded_msg.channel.clone();
            thread::spawn(move|| {
                let (thr_tx, thr_rx) = channel();
                println!("Spawned: New channel created!");
                let mut users = HashMap::new();
                let mut messages: Vec<Vec<u8>> = Vec::new();
                tx.send(thr_tx.clone()).unwrap();
                loop {
                    let mut removals = Vec::new();
                    let mut newuser = true;
                    while newuser {
                        match thr_rx.try_recv() {
                            Ok(val) => { 
                                let mut thr: TcpStream = val;
                                cnt += 1;
                                println!("Adding {}", cnt);
                                users.insert(cnt, thr.try_clone().unwrap());

                                /* If a new user, all push messages to her */
                                for buf in &messages {
                                    println!("Historial msg is {:?}", buf);
                                    thr.write(buf.as_slice()).unwrap();
                                }
                                newuser = true;
                            },
                            Err(_) => { newuser = false; }
                        }
                    }
                    for (user, thr_socket) in &users {
                        let stream = thr_socket.try_clone().unwrap();
                        let tuple = read_n(&stream, 4);
                        let status = tuple.0;
                        match status {
                            Ok(0) => {
                                removals.push(user.clone());
                            },
                            _ => {}
                        }
                        let mut buf = tuple.1;
                        if 0 == buf.len() {
                            continue;
                        }
                        println!("Got buf len {}", buf.len());
                        if read_hdr_type(buf.as_slice()) != 'M' as u32 {
                            continue;
                        }
                        println!("Ok hdr type");
                        let hdr_len = read_hdr_len(buf.as_slice()) as u64;
                        println!("Hdr_len {}", hdr_len);
                        if 0 == hdr_len {
                            continue;
                        }
                        let tuple = read_n(&stream, hdr_len);
                        let status = tuple.0;
                        match status {
                            Ok(0) => {
                                removals.push(user.clone());
                            },
                            _ => {}
                        }
                        let payload = tuple.1;
                        println!("Payload len {}", payload.len());
                        if payload.len() != (hdr_len as usize) {
                            continue;
                        }
                        buf.extend(payload);
                        for (another_user, mut thr_sock) in &users {
                            if user != another_user {
                                println!("Msg is {:?}", buf);
                                thr_sock.write(buf.as_slice()).unwrap();
                            }
                        }
                        /* Add to local db */
                        messages.push(buf);
                    }
                    for removal in &removals {
                        println!("Removing user {}", removal);
                        users.remove(removal);
                    }
                    if cnt > 0 && users.is_empty() {
                        removedtx.send(this_channel).unwrap();
                        break;
                    }
                }
            });
            let thr_feed = rx.recv().unwrap();
            println!("Channel {}", decoded_msg.channel.as_str());
            spawned.insert(decoded_msg.channel.clone(), thr_feed);
        }
        let thr_socket = spawned.get_mut(&decoded_msg.channel).unwrap();
        thr_socket.send(socket).unwrap();
    }
}
