/**
 *   Mles asynchronous server
 *   Copyright (C) 2017  Mles developers
 *
 *   This program is free software: you can redistribute it and/or modify
 *   it under the terms of the GNU General Public License as published by
 *   the Free Software Foundation, either version 3 of the License, or
 *   (at your option) any later version.
 *
 *   This program is distributed in the hope that it will be useful,
 *   but WITHOUT ANY WARRANTY; without even the implied warranty of
 *   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *   GNU General Public License for more details.
 *
 *   You should have received a copy of the GNU General Public License
 *   along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */
extern crate tokio_core;
extern crate futures;
extern crate mles_utils;

//use std::{thread, process, env};
//use std::sync::mpsc::{Sender, Receiver};
//use std::sync::mpsc::channel;
//use std::net::TcpStream;
//use std::net::{IpAddr, Ipv4Addr, SocketAddr};
//use std::net::TcpListener;
//use std::collections::HashMap;
//use std::io::{Read, Error};
//use std::io::Write;
//use std::time::Duration;
//use std::option::Option;
//use std::str;
//use mles_utils::*;

//const HDRL: u64 = 4;
//const KEYL: u64 = 8;
//const DELAY: u64 = 50;
//
//fn main_old() {
//    let mut peer = "".to_string();
//    let mut argcnt = 0;
//    for arg in env::args() {
//        argcnt += 1;
//        if 2 == argcnt {
//            peer = arg;
//            peer += ":8077";
//            match peer.parse::<SocketAddr>() {
//                Ok(_) => {},
//                    Err(err) => {
//                        println!("Error: {}\nUsage: mles [peer-address]", err);
//                        process::exit(1);
//                    },
//            }
//        }
//        if argcnt > 2 {
//            println!("Usage: mles [peer-address]");
//            process::exit(1);
//        }
//    }
//    let peer = match peer.parse::<SocketAddr>() {
//        Ok(addr) => addr,
//        Err(_) => {
//            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)
//        },
//    };
//
//    let address = "0.0.0.0:8077";
//    let listener = match TcpListener::bind(&address) {
//        Ok(listener) => listener,
//        Err(err) => {
//            println!("Error: {}", err);
//            process::exit(1);
//        },
//    };
//
//    let keyval =match env::var("MLES_KEY") {
//        Ok(val) => val,
//        Err(_) => "".to_string(),
//    };
//
//    let mut spawned = HashMap::new();
//    let option: Option<Duration> = Some(Duration::from_millis(DELAY));
//    let addr = listener.local_addr().unwrap();
//    println!("Listening for connections on {}", addr);
//    let (tx, rx) = channel();
//    let (removedtx, removedrx) = channel();
//
//    for socket in listener.incoming() {
//        /* Check first has anybody removed channels */
//        let mut remrx = true;
//        while remrx {
//            match removedrx.try_recv() {
//                Ok(val) => { 
//                    let removed_channel: String = val;
//                    println!("Removing unused channel {}", removed_channel.as_str());
//                    spawned.remove(&removed_channel);
//                    remrx = true;
//                },
//                Err(_) => { remrx = false; }
//            }
//        }
//        /* 1. Read incoming msg channel 
//         * 2. If it does not exist, spawn new thread 
//         * 3. Send socket to thread
//         */
//        let socket = socket.unwrap();
//        let stream = socket.try_clone().unwrap();
//        let tuple = read_n(&stream, HDRL);
//        let status = tuple.0;
//        match status {
//            Ok(0) => {
//               continue;
//            },
//            Ok(_) => {},
//            _ => {
//               continue;
//            },
//        }
//        let buf = tuple.1;
//        if 0 == buf.len() {
//            continue;
//        }
//        if read_hdr_type(buf.as_slice()) != 'M' as u32 {
//            println!("Incorrect payload type");
//            continue;
//        }
//        // read key
//        let tuple = read_n(&stream, KEYL);
//        let status = tuple.0;
//        match status {
//            Ok(0) => {
//               continue;
//            },
//            Ok(_) => {},
//            _ => {
//               continue;
//            },
//        }
//        // verify key
//        let hkey;
//        let key = read_key(tuple.1);
//        if 0 == keyval.len() {
//            let paddr = match stream.peer_addr() {
//                Ok(paddr) => paddr,
//                Err(_) => {
//                    let addr = "0.0.0.0:0";
//                    let addr = addr.parse::<SocketAddr>().unwrap();
//                    addr
//                }
//            };
//            hkey = do_hash(&paddr);
//        }
//        else {
//            hkey = do_hash(&keyval);
//        }
//        if hkey != key {
//            println!("Incorrect remote key");
//            continue;
//        }
//        let tuple = read_n(&stream, read_hdr_len(buf.as_slice()) as u64);
//        let status = tuple.0;
//        match status {
//            Ok(0) => {
//               continue;
//            },
//            Ok(_) => {},
//            _ => {
//               continue;
//            },
//        }
//        let buf = tuple.1;
//        let decoded_msg = message_decode(buf.as_slice());
//        if 0 == decoded_msg.channel.len() {
//            continue;
//        }
//        let _val = socket.set_nodelay(true);
//        let _val = socket.set_read_timeout(option);
//        if !spawned.contains_key(decoded_msg.channel.as_str()) { 
//            let tx = tx.clone();
//            let removedtx = removedtx.clone();
//            let msg = decoded_msg.clone();
//            thread::spawn(move|| process_channel(peer, key, tx, removedtx, msg));
//
//            let thr_feed = rx.recv().unwrap();
//            spawned.insert(decoded_msg.channel.clone(), thr_feed);
//        }
//        let thr_socket = spawned.get_mut(&decoded_msg.channel).unwrap();
//        thr_socket.send(socket).unwrap();
//    }
//}
//
//fn process_channel(peer: SocketAddr, key: u64, tx: Sender<Sender<TcpStream>>, removedtx: Sender<String>, msg: Msg ) {
//    let mut cnt = 0;
//    let (thr_tx, thr_rx): (Sender<TcpStream>, Receiver<TcpStream>) = channel();
//    println!("Spawned: New channel {} created!", msg.channel);
//    let mut users = HashMap::new();
//    let mut messages: Vec<Vec<u8>> = Vec::new();
//    tx.send(thr_tx.clone()).unwrap();
//
//    // just try once during channel creation
//    if 0 != peer.port() {
//        match TcpStream::connect(peer) {
//            Ok(mut sock) => {
//                let option: Option<Duration> = Some(Duration::from_millis(DELAY));
//                let _val = sock.set_nodelay(true);
//                let _val = sock.set_read_timeout(option);
//                let encoded_msg = message_encode(&msg);
//                let keyv = write_key(key);
//                let mut msgv = write_hdr(encoded_msg.len());
//                msgv.extend(keyv);
//                msgv.extend(encoded_msg);
//                sock.write(msgv.as_slice()).unwrap();
//
//                cnt += 1;
//                println!("Adding peer {}", cnt);
//                users.insert(cnt, sock);
//            },
//            Err(_) => {
//                println!("Could not connect to peer {}", peer);
//            },
//        }
//    }
//
//    loop {
//        let mut removals = Vec::new();
//        let mut newuser = true;
//        while newuser {
//            match thr_rx.try_recv() {
//                Ok(val) => { 
//                    let mut thr: TcpStream = val;
//                    cnt += 1;
//                    println!("Adding user {}", cnt);
//                    users.insert(cnt, thr.try_clone().unwrap());
//
//                    /* If a new user, all push messages to her */
//                    for buf in &messages {
//                        thr.write(buf.as_slice()).unwrap();
//                    }
//                    newuser = true;
//                },
//                Err(_) => { newuser = false; }
//            }
//        }
//        for (user, thr_socket) in &users {
//            let stream = thr_socket.try_clone().unwrap();
//            let tuple = read_n(&stream, HDRL);
//            let status = tuple.0;
//            match status {
//                Ok(0) => {
//                    removals.push(user.clone());
//                },
//                    _ => {}
//            }
//            let mut buf = tuple.1;
//            if 0 == buf.len() {
//                continue;
//            }
//            if read_hdr_type(buf.as_slice()) != 'M' as u32 {
//                continue;
//            }
//            let hdr_len = read_hdr_len(buf.as_slice()) as u64;
//            if 0 == hdr_len {
//                continue;
//            }
//            // read key
//            let tuple = read_n(&stream, KEYL);
//            let status = tuple.0;
//            match status {
//                Ok(0) => {
//                    continue;
//                },
//                    Ok(_) => {},
//                    _ => {
//                        continue;
//                    },
//            }
//            let key = tuple.1;
//            //ignore key 
//            buf.extend(key);
//            let tuple = read_n(&stream, hdr_len);
//            let status = tuple.0;
//            match status {
//                Ok(0) => {
//                    removals.push(user.clone());
//                },
//                    _ => {}
//            }
//            let payload = tuple.1;
//            if payload.len() != (hdr_len as usize) {
//                continue;
//            }
//            buf.extend(payload);
//            for (another_user, mut thr_sock) in &users {
//                if user != another_user {
//                    thr_sock.write(buf.as_slice()).unwrap();
//                }
//            }
//            /* Add to local db */
//            messages.push(buf);
//        }
//        for removal in &removals {
//            println!("Removing user {}", removal);
//            users.remove(removal);
//        }
//        if cnt > 0 && users.is_empty() {
//            removedtx.send(msg.channel).unwrap();
//            break;
//        }
//    }
//}

