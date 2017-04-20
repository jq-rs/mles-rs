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
    channels: Option<HashMap<u64, UnboundedSender<Vec<u8>>>>,
    messages: Vec<Vec<u8>>,
    peer_tx: Option<UnboundedSender<UnboundedSender<Vec<u8>>>>, 
}

impl MlesDb {
    pub fn new() -> MlesDb {
        MlesDb {
            channels: None,
            messages: Vec::new(),
            peer_tx: None,
        }
    }

    pub fn get_channels(&self) -> Option<&HashMap<u64, UnboundedSender<Vec<u8>>>> {
        self.channels.as_ref()
    }

    pub fn get_messages(&self) -> &Vec<Vec<u8>> {
        &self.messages
    }

    pub fn get_peer_tx(&self) -> Option<&UnboundedSender<UnboundedSender<Vec<u8>>>> {
        self.peer_tx.as_ref()
    }

    pub fn add_channel(&mut self, cnt: u64, sender: UnboundedSender<Vec<u8>>) {
        if self.channels.is_none() {
            self.channels = Some(HashMap::new());
        }
        if let Some(ref mut channels) = self.channels {
            channels.insert(cnt, sender);
        }
    }

    pub fn rem_channel(&mut self, cnt: u64) {
        if let Some(ref mut channels) = self.channels {
            channels.remove(&cnt);
        }
    }

    pub fn get_channels_len(&mut self) -> usize {
        if let Some(ref mut channels) = self.channels {
            return channels.len();
        }
        0
    }

    pub fn add_message(&mut self, message: Vec<u8>) {
        self.messages.push(message);
    }

    pub fn set_peer_tx(&mut self, peer_tx: UnboundedSender<UnboundedSender<Vec<u8>>>) {
        self.peer_tx = Some(peer_tx);
    }
}

pub struct MlesPeerDb {
    channels: Vec<UnboundedSender<Vec<u8>>>,
    messages: Vec<Vec<u8>>,
}

impl MlesPeerDb {
    pub fn new() -> MlesPeerDb {
        MlesPeerDb {
            channels: Vec::new(),
            messages: Vec::new(),
        }
    }

    pub fn get_channels(&self) -> &Vec<UnboundedSender<Vec<u8>>> {
        &self.channels
    }

    pub fn add_channel(&mut self, channel: UnboundedSender<Vec<u8>>) {
        self.channels.push(channel);
    }

    pub fn add_message(&mut self, message: Vec<u8>) {
        self.messages.push(message);
    }

    pub fn get_messages(&self) -> &Vec<Vec<u8>> {
        &self.messages
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
        let mut mles_db = MlesDb::new();
        mles_db.add_message(msg);
        assert_eq!(1, mles_db.get_messages().len());
        assert_eq!(0, mles_db.get_channels_len());
        let channel = mles_db.get_channels();
        assert_eq!(true, channel.is_none());
    }

    #[test]
    fn test_new_peer_db() {
        let msg = "Message".to_string().into_bytes();
        let mut mles_peer = MlesPeerDb::new();
        assert_eq!(0, mles_peer.get_messages_len());
        mles_peer.add_message(msg);
        assert_eq!(1, mles_peer.get_messages_len());
    }
}
