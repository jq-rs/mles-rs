/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use siphasher::sip::SipHasher;
use std::hash::Hasher;

pub fn verify_auth(uid: &str, channel: &str, auth: Option<&str>) -> bool {
    let mut hasher = SipHasher::new();
    hasher.write(uid.as_bytes());
    hasher.write(channel.as_bytes());

    if let Ok(key) = std::env::var("MLES_KEY") {
        if let Some(auth) = auth {
            hasher.write(key.as_bytes());
            let hash = hasher.finish();
            let auth_hash = u64::from_str_radix(auth, 16).unwrap_or(0);
            hash == auth_hash
        } else {
            false
        }
    } else {
        if let Some(auth) = auth {
            let hash = hasher.finish();
            let auth_hash = u64::from_str_radix(auth, 16).unwrap_or(0);
            return hash == auth_hash;
        }
        true
    }
}
