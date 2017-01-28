extern crate mles_utils;

use std::thread;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc::channel;
use std::net::TcpStream;
use std::net::TcpListener;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;
use std::option::Option;
use std::str;
use mles_utils::*;


fn process_channel(tx: Sender<Sender<TcpStream>>, removedtx: Sender<String>, this_channel: String ) {
    let mut cnt = 0;
    let (thr_tx, thr_rx): (Sender<TcpStream>, Receiver<TcpStream>) = channel();
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
                    println!("Adding user {}", cnt);
                    users.insert(cnt, thr.try_clone().unwrap());

                    /* If a new user, all push messages to her */
                    for buf in &messages {
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
            if read_hdr_type(buf.as_slice()) != 'M' as u32 {
                continue;
            }
            let hdr_len = read_hdr_len(buf.as_slice()) as u64;
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
            if payload.len() != (hdr_len as usize) {
                continue;
            }
            buf.extend(payload);
            for (another_user, mut thr_sock) in &users {
                if user != another_user {
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
}

fn main() {
    let address = "0.0.0.0:8081";
    let listener = TcpListener::bind(&address).unwrap();
    let mut spawned = HashMap::new();
    let option: Option<Duration> = Some(Duration::from_millis(50));

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
        let socket = socket.unwrap();
        let stream = socket.try_clone().unwrap();
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
        let decoded_msg = message_decode(buf.as_slice());
        if 0 == decoded_msg.channel.len() {
            continue;
        }
        socket.set_nodelay(true);
        socket.set_read_timeout(option);
        if !spawned.contains_key(decoded_msg.channel.as_str()) { 
            let tx = tx.clone();
            let removedtx = removedtx.clone();
            let this_channel = decoded_msg.channel.clone();
            thread::spawn(move|| process_channel(tx, removedtx, this_channel));

            let thr_feed = rx.recv().unwrap();
            println!("Channel {}", decoded_msg.channel.as_str());
            spawned.insert(decoded_msg.channel.clone(), thr_feed);
        }
        let thr_socket = spawned.get_mut(&decoded_msg.channel).unwrap();
        thr_socket.send(socket).unwrap();
    }
}
