/**
 *   Mles Asynchronous Server
 *
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

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::io::{Error, ErrorKind};
use std::{process, env};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tokio_core::net::TcpListener;
//use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;
use tokio_core::io::{self, Io};

use futures::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::unbounded;
use mles_utils::*;

const HDRL: usize = 4; //hdr len
const KEYL: usize = 8; //key len

fn main() {
    let mut peer = "".to_string();
    let mut argcnt = 0;
    for arg in env::args() {
        argcnt += 1;
        if 2 == argcnt {
            peer = arg;
            peer += ":8077";
            match peer.parse::<SocketAddr>() {
                Ok(_) => {},
                    Err(err) => {
                        println!("Error: {}\nUsage: mles [peer-address]", err);
                        process::exit(1);
                    },
            }
        }
        if argcnt > 2 {
            println!("Usage: mles [peer-address]");
            process::exit(1);
        }
    }

    /* TODO: add peer connection */
    let _peer = match peer.parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_) => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)
        },
    };

    let address = "0.0.0.0:8077";
    let address = address.parse().unwrap();

    let keyval = match env::var("MLES_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let socket = match TcpListener::bind(&address, &handle) {
        Ok(listener) => listener,
        Err(err) => {
            println!("Error: {}", err);
            process::exit(1);
        },
    };
    println!("Listening on: {}", address);

    let spawned = Rc::new(RefCell::new(HashMap::new()));  
    let channelmsgs = Rc::new(RefCell::new(HashMap::new()));  
    let mut cnt = 0;

    let srv = socket.incoming().for_each(move |(stream, addr)| {
        println!("New Connection: {}", addr);
        let paddr = match stream.peer_addr() {
                Ok(paddr) => paddr,
                Err(_) => {
                    let addr = "0.0.0.0:0";
                    let addr = addr.parse::<SocketAddr>().unwrap();
                    addr
                }
        };

        let (reader, writer) = stream.split();

        let (tx, rx) = unbounded();
        cnt += 1;

        let frame = io::read_exact(reader, vec![0;HDRL]);
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

        let paddr_inner = paddr.clone();
        let keyval_inner = keyval.clone();
        let frame = frame.and_then(move |(reader, hdr_len)| {
            let tframe = io::read_exact(reader, vec![0;KEYL]);
            // verify key
            let tframe = tframe.and_then(move |(reader, key)| {
                let hkey;
                let key = read_key(key);
                if 0 == keyval_inner.len() {
                    hkey = do_hash(&paddr_inner);
                }
                else {
                    hkey = do_hash(&keyval_inner);
                }
                if hkey != key {
                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect remote key"));
                }
                Ok((reader, hdr_len))
            });
            tframe
        });

        let tx_once = tx.clone();
        let spawned_inner = spawned.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let socket_once = frame.and_then(move |(reader, hdr_len)| {
            let tframe = io::read_exact(reader, vec![0;hdr_len]);
            let tframe = tframe.and_then(move |(reader, message)| {
                if 0 == message.len() { 
                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"));
                }
                else {
                    let mut spawned_once = spawned_inner.borrow_mut();
                    let mut chanmsgs_once = chanmsgs_inner.borrow_mut();
                    let decoded_message = message_decode(message.as_slice());
                    let channel = decoded_message.channel.clone();
                    if !spawned_once.contains_key(&channel) {
                        let mut channel_entry = HashMap::new();
                        channel_entry.insert(cnt, tx_once);
                        spawned_once.insert(channel.clone(), channel_entry);

                        let messages: Vec<Vec<u8>> = Vec::new();
                        chanmsgs_once.insert(channel.clone(), messages);
                    }
                    else {
                        let mut channel_entry = spawned_once.get_mut(&channel).unwrap();
                        channel_entry.insert(cnt, tx_once);
                        let chanmsgs = chanmsgs_once.get(&channel).unwrap();
                        for msg in chanmsgs {
                            tx.send(msg.clone()).unwrap();
                        }
                    }
                    println!("User {}:{} joined channel {}", cnt, decoded_message.uid, channel);
                    /* TODO: add peer connection */
                    Ok((reader, channel))
                }
            });
            tframe
        });

        let spawned_inner = spawned.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let socket_next = socket_once.and_then(move |(reader, channel)| {
            let channel_next = channel.clone();
            let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;HDRL]);
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
                        Ok((reader, payload, hdr_len))
                    }
                });

                let frame = frame.and_then(move |(reader, hdr, hdr_len)| {
                    //dummy read key
                    let tframe = io::read_exact(reader, vec![0;KEYL]);
                    let tframe = tframe.and_then(move |(reader, key)| {
                        Ok((reader, hdr, key, hdr_len))
                    });
                    tframe
                });

                let frame = frame.and_then(move |(reader, hdr, key, hdr_len)| {
                    let tframe = io::read_exact(reader, vec![0;hdr_len]);
                    let tframe = tframe.and_then(move |(reader, message)| {
                        if 0 == message.len() { 
                            return Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"));
                        }
                        else {
                            Ok((reader, hdr, key, message))
                        }
                    });
                    tframe
                });

                let spawned = spawned_inner.clone();
                let chanmsgs = chanmsgs_inner.clone();
                let channel = channel_next.clone();
                frame.map(move |(reader, mut hdr, mut key, message)| {
                    key.extend(message);
                    hdr.extend(key);

                    // add to history
                    let mut channel_msgs = chanmsgs.borrow_mut();
                    let mut channel_msg = channel_msgs.get_mut(&channel).unwrap();
                    channel_msg.push(hdr.clone());

                    //distribute
                    let spawned = spawned.borrow();
                    let channels = spawned.get(&channel).unwrap();
                    for (ocnt, tx) in channels {
                        if *ocnt != cnt {
                            tx.send(hdr.clone()).unwrap();
                        }
                    }
                    reader
                })
            })
        });

        let socket_writer = rx.fold(writer, |writer, msg| {
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });

        let channels = spawned.clone();
        let chanmsgs = channelmsgs.clone();
        let socket_reader = socket_next.map_err(|_| ());
        let connection = socket_reader.map(|_| ()).select(socket_writer.map(|_| ()));
        handle.spawn(connection.then(move |_| {
            let mut chans = channels.borrow_mut();
            for (cname, channel) in chans.iter_mut() {
                if channel.contains_key(&cnt) {
                    channel.remove(&cnt);
                    if 0 == channel.len() {
                        let mut channelmsgs = chanmsgs.borrow_mut();
                        channelmsgs.remove(cname);
                    }
                    if 0 == channel.len() {
                        println!("Channel {} dropped.", cname);
                        drop(channel);
                    }
                    break;
                }
            }
            println!("Connection {} for user {} closed.", addr, cnt);
            Ok(())
        }));
        Ok(())
    });

    // execute server
    core.run(srv).unwrap();
}


