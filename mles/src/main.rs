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
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
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

const SRVPORT: &str = ":8077";

const HDRL: usize = 4; //hdr len
const KEYL: usize = 8; //key len
const HDRKEYL: usize = HDRL + KEYL;
const USAGE: &str = "Usage: mles [peer-address] [--history-limit=N]";
const HISTLIMIT: usize = 100;

const KEYAND: u64 = 0x0000ffffffffffff;

const KEEPALIVEMS: Option<u32> = Some(5000);

fn main() {
    let mut peer: Option<SocketAddr> = None;
    let mut argcnt = 0;
    let mut hist_limit = HISTLIMIT;
    for arg in env::args() {
        argcnt += 1;
        if 1 == argcnt {
            continue;
        }
        let this_arg = arg.clone();
        let v: Vec<&str> = this_arg.split("--history-limit=").collect();
        if v.len() > 1 {
            if let Some(limitstr) = v.get(1) {
                if let Ok(limit)= limitstr.parse::<usize>() {
                    hist_limit = limit;
                    println!("History limit: {}", hist_limit);
                    continue;
                }
            }
            println!("{}", USAGE);
            process::exit(1);
        }
        else {
            let peerarg = arg + SRVPORT;
            let rpeer: Vec<_> = peerarg.to_socket_addrs()
                                       .unwrap_or(vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)].into_iter())
                                       .collect();
            let rpeer = *rpeer.first().unwrap();
            let rpeer = Some(rpeer);
            // check that we really got a proper peer
            if has_peer(&rpeer) {
                println!("Using peer domain {}", peerarg); 
                peer = rpeer;
            }
            else { 
                println!("Unable to resolve peer domain {}", peerarg); 
            }
        }
        if argcnt > 3 {
            println!("{}", USAGE);
            process::exit(1);
        }
    }

    let keyval = match env::var("MLES_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    let keyaddr = match env::var("MLES_ADDR_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let address = "0.0.0.0".to_string() + SRVPORT;
    let address = address.parse().unwrap();
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

    let srv = socket.incoming().for_each(move |(stream, addr)| {
        let _val = stream.set_nodelay(true)
                         .map_err(|_| panic!("Cannot set to no delay"));
        let _val = stream.set_keepalive_ms(KEEPALIVEMS)
                         .map_err(|_| panic!("Cannot set keepalive"));
        cnt += 1;

        println!("New Connection: {}", addr);
        let paddr = match stream.peer_addr() {
                Ok(paddr) => paddr,
                Err(_) => {
                    let addr = "0.0.0.0:0";
                    let addr = addr.parse::<SocketAddr>().unwrap();
                    addr
                }
        };
        let mut is_addr_set = false;
        let mut keys = Vec::new();
        if keyval.len() > 0 {
            keys.push(keyval.clone());
        }
        else {
            keys.push(addr2str(&paddr));
            is_addr_set = true;
            if keyaddr.len() > 0 {
                keys.push(keyaddr.clone());
            }
        }

        let (reader, writer) = stream.split();
        let (tx, rx) = unbounded();

        let (tx_peer_for_msgs, rx_peer_for_msgs) = unbounded();

        let frame = io::read_exact(reader, vec![0;HDRKEYL]);
        let frame = frame.and_then(move |(reader, hdr_key)| process_hdr(reader, hdr_key));
        let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| { 
            let tframe = io::read_exact(reader, vec![0;hdr_len]);
            tframe.and_then(move |(reader, message)| process_msg(reader, hdr_key, message))
        });

        // verify key
        let frame = frame.and_then(move |(reader, hdr_key, message)| process_key(reader, hdr_key, message, keys));

        let tx_inner = tx.clone();
        let channel_db_inner = channel_db.clone();
        let mles_db_inner = mles_db.clone();
        let keyaddr_inner = keyaddr.clone();
        let socket_once = frame.and_then(move |(reader, mut hdr_key, message, decoded_message)| {
                let channel = decoded_message.get_channel().clone();
                let mut mles_db_once = mles_db_inner.borrow_mut();
                let mut channel_db = channel_db_inner.borrow_mut();

                //pick the verified key from header and use it as an identifier
                let key = read_key_from_hdr(&hdr_key);
                let key = set_key(key);

                if !mles_db_once.contains_key(&channel) {
                    let chan = channel.clone();

                    //if peer is set, create peer channel thread
                    if peer::has_peer(&peer) {
                        let mut msg = hdr_key.clone();
                        msg.extend(message.clone());
                        let peer = peer.unwrap();
                        thread::spawn(move || peer_conn(hist_limit, peer, is_addr_set, keyaddr_inner, chan, msg, tx_peer_for_msgs));
                    }

                    let mut mles_db_entry = MlesDb::new(hist_limit);
                    mles_db_entry.add_channel(key, tx_inner.clone());
                    mles_db_once.insert(channel.clone(), mles_db_entry);

                }
                else {
                    if let Some(mles_db_entry) = mles_db_once.get_mut(&channel) {
                        if mles_db_entry.check_for_duplicate_key(key) {
                            println!("Duplicate key {:x} detected", key);
                            return Err(Error::new(ErrorKind::BrokenPipe, "duplicate key"));
                        }
                        mles_db_entry.add_channel(key, tx_inner.clone());
                        if peer::has_peer(&peer) {
                            if let Some(channelpeer_entry) = mles_db_entry.get_peer_tx() {
                                //sending tx to peer
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
                    mles_db_entry.add_tx_db(tx_inner.clone());
                }
                channel_db.insert(cnt, (key, channel.clone()));
                println!("User {}:{:x} joined.", cnt, key);
                Ok((reader, key, channel))
        });

        let mles_db_inner = mles_db.clone();
        let socket_next = socket_once.and_then(move |(reader, key, channel)| {
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
                            for (okey, tx) in channels.iter() {
                                if *okey != key {
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

        let mles_db_inner = mles_db.clone();
        let peer_writer = rx_peer_for_msgs.for_each(move |(peer_key, channel, peer_tx, tx_orig_chan)| {
            let mut mles_db_once = mles_db_inner.borrow_mut();
            if let Some(mut mles_db_entry) = mles_db_once.get_mut(&channel) {  
                //setting peer tx
                mles_db_entry.add_channel(peer_key, peer_tx);  
                mles_db_entry.set_peer_tx(tx_orig_chan.clone());
                //sending all tx's to (possibly restarted) peer
                for tx_entry in mles_db_entry.get_tx_db() {
                    let _res = tx_orig_chan.send(tx_entry.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
                }
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
            if let Some(&mut (key, ref channel)) = channel_db.get_mut(&cnt) {
                if let Some(mles_db_entry) = mles_db.get_mut(channel) {
                    mles_db_entry.rem_channel(key);
                    chan_to_rem = Some(key);
                    if 0 == mles_db_entry.get_channels_len() {
                        mles_db_entry.clear_tx_db();
                        if 0 == mles_db_entry.get_history_limit() {
                            chan_drop = true;
                        }
                    }
                }
                if chan_drop {
                    mles_db.remove(channel);
                }
            }
            if let Some(key) = chan_to_rem {
                channel_db.remove(&key);
                println!("Connection {} for user {}:{:x} closed.", addr, cnt, key);
            }
            Ok(())
        }));
        Ok(())
    });

    // execute server                               
    let _res = core.run(srv).map_err(|err| { println!("Main: {}", err); ()});
}

fn set_key(orig_key: u64) -> u64 {
    let mut val = orig_key;
    val &= KEYAND;
    val 
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_key() {
        let val: u64 = 1;
        assert_eq!(val & KEYAND, set_key(val));
    }
}
