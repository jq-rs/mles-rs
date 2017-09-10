/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/. 
*
*  Copyright (C) 2017  Juhamatti Kuusisaari / Mles developers
* */
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

use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;
use tokio_io::io;
use tokio_io::AsyncRead;

use futures::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::{unbounded, UnboundedSender};

use local_db::*;
use frame::*;
use super::*;

const MAXWAIT: u64 = 10*60;
const WAITTIME: u64 = 5;

/// Initiate peer connection
pub fn peer_conn(hist_limit: usize, peer: SocketAddr, is_addr_set: bool, keyaddr: String, channel: String, msg: Vec<u8>, 
                 tx_peer_for_msgs: &UnboundedSender<(u64, String, UnboundedSender<Vec<u8>>, UnboundedSender<UnboundedSender<Vec<u8>>>)>,
                 debug_flags: u64) 
{
    let mut core = Core::new().unwrap();
    let loopcnt = Rc::new(RefCell::new(1));
    let mles_peer_db = Rc::new(RefCell::new(MlesPeerDb::new(hist_limit)));

    loop {
        let handle = core.handle();
        let channel = channel.clone();
        let tx_peer_for_msgs = tx_peer_for_msgs.clone();
        let mut msg = msg.clone();
        let keyaddr = keyaddr.clone();

        let tcp = TcpStream::connect(&peer, &handle);

        let (tx_orig_chan, rx_orig_chan) = unbounded();
        let (tx, rx) = unbounded();

        //save cid
        let peer_cid = set_peer_cid(read_cid_from_hdr(&msg));

        //distribute channels
        let _res = tx_peer_for_msgs.unbounded_send((peer_cid, channel, tx.clone(), tx_orig_chan.clone())).map_err(|err| { println!("Cannot send from peer: {}", err); () });

        let loopcnt_inner = loopcnt.clone();
        let mles_peer_db_inner = mles_peer_db.clone();
        let client = tcp.and_then(move |pstream| {
            let laddr = match pstream.local_addr() {
                Ok(laddr) => laddr,
                Err(_) => {
                    let addr = "0.0.0.0:0";
                    addr.parse::<SocketAddr>().unwrap()
                }
            };
            let _val = pstream.set_nodelay(true)
                              .map_err(|_| panic!("Cannot set peer to no delay"));
            let _val = pstream.set_keepalive(Some(Duration::new(::KEEPALIVE, 0)))
                              .map_err(|_| panic!("Cannot set keepalive"));
            let mut loopcnt = loopcnt_inner.borrow_mut();
            *loopcnt = 1;

            if debug_flags != 0 {
                println!("Successfully connected to peer");
            }

            //if history exists, send resync to peer
            let mut mles_peer_db = mles_peer_db_inner.borrow_mut();
            if mles_peer_db.get_messages_len() > 1 {
                let message = msg.split_off(HDRKEYL);
                if is_addr_set {
                    msg = update_key(Msg::decode(message.as_slice()), read_hdr_len(&msg), keyaddr, laddr);
                }
                //send resync message to peer
                let rmsg = ResyncMsg::new(mles_peer_db.get_messages());
                let resync_message = rmsg.encode();
                let size = resync_message.len();
                println!("Resync Size {}", size);
                if size <= MSGMAXSIZE {
                    msg = write_len_to_hdr(size, msg);
                    msg.extend(resync_message);
                }
                else {
                    msg.extend(message);
                }

                let _res = tx.unbounded_send(msg).map_err(|err| { println!("Cannot write to tx: {}", err); });
            }
            else { 
                let message = msg.split_off(HDRKEYL);
                if is_addr_set {
                    msg = update_key(Msg::decode(message.as_slice()), read_hdr_len(&msg), keyaddr, laddr);
                }
                msg.extend(message);

                //send message to peer
                let _res = tx.unbounded_send(msg.clone()).map_err(|err| { println!("Cannot write to tx: {}", err); });

                //push message to history
                mles_peer_db.add_message(msg);
            }

            let (reader, writer) = pstream.split();

            let mles_peer_db = mles_peer_db_inner.clone();
            let psocket_writer = rx.fold(writer, move |writer, msg| {
                //push message to history
                let mut mles_peer_db = mles_peer_db.borrow_mut();
                mles_peer_db.add_message(msg.clone());

                //send message forward
                let amt = io::write_all(writer, msg);
                let amt = amt.map(|(writer, _)| writer);
                amt.map_err(|_| ())
            });
            handle.spawn(psocket_writer.then(|_| {
                Ok(())
            }));

            let mles_peer_db = mles_peer_db_inner.clone();
            let tx_origs_reader = rx_orig_chan.for_each(move |tx_orig| {
                //save receiver side tx to db
                let mut mles_peer_db_once = mles_peer_db.borrow_mut();
                mles_peer_db_once.add_channel(tx_orig.clone());  

                //push history always to client
                for msg in mles_peer_db_once.get_messages().iter() {
                    let _res = tx_orig.unbounded_send(msg.clone()).map_err(|_| { 
                        //just ignore for now
                        () 
                    });
                }
                Ok(())
            });
            handle.spawn(tx_origs_reader.then(|_| {
                Ok(())
            }));

            let mles_peer_db = mles_peer_db_inner.clone();
            let iter = stream::iter_ok(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, vec![0;HDRKEYL]);
                let frame = frame.and_then(move |(reader, hdr_key)| process_hdr_dummy_key(reader, hdr_key));

                let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| {
                    let tframe = io::read_exact(reader, vec![0;hdr_len]);
                    tframe.and_then(move |(reader, message)| process_msg(reader, hdr_key, message)) 
                }); 

                let mles_peer_db_frame = mles_peer_db.clone();
                frame.map(move |(reader, mut hdr_key, message)| {
                    hdr_key.extend(message);

                    //send message forward
                    let mut mles_peer_db = mles_peer_db_frame.borrow_mut();
                    for tx_orig in mles_peer_db.get_channels().iter() {
                        let _res = tx_orig.unbounded_send(hdr_key.clone()).map_err(|err| { println!("Failed to send from peer: {}", err); () });
                    }
                    //push message to history
                    mles_peer_db.add_message(hdr_key);

                    reader
                })
            })
        });

        // execute server
        let _res = core.run(client).map_err(|err| { println!("Peer: {}", err); () });
         
        let mut mles_peer_db_clear = mles_peer_db.borrow_mut();
        mles_peer_db_clear.clear_channels();

        let mut loopcnt = loopcnt.borrow_mut();
        let mut wait = WAITTIME * *loopcnt;
        if wait > MAXWAIT {
            wait = MAXWAIT;
        }
        *loopcnt *= 2;

        println!("Connection failed. Please check for proper key. Retrying in {} s.", wait);
        thread::sleep(Duration::from_secs(wait));
    }
}