//fn read_n<R>(reader: R, bytes_to_read: u64) -> (Result<usize, Error>, Vec<u8>)
//where R: Read,
//{
//    let mut buf = vec![];
//    let mut chunk = reader.take(bytes_to_read);
//    let status = chunk.read_to_end(&mut buf);
//    (status, buf)
//}

/* Tokio version */
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::env;
use std::io::{Error, ErrorKind};
use std::thread;

use tokio_core::net::TcpListener;
use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;
use tokio_core::io::{self, Io};

use futures::{Sink, Future};
use futures::stream::{self, Stream};
use futures::sync::mpsc::{channel, Sender, Receiver};
use mles_utils::*;

//static mut BUF: Vec<u8> = vec![0,4];

fn main() {
    let addr = env::args().nth(1).unwrap_or("0.0.0.0:8077".to_string());
    let addr = addr.parse().unwrap();

    // Create the event loop and TCP listener we'll accept connections on.
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let socket = TcpListener::bind(&addr, &handle).unwrap();
    println!("Listening on: {}", addr);

    let spawned = Rc::new(RefCell::new(HashMap::new()));  
    //let (remtx, remrx) = futures::sync::mpsc::unbounded();

    let srv = socket.incoming().for_each(move |(stream, addr)| {
        println!("New Connection: {}", addr);
        let (reader, _) = stream.split();

        let spawned_inner = spawned.clone();
        let (tx, rx) = channel(0);
        //let tx_inner = tx.clone();

        // Read a off header the socket, failing if we're at EOF
        let frame = io::read_exact(reader, vec![0;4]);
        let frame = frame.and_then(move |(reader, payload)| {
            if payload.len() == 0 {
                Err(Error::new(ErrorKind::BrokenPipe, "broken pipe"))
            } else {
                if read_hdr_type(payload.as_slice()) != 'M' as u32 {
                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header"));
                }
                let hdr_len = read_hdr_len(payload.as_slice());
                if 0 == hdr_len {
                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header len"));
                }
                Ok((reader, hdr_len))
            }
        });

        let frame = frame.and_then(move |(reader, hdr_len)| {
            //dummy read key
            let tframe = io::read_exact(reader, vec![0;8]);
            let tframe = tframe.and_then(move |(reader, _)| {
                Ok((reader, hdr_len))
            });
            tframe
        });

        let frame = frame.and_then(move |(reader, hdr_len)| {
            println!("Hdrlen {}", hdr_len);
            let tframe = io::read_exact(reader, vec![0;hdr_len]);
            let tframe = tframe.and_then(move |(reader, payload)| {
                Ok((reader, payload))
            });
            tframe
        });
        

        let spawned = spawned_inner.clone();
        let socket_reader = frame.map(move |(reader, message)| {
            let decoded_message = message_decode(message.as_slice());
            println!("User {}, Channel {}", decoded_message.uid, decoded_message.channel);
            let mut spawned = spawned.borrow_mut();
            if !spawned.contains_key(decoded_message.channel.as_str()) { 
                let tx = tx.clone();
                //let remtx = remtx.clone();
                let msg = decoded_message.clone();
                thread::spawn(move|| process_tokio_channel(tx, msg));
                let thr_feed = rx.and_then(|thr_feed| {
                    spawned.insert(decoded_message.channel.clone(), thr_feed);
                    println!("Here");
                    Ok(())
                });
                //spawned.insert(decoded_message.channel.clone(), 1);
            }
            ()
        });


        socket_reader.then(move |_| {
            println!("Connection {} closed.", addr);
            Ok(())
        })
    });

    // execute server
    core.run(srv).unwrap();
}

