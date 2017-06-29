/**
 *   Mles server database 
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
use std::collections::HashMap;
use futures::sync::mpsc::UnboundedSender;

pub struct MlesDb {
    channels: Option<HashMap<u32, UnboundedSender<Vec<u8>>>>,
    messages: Vec<Vec<u8>>,
    peer_tx: Option<UnboundedSender<UnboundedSender<Vec<u8>>>>, 
    history_limit: usize,
    tx_db: Vec<UnboundedSender<Vec<u8>>>,
}

impl MlesDb {
    pub fn new(hlim: usize) -> MlesDb {
        MlesDb {
            channels: None,
            messages: Vec::new(),
            peer_tx: None,
            history_limit: hlim,
            tx_db: Vec::new(),
        }
    }

    pub fn get_channels(&self) -> Option<&HashMap<u32, UnboundedSender<Vec<u8>>>> {
        self.channels.as_ref()
    }

    pub fn get_messages(&self) -> &Vec<Vec<u8>> {
        &self.messages
    }

    pub fn get_tx_db(&self) -> &Vec<UnboundedSender<Vec<u8>>> {
        &self.tx_db
    }

    pub fn clear_tx_db(&mut self) {
        self.tx_db = Vec::new();
    }

    pub fn add_tx_db(&mut self, tx: UnboundedSender<Vec<u8>>) {
        &self.tx_db.push(tx);
    }

    pub fn get_history_limit(&self) -> usize {
        self.history_limit
    }

    pub fn get_peer_tx(&self) -> Option<&UnboundedSender<UnboundedSender<Vec<u8>>>> {
        self.peer_tx.as_ref()
    }

    pub fn add_channel(&mut self, cid: u32, sender: UnboundedSender<Vec<u8>>) {
        if self.channels.is_none() {
            self.channels = Some(HashMap::new());
        }
        if let Some(ref mut channels) = self.channels {
            channels.insert(cid, sender);
        }
    }

    pub fn check_for_duplicate_cid(&self, cid: u32) -> bool {
        if let Some(ref channels) = self.channels {
            if channels.contains_key(&cid) {
                return true;
            }
        }
        false
    }

    pub fn rem_channel(&mut self, cid: u32) {
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

    pub fn add_message(&mut self, message: Vec<u8>) {
        if 0 == self.get_history_limit() {
            return;
        }
        if self.messages.len() == self.get_history_limit() {
            self.messages.remove(0);
        }
        self.messages.push(message);
    }

    pub fn set_peer_tx(&mut self, peer_tx: UnboundedSender<UnboundedSender<Vec<u8>>>) {
        self.peer_tx = Some(peer_tx);
    }
}

pub struct MlesPeerDb {
    channels: Vec<UnboundedSender<Vec<u8>>>,
    messages: Vec<Vec<u8>>,
    history_limit: usize
}

impl MlesPeerDb {
    pub fn new(hlim: usize) -> MlesPeerDb {
        MlesPeerDb {
            channels: Vec::new(),
            messages: Vec::new(),
            history_limit: hlim,
        }
    }

    pub fn get_channels(&self) -> &Vec<UnboundedSender<Vec<u8>>> {
        &self.channels
    }

    pub fn add_channel(&mut self, channel: UnboundedSender<Vec<u8>>) {
        self.channels.push(channel);
    }

    pub fn clear_channels(&mut self) {
        self.channels = Vec::new();
    }

    pub fn add_message(&mut self, message: Vec<u8>) {
        if 0 == self.get_history_limit() {
            return;
        }
        if self.messages.len() == self.get_history_limit() {
            self.messages.remove(0);
        }
        self.messages.push(message);
    }

    pub fn get_messages(&self) -> &Vec<Vec<u8>> {
        &self.messages
    }

    pub fn get_history_limit(&mut self) -> usize {
        self.history_limit
    }

    pub fn get_messages_len(&mut self) -> usize {
        self.messages.len()
    }

}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_new_db() {
        let msg = "Message".to_string().into_bytes();
        let mut mles_db = MlesDb::new(::HISTLIMIT);
        mles_db.add_message(msg);
        assert_eq!(1, mles_db.get_messages().len());
        assert_eq!(0, mles_db.get_channels_len());
        let channel = mles_db.get_channels();
        assert_eq!(true, channel.is_none());
    }

    #[test]
    fn test_new_peer_db() {
        let msg = "Message".to_string().into_bytes();
        let mut mles_peer = MlesPeerDb::new(::HISTLIMIT);
        assert_eq!(0, mles_peer.get_messages_len());
        mles_peer.add_message(msg);
        assert_eq!(1, mles_peer.get_messages_len());
    }

    #[test]
    fn test_db_history_limit() {
        let mut limit = ::HISTLIMIT;
        let msg = "Message".to_string().into_bytes();
        let mut mles_peer = MlesPeerDb::new(limit);
        assert_eq!(0, mles_peer.get_messages_len());
        mles_peer.add_message(msg.clone());
        assert_eq!(1, mles_peer.get_messages_len());
        while limit > 0 {
            mles_peer.add_message(msg.clone());
            limit -= 1;
        }
        assert_eq!(::HISTLIMIT, mles_peer.get_messages_len());
        assert_eq!(::HISTLIMIT, mles_peer.get_history_limit());
    }
}