pub fn set_peer_cid(cid: u32) -> u64 {
    (cid as u64) | 1 << 32
}

fn update_key(decoded_message: Msg, len: usize, keyaddr: String, laddr: std::net::SocketAddr) -> Vec<u8> {
    let mut keys = Vec::new();
    keys.push(addr2str(&laddr));
    if !keyaddr.is_empty() {
        keys.push(keyaddr);
    }
    //create hash for verification
    keys.push(decoded_message.get_uid().to_string());
    keys.push(decoded_message.get_channel().to_string());
    let key = Some(do_hash(&keys));
    let msg = write_hdr_with_key(len, key.unwrap());
    msg
}

/// Check if an peer is defined
///
/// # Example
/// ```
/// use mles_utils::has_peer;
///
/// let sockaddr = None;
/// assert_eq!(false, has_peer(&sockaddr));
/// ```
pub fn has_peer(peer: &Option<SocketAddr>) -> bool {
   if let Some(peer) = *peer {
       return peer.port() != 0;
   }
   false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_peer_set_cid() {
        let val: u32 = 1;
        assert_eq!(0x0000000100000001, set_peer_cid(val));
    }

    #[test]
    fn test_has_peer() {
        let addr = Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0));
        assert_eq!(false, has_peer(&addr));
    }
}
