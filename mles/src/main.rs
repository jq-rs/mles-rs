/**
 *   Mles server
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
extern crate mles_utils;

use std::{process, env};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use mles_utils::*;


const SRVPORT: &str = ":8077";

const USAGE: &str = "Usage: mles [peer-address] [--history-limit=N]";
const HISTLIMIT: usize = 100;

fn main() {
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
                if let Ok(limit)= limitstr.parse::<usize>() {
                    hist_limit = limit;
                    println!("History limit: {}", hist_limit);
                    continue;
                }
            }
            println!("{}", USAGE);
            process::exit(1);
        }
        else {
            let peerarg = item + SRVPORT;
            let rpeer: Vec<_> = peerarg.to_socket_addrs()
                                       .unwrap_or_else(|_| vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)].into_iter())
                                       .collect();
            let rpeer = *rpeer.first().unwrap();
            let rpeer = Some(rpeer);
            // check that we really got a proper peer
            if mles_utils::peer::has_peer(&rpeer) {
                println!("Using peer domain {}", peerarg); 
                peer = rpeer;
            }
            else { 
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

    server_run(SRVPORT, keyval, keyaddr, peer, hist_limit);
}


#[cfg(test)]
mod tests {
    //use super::*;
}
