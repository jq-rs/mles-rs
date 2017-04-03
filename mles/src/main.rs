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

        let (tx_peer_for_msgs, rx_peer_for_msgs) = unbounded();

        let frame = io::read_exact(reader, vec![0;HDRL+KEYL]);
        let frame = frame.and_then(move |(reader, hdr_key)| mles_process_hdr(reader, hdr_key));

        let paddr_inner = paddr.clone();
        let keyval_inner = keyval.clone();
        // verify key
        let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| mles_process_key(reader, hdr_key, hdr_len, keyval_inner, paddr_inner));

        let tx_once = tx.clone();
        let spawned_inner = spawned.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let socket_once = frame.and_then(move |(reader, mut hdr_key, hdr_len)| {
            let tframe = io::read_exact(reader, vec![0;hdr_len]);
            tframe.and_then(move |(reader, message)| {
                if 0 == message.len() {  
                    return Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"));
                }
                let mut spawned_once = spawned_inner.borrow_mut();
                let mut chanmsgs_once = chanmsgs_inner.borrow_mut();
                let decoded_message = message_decode(message.as_slice());
                let channel = decoded_message.channel.clone();

                if !spawned_once.contains_key(&channel) {
                    let chan = channel.clone();
                    hdr_key.extend(message);

                    //if peer is set, create peer channel thread
                    if mles_has_peer(&peer) {
                        peer_cnt = mles_get_peer_cnt(cnt);
                        thread::spawn(move || peer_conn(peer, peer_cnt, chan, hdr_key, tx_peer_for_msgs));
                    }

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

                    // send history to client if peer is not set
                    if !mles_has_peer(&peer) {
                        for msg in chanmsgs {
                            tx_once.send(msg.clone()).unwrap();
                        }
                    }
                }
                println!("User {}:{} joined channel {}", cnt, decoded_message.uid, channel);
                Ok((reader, channel))
            })
        });

        let spawned_inner = spawned.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let socket_next = socket_once.and_then(move |(reader, channel)| {
            let channel_next = channel.clone();
            let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;HDRL+KEYL]);
                let frame = frame.and_then(move |(reader, hdr_key)| mles_process_hdr_dummy_key(reader, hdr_key));

                let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| {
                    let tframe = io::read_exact(reader, vec![0;hdr_len]);
                    tframe.and_then(move |(reader, message)| mles_process_msg(reader, hdr_key, message)) 
                });

                let spawned = spawned_inner.clone();
                let chanmsgs = chanmsgs_inner.clone();
                let channel = channel_next.clone();
                frame.map(move |(reader, mut hdr_key, message)| {
                    hdr_key.extend(message);

                    // add to history if no peer
                    if !mles_has_peer(&peer) {
                        let mut channel_msgs = chanmsgs.borrow_mut();
                        let mut channel_msg = channel_msgs.get_mut(&channel).unwrap();
                        channel_msg.push(hdr_key.clone());
                    }

                    //distribute
                    let spawned_inner = spawned.borrow();
                    let channels = spawned_inner.get(&channel).unwrap();
                    for (ocnt, tx) in channels {
                        if *ocnt != cnt {
                            //todo remove failed channels
                            let _res = tx.send(hdr_key.clone()).map_err(|_| { 
                                //borrow checker does not like it
                                //channels.remove(ocnt);
                                () 
                            });
                        }
                    }

                    reader
                })
            })
        });

        //try to get tx to peer
        let spawned_inner = spawned.clone();   
        let tx_chan = tx.clone();
        let peer_writer = rx_peer_for_msgs.for_each(move |(peer_cnt, channel, peer_tx, tx_orig_chan)| {
            let mut spawned_once = spawned_inner.borrow_mut();
            let mut channel_entry = spawned_once.get_mut(&channel).unwrap();  
            channel_entry.insert(peer_cnt, peer_tx);  
            let _res = tx_orig_chan.send(tx_chan.clone()).map_err(|err| {println!("Cannot reach peer: {}", err); ()});
            Ok(())
        });
        handle.spawn(peer_writer);

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
    let _res = core.run(srv).map_err(|err| { println!("Main: {}", err); ()});
}

