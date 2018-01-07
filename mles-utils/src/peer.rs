/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/. 
*
*  Copyright (C) 2017-2018  Juhamatti Kuusisaari / Mles developers
* */
use std::rc::Rc;
use std::cell::RefCell;
use std::iter;
use std::io::Error;
use std::io::ErrorKind;
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

use bytes::{BytesMut, Bytes};

use local_db::*;
use frame::*;
use super::*;

const MAXWAIT: u64 = 10*60;
const WAITTIME: u64 = 5;

/// Initiate peer connection
pub(crate) fn peer_conn(hist_limit: usize, peer: SocketAddr, is_addr_set: bool, keyaddr: String, channel: String, msg: Bytes, 
                        tx_peer_for_msgs: &UnboundedSender<(u64, String, UnboundedSender<Bytes>, UnboundedSender<UnboundedSender<Bytes>>)>,
                        tx_peer_remover: UnboundedSender<(String, u64)>,
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
        let (tx_removals, rx_removals) = unbounded();
        let mut cnt = 0;

        let peer_cid = set_peer_cid(read_cid_from_hdr(&msg));

        //distribute channels
        let _res = tx_peer_for_msgs.unbounded_send((peer_cid, channel.clone(), tx.clone(), tx_orig_chan.clone())).map_err(|err| { println!("Cannot send from peer: {}", err); () });

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
                println!("Successfully connected to peer with cid {:x}", clear_peer_cid(peer_cid));
            }

            //if history exists, send resync to peer
            let mut mles_peer_db = mles_peer_db_inner.borrow_mut();
            if mles_peer_db.get_messages_len() > 1 {
                let message = msg.split_off(HDRKEYL);
                if is_addr_set {
                    msg = update_key(Msg::decode(message.as_ref()), read_hdr_len(&msg.to_vec()), keyaddr, laddr);
                }
                //send resync message to peer
                let mut rbv = Vec::with_capacity(mles_peer_db.get_messages_len());
                for msge in mles_peer_db.get_messages() {
                    rbv.push(msge.to_vec());
                }
                let mut msg = BytesMut::from(msg.as_ref()); 
                let rmsg = ResyncMsg::new(&rbv);
                let resync_message = rmsg.encode();
                let size = resync_message.len();
                if size <= MSGMAXSIZE {
                    msg = write_len_to_hdr(size, msg);
                    msg.extend(resync_message);
                }
                else {
                    msg.extend(message);
                }
                let msgf = msg.freeze();

                let _res = tx.unbounded_send(msgf).map_err(|err| { println!("Cannot write to tx: {}", err); });
            }
            else { 
                let message = msg.split_off(HDRKEYL);
                if is_addr_set {
                    msg = update_key(Msg::decode(message.to_vec().as_slice()), read_hdr_len(&msg), keyaddr, laddr);
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
                mles_peer_db.add_tx_stats();

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
                cnt += 1;
                let mut mles_peer_db_once = mles_peer_db.borrow_mut();
                mles_peer_db_once.add_channel(cnt, tx_orig.clone());  

                //push history always to client
                for msg in mles_peer_db_once.get_messages().iter() {
                    let _res = tx_orig.unbounded_send(msg.clone()).map_err(|_| { 
                        //unlikely to happen, ignore
                        () 
                    });
                }
                Ok(())
            });
            handle.spawn(tx_origs_reader.then(|_| {
                Ok(())
            }));

            let mles_peer_db = mles_peer_db_inner.clone();
            let channel_removals = rx_removals.for_each(move |cid| {
                let mut mles_peer_db_once = mles_peer_db.borrow_mut();
                mles_peer_db_once.rem_channel(cid);
                Ok(())
            });
            handle.spawn(channel_removals.then(|_| {
                Ok(())
            }));

            let mles_peer_db = mles_peer_db_inner.clone();
            let tx_removals_inner = tx_removals.clone();
            let iter = stream::iter_ok(iter::repeat(()).map(Ok::<(), Error>));
            iter.fold(reader, move |reader, _| {
                let frame = io::read_exact(reader, BytesMut::from(vec![0;HDRKEYL]));
                let frame = frame.and_then(move |(reader, hdr_key)| process_hdr_dummy_key(reader, hdr_key));

                let frame = frame.and_then(move |(reader, hdr_key, hdr_len)| {
                    let mut hdr = BytesMut::from(vec![0;HDRKEYL+hdr_len]);
                    let message = hdr.split_off(HDRKEYL);
                    let tframe = io::read_exact(reader, message);
                    tframe.and_then(move |(reader, message)| {
                        hdr.copy_from_slice(hdr_key.as_ref());
                        process_msg(reader, hdr_key, message)
                    })
                }); 

                let mles_peer_db_frame = mles_peer_db.clone();
                let tx_removals = tx_removals_inner.clone();
                frame.map(move |(reader, mut hdr_key, message)| {
                    hdr_key.unsplit(message);
                    let msg = hdr_key.freeze();

                    //send message forward
                    let mut mles_peer_db = mles_peer_db_frame.borrow_mut();
                    for (cid, tx_orig) in mles_peer_db.get_channels().iter() {
                        let _res = tx_orig.unbounded_send(msg.clone()).map_err(|_| { 
                            let _rem = tx_removals.unbounded_send(cid.clone()).map_err(|_| {
                                ()
                            });
                        });
                    }
                    //push message to history
                    mles_peer_db.add_message(msg);
                    mles_peer_db.add_rx_stats();

                    reader
                })
            })
        });

        // execute server
        let res = core.run(client).map_err(|err| { 
            println!("Peer: {}", err); 
            err
        });
        match res {
            Err(err) => {
                let mles_peer_db = mles_peer_db.borrow();
                if err.kind() == ErrorKind::UnexpectedEof && 0 == mles_peer_db.get_rx_stats() {
                    //we got reset directly from other side
                    //let's wrap our things as it is bad
                    let _res = tx_peer_remover.unbounded_send((channel, peer_cid));
                    println!("Connection failed. Please check for proper key or duplicate user.");
                    return;
                }
            },
            Ok(_) => {}
        }
         
        let mut mles_peer_db_clear = mles_peer_db.borrow_mut();
        mles_peer_db_clear.clear_channels();

        let mut loopcnt = loopcnt.borrow_mut();
        let mut wait = WAITTIME * *loopcnt;
        if wait > MAXWAIT {
            wait = MAXWAIT;
        }
        if wait == MAXWAIT {
            mles_peer_db_clear.clear_stats();
        }
        *loopcnt *= 2;

        println!("Connection failed. Retrying in {} s.", wait);
        thread::sleep(Duration::from_secs(wait));
    }
}

pub(crate) fn set_peer_cid(cid: u32) -> u64 {
    (cid as u64) | 1 << 32
}

pub(crate) fn clear_peer_cid(cid: u64) -> u64 {
    (cid as u32) as u64
}

fn update_key(decoded_message: Msg, len: usize, keyaddr: String, laddr: std::net::SocketAddr) -> Bytes {
    let mut keys = Vec::new();
    keys.push(MsgHdr::addr2str(&laddr));
    if !keyaddr.is_empty() {
        keys.push(keyaddr);
    }
    //create hash for verification
    keys.push(decoded_message.get_uid().to_string());
    keys.push(decoded_message.get_channel().to_string());
    let key = Some(MsgHdr::do_hash(&keys));
    let msg = write_hdr_with_key(len, key.unwrap());
    Bytes::from(msg)
}

/// Check if a peer is defined
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
