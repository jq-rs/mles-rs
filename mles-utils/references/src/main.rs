/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/. 
*
*  Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
* */
extern crate mles_utils;

use std::thread;
use std::time::{Duration, Instant};
use std::net::SocketAddr;

use mles_utils::*;

fn main() {
    let sec = Duration::new(2,0);
    let addr = "127.0.0.1:8077";
    let addr = addr.parse::<SocketAddr>().unwrap();
    let raddr = addr.clone();
    let uid = "User".to_string();
    let channel = "Channel".to_string();
    let message = "Hello World!".to_string();
    let mut childs = Vec::new();
    let now;

    //create server
    let server = thread::spawn(move || server_run(addr, None, "".to_string(), "".to_string(), 0, 0));
    thread::sleep(sec);

    for _ in 0..100 {
        let uid = uid.clone();
        let channel = channel.clone();
        let child = thread::spawn(move || {
            //read hello world
            let mut conn = MsgConn::new(uid, channel);
            conn = conn.connect(raddr);
            let (conn, msg) = conn.read_message();
            let msg = String::from_utf8_lossy(msg.as_slice());
            assert_eq!("Hello World!", msg);
            //close connection
            conn.close();
        });
        childs.push(child);
    }
    thread::sleep(sec);

    //send hello world
    let mut conn = MsgConn::new(uid.clone(), channel.clone());
    now = Instant::now();
    conn = conn.connect_with_message(raddr, message.into_bytes());

    for _ in 0..100 {
        let child = childs.remove(0);
        let _res = child.join();
    }

    let endtime = now.elapsed();
    println!("{}.{:09}", endtime.as_secs(), endtime.subsec_nanos());

    conn.close();

    //drop server
    drop(server);
}



