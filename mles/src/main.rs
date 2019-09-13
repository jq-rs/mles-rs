/* This Source Code Form is subject to the terms of the Mozilla Public
*  License, v. 2.0. If a copy of the MPL was not distributed with this
*  file, You can obtain one at http://mozilla.org/MPL/2.0/.
*
*  Copyright (C) 2017-2018  Mles developers
* */

use mles_utils::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::{env, process};

const SRVPORT: &str = ":8077";

const USAGE: &str = "Usage: mles [peer-address] [--history-limit=N]";
const HISTLIMIT: usize = 100;

fn main() {
    let addr = "0.0.0.0:8077";
    let addr = addr.parse::<SocketAddr>().unwrap();
    let mut peer: Option<SocketAddr> = None;
    let mut hist_limit = HISTLIMIT;
    for (argcnt, item) in env::args().enumerate() {
        if argcnt < 1 {
            continue;
        }
        let this_arg = item.clone();
        let v: Vec<&str> = this_arg.split("--history-limit=").collect();
        if v.len() > 1 {
            if let Some(limitstr) = v.get(1) {
                if let Ok(limit) = limitstr.parse::<usize>() {
                    hist_limit = limit;
                    println!("History limit: {}", hist_limit);
                    continue;
                }
            }
            println!("{}", USAGE);
            process::exit(1);
        } else {
            let peerarg = item + SRVPORT;
            let rpeer: Vec<_> = peerarg
                .to_socket_addrs()
                .unwrap_or_else(|_| {
                    vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)].into_iter()
                })
                .collect();
            let rpeer = *rpeer.first().unwrap();
            let rpeer = Some(rpeer);
            // check that we really got a proper peer
            if mles_utils::has_peer(&rpeer) {
                println!("Using peer domain {}", peerarg);
                peer = rpeer;
            } else {
                println!("Unable to resolve peer domain {}", peerarg);
            }
        }
        if argcnt > 2 {
            println!("{}", USAGE);
            process::exit(1);
        }
    }

    let keyval = match env::var("MLES_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    let keyaddr = match env::var("MLES_ADDR_KEY") {
        Ok(val) => val,
        Err(_) => "".to_string(),
    };

    server_run(addr, peer, keyval, keyaddr, hist_limit, 1);
}

#[cfg(test)]
mod tests {
    //use super::*;
}