fn process_tokio_channel( tx: futures::sync::mpsc::Sender<futures::sync::mpsc::Sender<TcpStream>>, msg: Msg ) {
//fn process_tokio_channel( msg: Msg ) {
    // Create the event loop 
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let mut cnt = 0;
    let (thr_tx, thr_rx): (Sender<TcpStream>, Receiver<TcpStream>) = channel(0);
    println!("Here we are at new channel {}", msg.channel);
    tx.send(thr_tx.clone()).wait().unwrap();

    // This is a single-threaded server, so we can just use Rc and RefCell to
    // store the map of all connections we know about.
    //let connections = Rc::new(RefCell::new(HashMap::new()));

    //// Create a channel for our stream, which other sockets will use to
    //// send us messages. Then register our address with the stream to send
    //// data to us.
    //let (tx, rx) = futures::sync::mpsc::unbounded();
    //cnt += 1;
    //connections.borrow_mut().insert(cnt, tx);

    //let srv = socket.incoming().for_each(move |(stream, addr)| {
    //    println!("New Connection: {}", addr);
    //    let (reader, writer) = stream.split();

    //    // Read a off header the socket, failing if we're at EOF
    //    let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
    //    let socket_reader = iter.fold(reader, move |reader, _| {
    //        let frame = io::read_exact(reader, vec![0;4]);
    //        let frame = frame.and_then(move |(reader, payload)| {
    //            if payload.len() == 0 {
    //                Err(Error::new(ErrorKind::BrokenPipe, "broken pipe"))
    //            } else {
    //                if read_hdr_type(payload.as_slice()) != 'M' as u32 {
    //                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header"));
    //                }
    //                let hdr_len = read_hdr_len(payload.as_slice());
    //                if 0 == hdr_len {
    //                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header len"));
    //                }
    //                Ok((reader, hdr_len))
    //            }
    //        });

    //        let frame = frame.and_then(move |(reader, hdr_len)| {
    //            //dummy read key
    //            let tframe = io::read_exact(reader, vec![0;8]);
    //            let tframe = tframe.and_then(move |(reader, _)| {
    //                Ok((reader, hdr_len))
    //            });
    //            tframe
    //        });

    //        let frame = frame.and_then(move |(reader, hdr_len)| {
    //            println!("Hdrlen {}", hdr_len);
    //            let tframe = io::read_exact(reader, vec![0;hdr_len]);
    //            let tframe = tframe.and_then(move |(reader, payload)| {
    //                let decoded_message = message_decode(payload.as_slice());
    //                println!("Message {:?}", decoded_message);
    //                Ok((reader, message_encode(&decoded_message)))
    //            });
    //            tframe
    //        });

    //        // Define here what we do for the actual I/O. That is, read a bunch of
    //        // frames from the socket and dispatch them while we also write any frames
    //        // from other sockets.
    //        let connections_inner = connections.clone();

    //        let mut conns = connections.borrow_mut();
    //        let iter = conns.iter_mut()
    //            .filter(|&(&k, _)| k != cnt)
    //            .map(|(_, v)| v);
    //        for tx in iter {
    //            println!("Sending {}: {:?}", cnt, message);
    //            tx.send(message.clone()).unwrap();
    //        }
    //        reader
    //    })
    //});

    //// Whenever we receive a string on the Receiver, we write it to
    //// `WriteHalf<TcpStream>`.
    //let socket_writer = rx.fold(writer, |writer, msg| {
    //    let amt = io::write_all(writer, msg);
    //    let amt = amt.map(|(writer, _)| writer);
    //    amt.map_err(|_| ())
    //});

    //// Now that we've got futures representing each half of the socket, we
    //// use the `select` combinator to wait for either half to be done to
    //// tear down the other. Then we spawn off the result.
    //let connections = connections.clone();
    //let socket_reader = socket_reader.map_err(|_| ());
    //let connection = socket_reader.map(|_| ()).select(socket_writer.map(|_| ()));
    //handle.spawn(connection.then(move |_| {
    //    connections.borrow_mut().remove(&cnt);
    //    println!("Connection {} closed.", addr);
    //    Ok(())
    //}));

    // execute server
    //core.run(srv).unwrap();

}
