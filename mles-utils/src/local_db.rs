/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/.
*
*  Copyright (C) 2017-2018  Mles developers
* */
use futures::sync::mpsc::UnboundedSender;
use std::collections::HashMap;

use bytes::Bytes;

pub(crate) struct MlesDb {
    channels: Option<HashMap<u64, UnboundedSender<Bytes>>>,
    messages: Vec<Bytes>,
    peer_tx: Option<UnboundedSender<UnboundedSender<Bytes>>>,
    history_limit: usize,
    tx_db: Vec<UnboundedSender<Bytes>>,
}

impl MlesDb {
    pub fn new(hlim: usize) -> MlesDb {
        MlesDb {
            channels: None,
            messages: Vec::with_capacity(hlim),
            peer_tx: None,
            history_limit: hlim,
            tx_db: Vec::new(),
        }
    }

    pub fn get_channels(&self) -> Option<&HashMap<u64, UnboundedSender<Bytes>>> {
        self.channels.as_ref()
    }

    pub fn get_messages(&self) -> &Vec<Bytes> {
        &self.messages
    }

    pub fn get_messages_len(&self) -> usize {
        self.messages.len()
    }

    pub fn get_tx_db(&self) -> &Vec<UnboundedSender<Bytes>> {
        &self.tx_db
    }

    pub fn clear_tx_db(&mut self) {
        self.tx_db = Vec::new();
    }

    pub fn add_tx_db(&mut self, tx: UnboundedSender<Bytes>) {
        self.tx_db.push(tx);
    }

    pub fn get_history_limit(&self) -> usize {
        self.history_limit
    }

    pub fn get_peer_tx(&self) -> Option<&UnboundedSender<UnboundedSender<Bytes>>> {
        self.peer_tx.as_ref()
    }

    pub fn add_channel(&mut self, cid: u64, sender: UnboundedSender<Bytes>) {
        if self.channels.is_none() {
            self.channels = Some(HashMap::new());
        }
        if let Some(ref mut channels) = self.channels {
            channels.insert(cid, sender);
        }
    }

    pub fn check_for_duplicate_cid(&self, cid: u32) -> bool {
        if let Some(ref channels) = self.channels {
            if channels.contains_key(&(cid as u64)) {
                return true;
            }
        }
        false
    }

    pub fn rem_channel(&mut self, cid: u64) {
        if let Some(ref mut channels) = self.channels {
            channels.remove(&cid);
        }
    }

    pub fn get_channels_len(&mut self) -> usize {
        if let Some(ref channels) = self.channels {
            return channels.len();
        }
        0
    }

    pub fn add_message(&mut self, message: Bytes) {
        if 0 == self.get_history_limit() {
            return;
        }
        if self.messages.len() == self.get_history_limit() {
            self.messages.remove(0);
        }
        self.messages.push(message);
    }

    pub fn set_peer_tx(&mut self, peer_tx: UnboundedSender<UnboundedSender<Bytes>>) {
        self.peer_tx = Some(peer_tx);
    }

    pub fn rem_peer_tx(&mut self) {
        self.peer_tx = None;
    }

    pub fn check_peer(&self) -> bool {
        match self.peer_tx {
            Some(_) => true,
            None => false,
        }
    }
}

pub(crate) struct MlesPeerDb {
    channels: HashMap<u64, UnboundedSender<Bytes>>,
    messages: Vec<Bytes>,
    history_limit: usize,
    rx_stats: u64,
    tx_stats: u64,
}

impl MlesPeerDb {
    pub fn new(hlim: usize) -> MlesPeerDb {
        MlesPeerDb {
            channels: HashMap::new(),
            messages: Vec::with_capacity(hlim),
            history_limit: hlim,
            rx_stats: 0,
            tx_stats: 0,
        }
    }

    pub fn get_channels(&self) -> &HashMap<u64, UnboundedSender<Bytes>> {
        &self.channels
    }

    pub fn add_channel(&mut self, cid: u64, channel: UnboundedSender<Bytes>) {
        self.channels.insert(cid, channel);
    }

    pub fn rem_channel(&mut self, cid: u64) {
        self.channels.remove(&cid);
    }

    pub fn clear_channels(&mut self) {
        self.channels = HashMap::new();
    }

    pub fn clear_stats(&mut self) {
        self.rx_stats = 0;
        self.tx_stats = 0;
    }

    pub fn add_message(&mut self, message: Bytes) {
        if 0 == self.get_history_limit() {
            return;
        }
        if self.messages.len() == self.get_history_limit() {
            self.messages.remove(0);
        }
        self.messages.push(message);
    }

    pub fn get_messages(&self) -> &Vec<Bytes> {
        &self.messages
    }

    pub fn get_history_limit(&self) -> usize {
        self.history_limit
    }

    pub fn get_messages_len(&self) -> usize {
        self.messages.len()
    }

    pub fn add_rx_stats(&mut self) {
        self.rx_stats += 1;
    }

    pub fn add_tx_stats(&mut self) {
        self.tx_stats += 1;
    }

    pub fn get_rx_stats(&self) -> u64 {
        self.rx_stats
    }

    //pub fn get_tx_stats(&self) -> u64 {
    //    self.tx_stats
    //}
}

#[cfg(test)]

mod tests {
    use super::*;
    const HISTLIMIT: usize = 100;

    #[test]
    fn test_new_db() {
        let msg = "Message".to_string().into_bytes();
        let mut mles_db = MlesDb::new(HISTLIMIT);
        mles_db.add_message(Bytes::from(msg));
        assert_eq!(1, mles_db.get_messages().len());
        assert_eq!(0, mles_db.get_channels_len());
        let channel = mles_db.get_channels();
        assert_eq!(true, channel.is_none());
    }

    #[test]
    fn test_new_peer_db() {
        let msg = "Message".to_string().into_bytes();
        let mut mles_peer = MlesPeerDb::new(HISTLIMIT);
        assert_eq!(0, mles_peer.get_messages_len());
        mles_peer.add_message(Bytes::from(msg));
        assert_eq!(1, mles_peer.get_messages_len());
    }

    #[test]
    fn test_db_history_limit() {
        let mut limit = HISTLIMIT;
        let msg = "Message".to_string().into_bytes();
        let mut mles_peer = MlesPeerDb::new(limit);
        assert_eq!(0, mles_peer.get_messages_len());
        mles_peer.add_message(Bytes::from(msg.clone()));
        assert_eq!(1, mles_peer.get_messages_len());
        while limit > 0 {
            mles_peer.add_message(Bytes::from(msg.clone()));
            limit -= 1;
        }
        assert_eq!(HISTLIMIT, mles_peer.get_messages_len());
        assert_eq!(HISTLIMIT, mles_peer.get_history_limit());
    }
}
