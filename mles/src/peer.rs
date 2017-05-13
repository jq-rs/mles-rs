/**
 *   Mles server peer
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

use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::io::Error;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;
use tokio_io::io;
use tokio_io::AsyncRead;

use futures::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::{unbounded, UnboundedSender};

use local_db::*;
use frame::*;

const HDRL: usize = 4; //hdr len
const KEYL: usize = 8; //key len
const HDRKEYL: usize = HDRL + KEYL;

const PEERAND: u64 = !(::KEYAND);

const MAXWAIT: u64 = 10*60;
const WAITTIME: u64 = 5;

pub fn peer_conn(hist_limit: usize, peer: SocketAddr, peer_key: u64, channel: String, msg: Vec<u8>, 
                 tx_peer_for_msgs: UnboundedSender<(u64, String, UnboundedSender<Vec<u8>>, UnboundedSender<UnboundedSender<Vec<u8>>>)>) 
{
    let mut core = Core::new().unwrap();
    let loopcnt = Arc::new(Mutex::new(1));

    loop {
        let mles_peer_db = Rc::new(RefCell::new(MlesPeerDb::new(hist_limit)));
        let handle = core.handle();
        let channel = channel.clone();
        let tx_peer_for_msgs = tx_peer_for_msgs.clone();
        let msg = msg.clone();

        let tcp = TcpStream::connect(&peer, &handle);

        let (tx_orig_chan, rx_orig_chan) = unbounded();
        let (tx, rx) = unbounded();

        //distribute channels
        let _res = tx_peer_for_msgs.send((peer_key, channel, tx.clone(), tx_orig_chan.clone())).map_err(|err| { println!("Cannot send from peer: {}", err); () });
        let _res = tx.send(msg).map_err(|err| { println!("Cannot write to tx: {}", err); });

        let loopcnt_inner = loopcnt.clone();
        let client = tcp.and_then(move |pstream| {
            let _val = pstream.set_nodelay(true)
                              .map_err(|_| panic!("Cannot set peer to no delay"));
            let _val = pstream.set_keepalive_ms(::KEEPALIVEMS)
                              .map_err(|_| panic!("Cannot set keepalive"));
            {
                let mut loopcnt = loopcnt_inner.lock().unwrap();
                *loopcnt = 1;
            }
            println!("Successfully connected to peer");

            let (reader, writer) = pstream.split();

            let mles_peer_db_inner = mles_peer_db.clone();
            let psocket_writer = rx.fold(writer, move |writer, msg| {
                //push message to history
                let mut mles_peer_db = mles_peer_db_inner.borrow_mut();
                mles_peer_db.add_message(msg.clone());

                //send message forward
                let amt = io::write_all(writer, msg);
                let amt = amt.map(|(writer, _)| writer);
                amt.map_err(|_| ())
            });
            handle.spawn(psocket_writer.then(|_| {
                Ok(())
            }));

            let mles_peer_db_inner = mles_peer_db.clone();
            let tx_origs_reader = rx_orig_chan.for_each(move |tx_orig| {
                //save receiver side tx to db
                let mut mles_peer_db_once = mles_peer_db_inner.borrow_mut();
                mles_peer_db_once.add_channel(tx_orig.clone());  

                //push history to client if not the first one (as peer will send the history then)
                if mles_peer_db_once.get_messages_len() > 1 {
                    for msg in mles_peer_db_once.get_messages().iter() {
                        let _res = tx_orig.send(msg.clone()).map_err(|err| { println!("Failed to send history from peer: {}", err); () });
                    }
                }
                Ok(())
            });
            handle.spawn(tx_origs_reader.then(|_| {
                Ok(())
            }));

            let mles_peer_db_inner = mles_peer_db.clone();
            let iter = stream::iter(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;HDRKEYL]);
                let frame = frame.and_then(move |(reader, hdr_key)| process_hdr_dummy_key(reader, hdr_key));

                let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| {
                    let tframe = io::read_exact(reader, vec![0;hdr_len]);
                    tframe.and_then(move |(reader, message)| process_msg(reader, hdr_key, message)) 
                }); 

                let mles_peer_db_frame = mles_peer_db_inner.clone();
                frame.map(move |(reader, mut hdr_key, message)| {
                    hdr_key.extend(message);

                    //send message forward
                    let mut mles_peer_db = mles_peer_db_frame.borrow_mut();
                    for tx_orig in mles_peer_db.get_channels().iter() {
                        let _res = tx_orig.send(hdr_key.clone()).map_err(|err| { println!("Failed to send from peer: {}", err); () });
                    }
                    //push message to history
                    mles_peer_db.add_message(hdr_key);

                    reader
                })
            })
        });

        // execute server
        let _res = core.run(client).map_err(|err| { println!("Peer: {}", err); () });
         
        let mut loopcnt = loopcnt.lock().unwrap();
        let mut wait = WAITTIME * *loopcnt;
        if wait > MAXWAIT {
            wait = MAXWAIT;
        }
        *loopcnt *= 2;

        println!("Connection failed. Please check for proper key. Retrying in {} s.", wait);
        thread::sleep(Duration::from_secs(wait));
    }
}

pub fn set_peer_key(peer_key: u64) -> u64 {
    let mut val = peer_key;
    val &= PEERAND;
    val
}

pub fn has_peer(peer: &Option<SocketAddr>) -> bool {
   if let Some(peer) = *peer {
       return peer.port() != 0;
   }
   return false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_peer_set_key() {
        let val: u64 = 1 << 41;
        assert_eq!(1 << 41 & PEERAND, set_peer_key(val));
    }

    #[test]
    fn test_has_peer() {
        let addr = Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0));
        assert_eq!(false, has_peer(&addr));
    }
}
