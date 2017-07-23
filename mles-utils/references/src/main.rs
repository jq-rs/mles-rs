/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/. 
*
*  Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
* */
extern crate mles_utils;

use std::thread;
use std::time::Duration;
use std::net::{IpAddr, Ipv4Addr};
use std::net::{SocketAddr, ToSocketAddrs};

use mles_utils::*;

fn main() {
    let sec = Duration::new(1,0);
    let raddr = "127.0.0.1:8077";
    let raddr: Vec<_> = raddr.to_socket_addrs()
        .unwrap_or_else(|_| vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)].into_iter())
        .collect();
    let raddr = *raddr.first().unwrap();
    let raddr = Some(raddr).unwrap();
    let uid = "User".to_string();
    let channel = "Channel".to_string();
    let message = "Hello World!".to_string();
    let mut childs = Vec::new();

    //create server
    let server = thread::spawn(|| server_run(":8077", "".to_string(), "".to_string(), None, 0));
    thread::sleep(sec);

    for _ in 0..2 {
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

    let _send = thread::spawn(move || {
        let raddr = raddr.clone();
        //send hello world
        let mut conn = MsgConn::new(uid.clone(), channel.clone());
        conn = conn.connect_with_msg(raddr, message.into_bytes());
        conn.close();
    });

    for _ in 0..2 {
        let child = childs.pop().unwrap();
        let _res = child.join();
    }

    //drop server
    drop(server);
}