fn peer_conn(peer: SocketAddr, peer_cnt: u64, channel: String, msg: Vec<u8>, 
             tx_peer_for_msgs: UnboundedSender<(u64, String, UnboundedSender<Vec<u8>>, UnboundedSender<UnboundedSender<Vec<u8>>>)>) 
{
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let channelmsgs = Rc::new(RefCell::new(Vec::new()));  

    let tcp = TcpStream::connect(&peer, &handle);
    let (tx_orig_chan, rx_orig_chan) = unbounded();
    let tx_origs = Rc::new(RefCell::new(Vec::new()));  

    let orig_channel = channel.clone();
    println!("Peer channel thread for channel {}", orig_channel);
    let client = tcp.and_then(move |pstream| {
        let _val = pstream.set_nodelay(true).map_err(|_| panic!("Cannot set peer to no delay"));
        println!("Successfully connected to peer");

        //save writes to db
        let (reader, writer) = pstream.split();
        let (tx, rx) = unbounded();
        let _res = tx_peer_for_msgs.send((peer_cnt, channel, tx.clone(), tx_orig_chan.clone())).map_err(|err| { println!("Cannot send from peer: {}", err); () });
        let _res = tx.send(msg).map_err(|err| { println!("Cannot write to tx: {}", err); });

        let chanmsgs_inner = channelmsgs.clone();
        let psocket_writer = rx.fold(writer, move |writer, msg| {
            //push message to history
            let mut channel_msgs = chanmsgs_inner.borrow_mut();
            channel_msgs.push(msg.clone());
            
            //send message forward
            let amt = io::write_all(writer, msg);
            let amt = amt.map(|(writer, _)| writer);
            amt.map_err(|_| ())
        });
        let psocket_writer = psocket_writer.map(|_| ());

        handle.spawn(psocket_writer.then(|_| {
            println!("Peer socket writer closed");
            Ok(())
        }));

        //todo tx_origs reader should be left to read, now bails out after first msg
        let tx_origs_inner = tx_origs.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let tx_origs_reader = rx_orig_chan.for_each(move |tx_orig| {
            //save receiver side tx to db
            let mut tx_origs_once = tx_origs_inner.borrow_mut();
            tx_origs_once.push(tx_orig.clone());  

            //push history to client if not the first one (as peer will send the history then)
            if tx_origs_once.len() > 1 {
                let channel_msgs = chanmsgs_inner.borrow();
                for msg in channel_msgs.iter() {
                    let _res = tx_orig.send(msg.clone()).map_err(|err| { println!("Failed to send history from peer: {}", err); () });
                }
            }
            Ok(())
        });
        let tx_origs_reader = tx_origs_reader.map_err(|err| {println!("Error {:?}", err); () });
        handle.spawn(tx_origs_reader.then(|err| {
            println!("Tx origs reader bail out {:?}", err);
            Ok(())
        }));

        /*
        let tx_origs_inner = tx_origs.clone();
        handle.spawn_fn(|| {
            rx_orig_chan.for_each(move |tx_orig| {
                let mut tx_origs_once = tx_origs_inner.borrow_mut();
                tx_origs_once.push(tx_orig);  
                Ok(())
            }).map(|_| {println!("Got ok for tx origs"); ()})
        });*/

        let tx_origs_inner = tx_origs.clone();
        let chanmsgs_inner = channelmsgs.clone();
        let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
        iter.fold(reader, move |reader, _| {
            let frame = io::read_exact(reader, vec![0;HDRL+KEYL]);
            let frame = frame.and_then(move |(reader, hdr_key)| mles_process_hdr_dummy_key(reader, hdr_key));

            let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| {
                let tframe = io::read_exact(reader, vec![0;hdr_len]);
                tframe.and_then(move |(reader, message)| mles_process_msg(reader, hdr_key, message)) 
            }); 

            let tx_origs_frame = tx_origs_inner.clone();
            let chanmsgs_frame = chanmsgs_inner.clone();
            frame.map(move |(reader, mut hdr_key, message)| {
                hdr_key.extend(message);

                //send message forward
                let tx_origs = tx_origs_frame.borrow();
                for tx_orig in tx_origs.iter() {
                    let _res = tx_orig.send(hdr_key.clone()).map_err(|err| { println!("Failed to send from peer: {}", err); () });
                }

                //push message to history
                let mut channel_msgs = chanmsgs_frame.borrow_mut();
                channel_msgs.push(hdr_key);
                
                reader
            })
        })
    });

    // execute server
    let _res = core.run(client).map_err(|err| { println!("Peer: {}", err); () });
    println!("Peer channel thread {} out", orig_channel);
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

fn mles_process_hdr_dummy_key(reader: io::ReadHalf<TcpStream>, hdr_key: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, usize), std::io::Error> {
    mles_process_hdr(reader, hdr_key)
}

fn mles_process_hdr(reader: io::ReadHalf<TcpStream>, hdr: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, usize), std::io::Error> {
    if hdr.len() == 0 {
        return Err(Error::new(ErrorKind::BrokenPipe, "broken pipe"));
    }
    if read_hdr_type(hdr.as_slice()) != 'M' as u32 {
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header"));
    }
    let hdr_len = read_hdr_len(hdr.as_slice());
    if 0 == hdr_len {
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect header len"));
    }
    Ok((reader, hdr, hdr_len))
}

fn mles_process_msg(reader: io::ReadHalf<TcpStream>, hdr_key: Vec<u8>, message: Vec<u8>) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, Vec<u8>), std::io::Error> { 
    if 0 == message.len() { 
        return Err(Error::new(ErrorKind::BrokenPipe, "incorrect message len"));
    }
    Ok((reader, hdr_key, message))
}

fn mles_process_key(reader: io::ReadHalf<TcpStream>, mut hdr_key: Vec<u8>, hdr_len: usize, keyval: String, peer_addr: SocketAddr) -> Result<(io::ReadHalf<TcpStream>, Vec<u8>, usize), std::io::Error> { 
    let hkey;
    let key = hdr_key.split_off(HDRL);
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
    hdr_key.extend(key);
    Ok((reader, hdr_key, hdr_len))
}

fn mles_has_peer(peer: &SocketAddr) -> bool {
    0 != peer.port() 
}

