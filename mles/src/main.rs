/**
 *   Mles server
 *
 *   Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
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
extern crate tokio_io;
extern crate futures;
extern crate mles_utils;

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::io::{Error, ErrorKind};
use std::{process, env};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::thread;

use tokio_core::net::TcpListener;
use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;
use tokio_io::io;
use tokio_io::AsyncRead;

use futures::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::{unbounded, UnboundedSender};
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

    let peer = match peer.parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_) => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)
        },
    };

    let keyval = match env::var("MLES_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let address = "0.0.0.0:8077".parse().unwrap();
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
    let mut peer_cnt = 0;

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
        let _val = stream.set_nodelay(true).map_err(|_| panic!("Cannot set to no delay"));;

        let (reader, writer) = stream.split();
        let (tx, rx) = unbounded();
        cnt = mles_get_cnt(cnt);

        let (tx_peer, rx_peer) = unbounded();
        peer_cnt = mles_get_peer_cnt(cnt);

        let frame = io::read_exact(reader, vec![0;HDRL]);
        let frame = frame.and_then(move |(reader, payload)| mles_process_hdr(reader, payload));

        let paddr_inner = paddr.clone();
        let keyval_inner = keyval.clone();
        let frame = frame.and_then(move |(reader, hdr, hdr_len)| {
            let tframe = io::read_exact(reader, vec![0;KEYL]);
            // verify key
            tframe.and_then(move |(reader, key)| mles_process_key(reader, hdr, key, hdr_len, keyval_inner, paddr_inner))
        });

        let tx_once = tx.clone();
        let spawned_inner = spawned.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let socket_once = frame.and_then(move |(reader, mut hdr, key, hdr_len)| {
            let tframe = io::read_exact(reader, vec![0;hdr_len]);
            tframe.and_then(move |(reader, message)| {
                if message.len() > 0 {  
                    let mut spawned_once = spawned_inner.borrow_mut();
                    let mut chanmsgs_once = chanmsgs_inner.borrow_mut();
                    let decoded_message = message_decode(message.as_slice());
                    let channel = decoded_message.channel.clone();

                    if !spawned_once.contains_key(&channel) {
                        let chan = channel.clone();
                        hdr.extend(key);
                        hdr.extend(message);

                        println!("Spawning peer channel thread");
                        thread::spawn(move || peer_conn(peer, peer_cnt, chan, hdr, tx_peer, tx));

                        let mut channel_entry = HashMap::new();
                        channel_entry.insert(cnt, tx_once.clone());
                        spawned_once.insert(channel.clone(), channel_entry);

                        let messages: Vec<Vec<u8>> = Vec::new();
                        chanmsgs_once.insert(channel.clone(), messages);
                    }
                    else {
                        let mut channel_entry = spawned_once.get_mut(&channel).unwrap();
                        channel_entry.insert(cnt, tx_once.clone());
                        let chanmsgs = chanmsgs_once.get(&channel).unwrap();
                        // send history to client
                        for msg in chanmsgs {
                            tx_once.send(msg.clone()).unwrap();
                        }
                    }
                    // todo: add empty message to history list too
                    println!("User {}:{} joined channel {}", cnt, decoded_message.uid, channel);
                    return Ok((reader, channel));
                }
                Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"))
            })
        });

        let spawned_inner = spawned.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let socket_next = socket_once.and_then(move |(reader, channel)| {
            let channel_next = channel.clone();
            let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;HDRL]);
                let frame = frame.and_then(move |(reader, payload)| mles_process_hdr(reader, payload));

                //todo: optimize, read hdr and key with one pass
                let frame = frame.and_then(move |(reader, hdr, hdr_len)| {
                    //dummy read key
                    let tframe = io::read_exact(reader, vec![0;KEYL]);
                    tframe.and_then(move |(reader, key)| mles_process_dummy_key(reader, hdr, key, hdr_len))
                });

                let frame = frame.and_then(move |(reader, hdr, key, hdr_len)| {
                    let tframe = io::read_exact(reader, vec![0;hdr_len]);
                    tframe.and_then(move |(reader, message)| mles_process_msg(reader, hdr, key, message)) 
                });

                let spawned = spawned_inner.clone();
                let chanmsgs = chanmsgs_inner.clone();
                let channel = channel_next.clone();
                frame.map(move |(reader, mut hdr, key, message)| {
                    hdr.extend(key);
                    hdr.extend(message);

                    // add to history
                    let mut channel_msgs = chanmsgs.borrow_mut();
                    let mut channel_msg = channel_msgs.get_mut(&channel).unwrap();
                    channel_msg.push(hdr.clone());

                    //distribute
                    let spawned = spawned.borrow();
                    let channels = spawned.get(&channel).unwrap();
                    for (ocnt, tx) in channels {
                        if *ocnt != cnt {
                            //todo remove failed channels
                            println!("Sending to {}", *ocnt);
                            let _res = tx.send(hdr.clone()).map_err(|err| { println!("Failed to send to channel: {}", err); () });
                        }
                    }
                    reader
                })
            })
        });

        //try to get tx to peer
        let spawned_inner = spawned.clone();   
        let peer_writer = rx_peer.for_each(move |(peer_cnt, channel, peer_tx)| {
            let mut spawned_once = spawned_inner.borrow_mut();
            let mut channel_entry = spawned_once.get_mut(&channel).unwrap();  
            channel_entry.insert(peer_cnt, peer_tx);  
            Ok(())
        });
        let peer_writer = peer_writer.map(|_| ());

        handle.spawn(peer_writer.then(|_| {
            Ok(())
        }));

        let socket_writer = rx.fold(writer, |writer, msg| {
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });

        let channels = spawned.clone();
        let chanmsgs = channelmsgs.clone();
        let socket_reader = socket_next.map_err(|_| ());
        let connection = socket_reader.map(|_| ()).select2(socket_writer.map(|_| ()));
        handle.spawn(connection.then(move |_| {
            let mut chans = channels.borrow_mut();
            for (cname, channel) in chans.iter_mut() {
                if channel.contains_key(&cnt) {
                    channel.remove(&cnt);
                    if 0 == channel.len() {
                        let mut channelmsgs = chanmsgs.borrow_mut();
                        channelmsgs.remove(cname);
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

fn peer_conn(peer: SocketAddr, peer_cnt: u64, channel: String, msg: Vec<u8>, 
             tx_peer_for_rcv: UnboundedSender<(u64, String, UnboundedSender<Vec<u8>>)>, 
             tx_peer: UnboundedSender<Vec<u8>>) 
{
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let tcp = TcpStream::connect(&peer, &handle);
    //todo: tx_peer handling
    let tx_peer = Rc::new(RefCell::new(tx_peer));  

    let client = tcp.and_then(move |pstream| {
        let _val = pstream.set_nodelay(true).map_err(|_| panic!("Cannot set peer to no delay"));
        println!("Successfully connected to peer");

        //save writes to db
        let (reader, writer) = pstream.split();
        let (tx, rx) = unbounded();
        let _res = tx_peer_for_rcv.send((peer_cnt, channel, tx.clone())).map_err(|err| { println!("Cannot send from peer: {}", err); () });
        let _res = tx.send(msg).map_err(|err| { println!("Cannot write to tx: {}", err); });

        let psocket_writer = rx.fold(writer, |writer, msg| {
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });
        let psocket_writer = psocket_writer.map(|_| ());

        handle.spawn(psocket_writer.then(|_| {
            println!("Peer socket writer closed");
            Ok(())
        }));

        let tx_peer_inner = tx_peer.clone();
        let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
        iter.fold(reader, move |reader, _| {

            //todo: read hdr and key with one pass
            let frame = io::read_exact(reader, vec![0;HDRL]);
            let frame = frame.and_then(move |(reader, hdr)| mles_process_hdr(reader, hdr));

            let frame = frame.and_then(move |(reader, hdr, hdr_len)| {
                let tframe = io::read_exact(reader, vec![0;KEYL]);
                tframe.and_then(move |(reader, key)| mles_process_dummy_key(reader, hdr, key, hdr_len))
            });

            let frame = frame.and_then(move |(reader, hdr, key, hdr_len)| {
                let tframe = io::read_exact(reader, vec![0;hdr_len]);
                tframe.and_then(move |(reader, message)| mles_process_msg(reader, hdr, key, message)) 
            }); 

            let tx_peer_frame = tx_peer_inner.clone();
            frame.map(move |(reader, mut hdr, key, message)| {
                let tx_peer = tx_peer_frame.borrow();
                hdr.extend(key);
                hdr.extend(message);
                let _res = tx_peer.send(hdr).map_err(|err| { println!("Failed to send from peer: {}", err); () });
                reader
            })
        })
    });

    // execute server
    core.run(client).unwrap();
}

fn mles_get_cnt(cnt: u64) -> u64 {
    let mut val = cnt as u32;
    val += 1;
    val as u64 
}

fn mles_get_peer_cnt(cnt: u64) -> u64 {
    let mut val = cnt;
    val = val >> 32;
    val += 1;
    val << 32
}

fn mles_process_hdr(reader: io::ReadHalf<TcpStream>, hdr: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, usize), std::io::Error> {
    if hdr.len() == 0 {
        Err(Error::new(ErrorKind::BrokenPipe, "broken pipe"))
    } else {
        if read_hdr_type(hdr.as_slice()) != 'M' as u32 {
            return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header"));
        }
        let hdr_len = read_hdr_len(hdr.as_slice());
        if 0 == hdr_len {
            return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header len"));
        }
        Ok((reader, hdr, hdr_len))
    }
}

fn mles_process_dummy_key(reader: io::ReadHalf<TcpStream>, hdr: Vec<u8>, key: Vec<u8>, hdr_len: usize) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, Vec<u8>, usize), std::io::Error> {
    Ok((reader, hdr, key, hdr_len))
}

fn mles_process_msg(reader: io::ReadHalf<TcpStream>, hdr: Vec<u8>, key: Vec<u8>, message: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, Vec<u8>, Vec<u8>), std::io::Error> { 
    if message.len() > 0 { 
        return Ok((reader, hdr, key, message));
    }
    Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"))
}

fn mles_process_key(reader: io::ReadHalf<TcpStream>, hdr: Vec<u8>, key: Vec<u8>, hdr_len: usize, keyval: String, peer_addr: SocketAddr) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, Vec<u8>, usize), std::io::Error> { 
    let hkey;
    let keyx = read_key(&key);
    if 0 == keyval.len() {
        hkey = do_hash(&peer_addr);
    }
    else {
        hkey = do_hash(&keyval);
    }
    if hkey != keyx {
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect remote key"));
    }
    Ok((reader, hdr, key, hdr_len))
}
