/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/. 
*
*  Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
* */
use std::io::Error;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::io::ErrorKind;
use std::process;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;
use tokio_io::io;
use tokio_io::AsyncRead;

use futures::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::unbounded;

use local_db::*;
use frame::*;
use peer::*;
use super::*;

pub fn run(address: SocketAddr, peer: Option<SocketAddr>, keyval: String, keyaddr: String, hist_limit: usize, debug_flags: u64) {
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let socket = match TcpListener::bind(&address, &handle) {
        Ok(listener) => listener,
        Err(err) => {
            println!("Error: {}", err);
            process::exit(1);
        },
    };
    if 0 != debug_flags {
        println!("Listening on: {}", address);
    }

    let mles_db_hash: HashMap<String, MlesDb> = HashMap::new();
    let mles_db = Rc::new(RefCell::new(mles_db_hash));
    let channel_db = Rc::new(RefCell::new(HashMap::new()));

    let mut cnt = 0;

    let srv = socket.incoming().for_each(move |(stream, addr)| {
        let _val = stream.set_nodelay(true)
                         .map_err(|_| panic!("Cannot set to no delay"));
        let _val = stream.set_keepalive(Some(Duration::new(KEEPALIVE, 0)))
                         .map_err(|_| panic!("Cannot set keepalive"));
        cnt += 1;

        if 0 != debug_flags {
            println!("New connection: {}", addr);
        }
        let paddr = match stream.peer_addr() {
                Ok(paddr) => paddr,
                Err(_) => {
                    let addr = "0.0.0.0:0";
                    addr.parse::<SocketAddr>().unwrap()
                }
        };
        let mut is_addr_set = false;
        let mut keys = Vec::new();
        if !keyval.is_empty() {
            keys.push(keyval.clone());
        }
        else {
            keys.push(addr2str(&paddr));
            is_addr_set = true;
            if !keyaddr.is_empty() {
                keys.push(keyaddr.clone());
            }
        }

        let (reader, writer) = stream.split();
        let (tx, rx) = unbounded();

        let (tx_peer_for_msgs, rx_peer_for_msgs) = unbounded();

        let frame = io::read_exact(reader, vec![0;::HDRKEYL]);
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
        let socket_once = frame.and_then(move |(reader, mut hdr_key, messages, decoded_message)| {
                let channel = decoded_message.get_channel().clone();
                let mut mles_db_once = mles_db_inner.borrow_mut();
                let mut channel_db = channel_db_inner.borrow_mut();
                let message = messages[0].clone();

                //pick the verified cid from header and use it as an identifier
                let cid = read_cid_from_hdr(&hdr_key) as u64;

                if !mles_db_once.contains_key(&channel) {
                    let chan = channel.clone();

                    //if peer is set, create peer channel thread
                    if peer::has_peer(&peer) {
                        let mut msg = hdr_key.clone();
                        msg.extend(message.clone());
                        let peer = peer.unwrap();
                        thread::spawn(move || peer_conn(hist_limit, peer, is_addr_set, keyaddr_inner, chan, msg, &tx_peer_for_msgs, debug_flags));
                    }

                    let mut mles_db_entry = MlesDb::new(hist_limit);
                    mles_db_entry.add_channel(cid, tx_inner.clone());
                    mles_db_once.insert(channel.clone(), mles_db_entry);

                }
                else if let Some(mles_db_entry) = mles_db_once.get_mut(&channel) {
                    if mles_db_entry.check_for_duplicate_cid(cid as u32) {
                        println!("Duplicate cid {:x} detected", cid);
                        return Err(Error::new(ErrorKind::BrokenPipe, "duplicate cid"));
                    }
                    mles_db_entry.add_channel(cid, tx_inner.clone());
                    if peer::has_peer(&peer) {
                        if let Some(channelpeer_entry) = mles_db_entry.get_peer_tx() {
                            //sending tx to peer
                            let _res = channelpeer_entry.unbounded_send(tx_inner.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
                        }
                        else {
                            if debug_flags != 0 {
                                println!("Cannot find channel peer for channel {}", channel);
                            }
                        }
                    }
                    else {

                        // send history to client if peer is not set
                        for msg in mles_db_entry.get_messages() {
                            let _res = tx_inner.unbounded_send(msg.clone()).map_err(|err| {println!("Send history failed: {}", err); ()});
                        }
                    }
                }
                else {
                    println!("Channel {} not found", channel);
                    return Err(Error::new(ErrorKind::BrokenPipe, "internal error"));
                }

                if let Some(mles_db_entry) = mles_db_once.get_mut(&channel) {
                    if let Some(channels) = mles_db_entry.get_channels() {
                        for msg in &messages {
                            let mut hdr = hdr_key.clone();
                            hdr.extend(msg);

                            for (ocid, tx) in channels.iter() {
                                if *ocid != cid {
                                    let _res = tx.unbounded_send(hdr.clone()).map_err(|_| { 
                                        //just ignore failures for now
                                        () 
                                    });
                                }
                            }
                        }
                    }
                    // add to history if no peer
                    if !peer::has_peer(&peer) {
                        if messages.len() > 1 && 0 == mles_db_entry.get_messages_len() {
                            // resync messages to history
                            for msg in &messages {
                                // add resync to history
                                let mut hdr = hdr_key.clone();
                                hdr.extend(msg);
                                println!("Adding resync msg {:?}", hdr);
                                mles_db_entry.add_message(hdr);
                            } 
                        }
                        else {
                            hdr_key.extend(message);
                            mles_db_entry.add_message(hdr_key);
                        }
                    }

                    mles_db_entry.add_tx_db(tx_inner.clone());
                }
                channel_db.insert(cnt, (cid, channel.clone()));
                if 0 != debug_flags {
                    println!("User {}:{:x} joined.", cnt, cid);
                }
                Ok((reader, cid, channel))
        });

        let mles_db_inner = mles_db.clone();
        let socket_next = socket_once.and_then(move |(reader, cid, channel)| {
            let channel_next = channel.clone();
            let iter = stream::iter_ok(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;::HDRKEYL]);
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
                            for (ocid, tx) in channels.iter() {
                                if *ocid != cid {
                                    let _res = tx.unbounded_send(hdr_key.clone()).map_err(|_| { 
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
        let peer_writer = rx_peer_for_msgs.for_each(move |(peer_cid, channel, peer_tx, tx_orig_chan)| {
            let mut mles_db_once = mles_db_inner.borrow_mut();
            if let Some(mut mles_db_entry) = mles_db_once.get_mut(&channel) {  
                //setting peer tx
                mles_db_entry.add_channel(peer_cid, peer_tx);  
                mles_db_entry.set_peer_tx(tx_orig_chan.clone());
                //sending all tx's to (possibly restarted) peer
                for tx_entry in mles_db_entry.get_tx_db() {
                    let _res = tx_orig_chan.unbounded_send(tx_entry.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
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
            if let Some(&mut (cid, ref channel)) = channel_db.get_mut(&cnt) {
                if let Some(mles_db_entry) = mles_db.get_mut(channel) {
                    mles_db_entry.rem_channel(cid);
                    chan_to_rem = Some(cid);
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
            if let Some(cid) = chan_to_rem {
                channel_db.remove(&cid);
                if 0 != debug_flags {
                    println!("Connection {} for user {}:{:x} closed.", addr, cnt, cid);
                }
            }
            Ok(())
        }));
        Ok(())
    });

    // execute server                               
    let _res = core.run(srv).map_err(|err| { println!("Main: {}", err); ()});
}

