/* 
 * userchannel.rs
 *
 * All user and channel related functionality.
 *
 */
extern crate tokio_core;

use std::collections::HashMap;
use tokio_core::net::TcpListener;
use tokio_core::net::TcpStream;
//use std::net::TcpStream;

#[derive(Debug)]
pub struct ChannelDb<'a> {
    pub channelname: String,
    pub users: HashMap<String, &'a TcpStream>,
    pub values: Vec<Vec<String>>
}

impl<'a> ChannelDb<'a> {
    pub fn join_channel(mut self, socket: &'a TcpStream, userid: &str) -> ChannelDb<'a> {
        let user_exists = self.check_user_existence(userid);

        if !user_exists {
            self.users.insert(userid.to_string(), socket);
        }
        self
    }

    pub fn leave_channel(mut self, userid: &str, channelid: &str) -> ChannelDb<'a> {
        let user_exists = self.check_user_existence(userid);
        if user_exists {
            self.users.remove(userid);
        }
        self
    }

    pub fn check_user_existence(&'a self, userid: &str) -> bool {
        let opt = self.users.get(userid);
        match opt {
            Some(_) => return true,
            None => {}
        }
        return false;
    }

}

#[cfg(test)]

mod tests {
    use std::collections::HashMap;
    use std::net::TcpStream;
    use super::ChannelDb;

    #[test]
    fn test_join_channel() {
        // google should be up always
        let socket = TcpStream::connect("209.85.202.94:80").unwrap();
        {
            /* Arrange */
            let mut channel_db = ChannelDb{ channels: HashMap::new(), users: HashMap::new(), values: Vec::new() };
            let channel = "Rust";
            let user = "Sampo";
            let mut ret;

            /* Assert */
            ret = channel_db.check_user_existence(user);
            assert_eq!(false, ret);
            ret = channel_db.check_channel_existence(channel);
            assert_eq!(false, ret);

            /* Act */
            channel_db = channel_db.join_channel(&socket, user, channel);

            /* Assert */
            ret = channel_db.check_user_existence(user);
            assert_eq!(true, ret);
            ret = channel_db.check_channel_existence(channel);
            assert_eq!(true, ret);

            /* Negative */
            channel_db = channel_db.join_channel(&socket, user, channel);
            ret = channel_db.check_user_existence(user);
            assert_eq!(true, ret);
            ret = channel_db.check_channel_existence(channel);
            assert_eq!(true, ret);
        }
    }

    #[test]
    fn test_leave_channel() {
        // google should be up always
        let socket = TcpStream::connect("209.85.202.94:80").unwrap();
        {
            let mut channel_db = ChannelDb{ channels: HashMap::new(), users: HashMap::new(), values: Vec::new() };
            let channel = "Mles";
            let user = "Anna";
            let mut ret;

            channel_db = channel_db.join_channel(&socket, user, channel);
            ret = channel_db.check_user_existence(user);
            assert_eq!(true, ret);
            ret = channel_db.check_channel_existence(channel);
            assert_eq!(true, ret);

            channel_db = channel_db.leave_channel(user, channel);
            ret = channel_db.check_user_existence(user);
            assert_eq!(false, ret);
            ret = channel_db.check_channel_existence(channel);
            assert_eq!(false, ret);

            channel_db = channel_db.leave_channel(user, channel);
            ret = channel_db.check_user_existence(user);
            assert_eq!(false, ret);
            ret = channel_db.check_channel_existence(channel);
            assert_eq!(false, ret);
        }
    }
}
