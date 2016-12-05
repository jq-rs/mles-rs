/* 
 * userchannel.rs
 *
 * All user and channel related functionality.
 *
 */
extern crate tokio_core;

use std::collections::HashMap;
//use tokio_core::net::TcpStream;
use std::net::TcpStream;

struct ChannelDb<'a> {
    channels: HashMap<String, ChannelDb<'a>>,
    users: HashMap<String, &'a TcpStream>,
    values: Vec<Vec<String>>
}

impl<'a> ChannelDb<'a> {
    pub fn join_channel(&'a mut self, socket: &'a TcpStream, userid: &str, channelid: &str) {
        let user_exists = self.check_user_existence(userid);

        if !user_exists {
            self.users.insert(userid.to_string(), socket);
        }

        let channel_exists = self.check_channel_existence(channelid);
        if !channel_exists {
            let mut channel = ChannelDb{ channels: HashMap::new(), users: HashMap::new(), values: Vec::new() };
            self.channels.insert(channelid.to_string(), channel);
        }
    }

    pub fn leave_channel(&'a mut self, userid: &str, channelid: &str) {
        let user_exists = self.check_user_existence(userid);
        if user_exists {
            let channel_exists = self.check_channel_existence(channelid);
            if channel_exists {
                self.channels.remove(channelid);
            }
            self.users.remove(userid);
        }
    }

    fn check_user_existence(&'a self, userid: &str) -> bool {
        let opt = self.users.get(userid);
        match opt {
            Some(db) => return true,
                None => {}
        }
        return false;
    }
    fn check_channel_existence(&'a self, channelid: &str) -> bool {
        let opt = self.channels.get(channelid);
        match opt {
            Some(db) => return true,
                None => {}
        }
        return false;
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_join_channel() {
        let mut user_chan_db = HashMap::new();
        let channel = "Rust";
        let user = "Sampo";
        let mut ret;

        ret = join_channel(user_chan_db, user, channel);
        assert_eq!(0, ret);

        ret = join_channel(user_chan_db, user, channel);
        assert_eq!(1, ret);
    }

    fn test_leave_channel() {
        let mut user_chan_db = HashMap::new();
        let channel = "Evolve";
        let user = "Anna";
        let mut ret;

        ret = join_channel(user_chan_db, user, channel);
        assert_eq!(0, ret);

        ret = leave_channel(user_chan_db, user, channel);
        assert_eq!(0, ret);

        ret = leave_channel(user_chan_db, user, channel);
        assert_eq!(1, ret);

    }
}
