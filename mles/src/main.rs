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

mod local_db;
mod frame;
mod peer;

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::io::{Error, ErrorKind};
use std::{process, env};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::thread;

use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;
use tokio_io::io;
use tokio_io::AsyncRead;

use futures::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::unbounded;
use mles_utils::*;

use local_db::*;
use frame::*;
use peer::*;

const HDRL: usize = 4; //hdr len
const KEYL: usize = 8; //key len
const HDRKEYL: usize = HDRL + KEYL;

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

    let mles_db_hash: HashMap<String, MlesDb> = HashMap::new();
    let mles_db = Rc::new(RefCell::new(mles_db_hash));
    let channel_db = Rc::new(RefCell::new(HashMap::new()));
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
        cnt = inc_cnt(cnt);

        let (tx_peer_for_msgs, rx_peer_for_msgs) = unbounded();

        let frame = io::read_exact(reader, vec![0;HDRKEYL]);
        let frame = frame.and_then(move |(reader, hdr_key)| process_hdr(reader, hdr_key));

        let paddr_inner = paddr.clone();
        let keyval_inner = keyval.clone();
        // verify key
        let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| process_key(reader, hdr_key, hdr_len, keyval_inner, paddr_inner));

        let tx_inner = tx.clone();
        let channel_db_inner = channel_db.clone();
        let mles_db_inner = mles_db.clone();
        let socket_once = frame.and_then(move |(reader, mut hdr_key, hdr_len)| {
            let tframe = io::read_exact(reader, vec![0;hdr_len]);
            tframe.and_then(move |(reader, message)| {
                if 0 == message.len() {  
                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"));
                }
                let decoded_message = message_decode(message.as_slice());
                let channel = decoded_message.get_channel().clone();
                let mut mles_db_once = mles_db_inner.borrow_mut();
                let mut channel_db = channel_db_inner.borrow_mut();

                if !mles_db_once.contains_key(&channel) {
                    let chan = channel.clone();

                    //if peer is set, create peer channel thread
                    if peer::has_peer(&peer) {
                        let mut msg = hdr_key.clone();
                        msg.extend(message.clone());
                        peer_cnt = inc_peer_cnt(cnt);
                        thread::spawn(move || peer_conn(peer, peer_cnt, chan, msg, tx_peer_for_msgs));
                    }

                    let mut mles_db_entry = MlesDb::new();
                    mles_db_entry.add_channel(cnt, tx_inner.clone());
                    mles_db_once.insert(channel.clone(), mles_db_entry);

                }
                else {
                    if let Some(mles_db_entry) = mles_db_once.get_mut(&channel) {
                        mles_db_entry.add_channel(cnt, tx_inner.clone());
                        if peer::has_peer(&peer) {
                            if let Some(channelpeer_entry) = mles_db_entry.get_peer_tx() {
                                let _res = channelpeer_entry.send(tx_inner.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
                            }
                            else {
                                println!("Cannot find channel peer for channel {}", channel);
                            }
                        }
                        else {
                            // send history to client if peer is not set
                            for msg in mles_db_entry.get_messages() {
                                let _res = tx_inner.send(msg.clone()).map_err(|err| {println!("Send history failed: {}", err); ()});
                            }
                        }
                    }
                    else {
                        println!("Channel {} not found", channel);
                        return Err(Error::new(ErrorKind::BrokenPipe, "internal error"));
                    }
                }

                if let Some(mles_db_entry) = mles_db_once.get_mut(&channel) {
                    // add to history if no peer
                    if !peer::has_peer(&peer) {
                        hdr_key.extend(message);
                        mles_db_entry.add_message(hdr_key);
                    }
                }
                channel_db.insert(cnt, channel.clone());
                println!("User {}:{} joined channel {}", cnt, decoded_message.get_uid(), channel);
                Ok((reader, channel))
            })
        });

        let mles_db_inner = mles_db.clone();
        let socket_next = socket_once.and_then(move |(reader, channel)| {
            let channel_next = channel.clone();
            let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;HDRKEYL]);
                let frame = frame.and_then(move |(reader, hdr_key)| process_hdr_dummy_key(reader, hdr_key));

                let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| {
                    let tframe = io::read_exact(reader, vec![0;hdr_len]);
                    tframe.and_then(move |(reader, message)| process_msg(reader, hdr_key, message)) 
                });

                let mles_db = mles_db_inner.clone();
                let channel = channel_next.clone();
                frame.map(move |(reader, mut hdr_key, message)| {
                    hdr_key.extend(message);

                    let mut mles_db_borrow = mles_db.borrow_mut();
                    if let Some(mles_db_entry) = mles_db_borrow.get_mut(&channel) {
                        // add to history if no peer
                        if !peer::has_peer(&peer) {
                            mles_db_entry.add_message(hdr_key.clone());
                        }

                        if let Some(channels) = mles_db_entry.get_channels() {
                            for (ocnt, tx) in channels.iter() {
                                if *ocnt != cnt {
                                    let _res = tx.send(hdr_key.clone()).map_err(|_| { 
                                        //just ignore failures for now
                                        () 
                                    });
                                }
                            }
                        }
                    }
                    else {
                        println!("Cannot distribute channel {}", channel);
                    }

                    reader
                })
            })
        });

        let tx_inner = tx.clone();
        let mles_db_inner = mles_db.clone();
        let peer_writer = rx_peer_for_msgs.for_each(move |(peer_cnt, channel, peer_tx, tx_orig_chan)| {
            let mut mles_db_once = mles_db_inner.borrow_mut();
            if let Some(mut mles_db_entry) = mles_db_once.get_mut(&channel) {  
                mles_db_entry.add_channel(peer_cnt, peer_tx);  
                mles_db_entry.set_peer_tx(tx_orig_chan.clone());
                let _res = tx_orig_chan.send(tx_inner.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
            }
            else {
                println!("Cannot find peer channel {}", channel);
            }
            Ok(())
        });
        handle.spawn(peer_writer.then(|_| {
            Ok(())
        }));

        let socket_writer = rx.fold(writer, |writer, msg| {
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });

        let mles_db_conn = mles_db.clone();
        let channel_db_conn = channel_db.clone();
        let socket_reader = socket_next.map_err(|_| ());
        let connection = socket_reader.map(|_| ()).select(socket_writer.map(|_| ()));
        handle.spawn(connection.then(move |_| {
            let mut mles_db = mles_db_conn.borrow_mut();
            let mut channel_db = channel_db_conn.borrow_mut();
            let mut chan_to_rem: Option<u64> = None;
            let mut chan_drop = false;
            if let Some(channel) = channel_db.get_mut(&cnt) {
                if let Some(mles_db_entry) = mles_db.get_mut(channel) {
                    mles_db_entry.rem_channel(cnt);
                    chan_to_rem = Some(cnt);
                    if 0 == mles_db_entry.get_channels_len() && 0 == mles_db_entry.get_history_limit() {
                        chan_drop = true;
                    }
                }
                if chan_drop {
                    mles_db.remove(channel);
                    println!("Channel {} dropped.", channel);
                }
            }
            if let Some(cnt) = chan_to_rem {
                channel_db.remove(&cnt);
            }
            println!("Connection {} for user {} closed.", addr, cnt);
            Ok(())
        }));
        Ok(())
    });

    // execute server
    let _res = core.run(srv).map_err(|err| { println!("Main: {}", err); ()});
}

fn inc_cnt(cnt: u64) -> u64 {
    let mut val = cnt as u32;
    val += 1;
    val as u64 
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inc_cnt() {
        let val: u64 = 1;
        assert_eq!(val + 1, inc_cnt(val));
    }
}
