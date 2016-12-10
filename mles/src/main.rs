
extern crate futures;
extern crate tokio_core;

use futures::stream::Stream;
use tokio_core::reactor::Core;
use tokio_core::net::TcpListener;
use std::collections::HashMap;
use futures::Future;

mod userchannel;

fn main() {
    let mut core = Core::new().unwrap();
    let address = "127.0.0.1:8080".parse().unwrap();
    let listener = TcpListener::bind(&address, &core.handle()).unwrap();

    let addr = listener.local_addr().unwrap();
    println!("Listening for connections on {}", addr);

    let clients = listener.incoming();
    let welcomes = clients.map(|(socket, _peer_addr)| {
        {
            let mut channel_db = userchannel::ChannelDb{ channelname: "Rust".to_string(), users: HashMap::new(), values: Vec::new() };
            //channel_db = channel_db.join_channel(&socket, "Sampo");
            //let socket_db = channel_db.users.get_mut("Sampo").unwrap(); 
            tokio_core::io::write_all(socket, b"Moikka!\n")
        }
    });
    let handle = core.handle();
    let server = welcomes.for_each(|future| {
        handle.spawn(future.then(|_| Ok(())));
        Ok(())
    });

    core.run(server).unwrap();
}
