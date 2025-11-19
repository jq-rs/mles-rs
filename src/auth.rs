/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use siphasher::sip::SipHasher;
use std::hash::Hasher;

pub(crate) fn verify_auth(uid: &str, channel: &str, auth: Option<&str>, key: Option<&str>) -> bool {
    let mut hasher = SipHasher::new();
    hasher.write(uid.as_bytes());
    hasher.write(channel.as_bytes());

    if let Some(k) = key {
        // If a key is provided, auth is required and must match the hash including the key
        if let Some(auth) = auth {
            hasher.write(k.as_bytes());
            let hash = hasher.finish();
            let auth_hash = u64::from_str_radix(auth, 16).unwrap_or(0);
            hash == auth_hash
        } else {
            false
        }
    } else {
        // No key configured: allow anonymous unless an auth is provided that must match
        if let Some(auth) = auth {
            let hash = hasher.finish();
            let auth_hash = u64::from_str_radix(auth, 16).unwrap_or(0);
            return hash == auth_hash;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use siphasher::sip::SipHasher;
    use std::hash::Hasher;

    #[test]
    fn test_verify_auth_no_key_no_auth() {
        // No key provided -> anonymous allowed
        assert!(verify_auth("user1", "chan1", None, None));
    }

    #[test]
    fn test_verify_auth_with_key_and_matching_auth() {
        // With explicit key the auth must be provided and valid
        let uid = "user1";
        let channel = "chan1";
        let key = Some("secret");
        let mut hasher = SipHasher::new();
        hasher.write(uid.as_bytes());
        hasher.write(channel.as_bytes());
        hasher.write("secret".as_bytes());
        let hash = hasher.finish();
        let auth_str = format!("{:x}", hash);

        assert!(verify_auth(uid, channel, Some(&auth_str), key));
    }

    #[test]
    fn test_verify_auth_with_key_requires_auth() {
        // If a key is provided but no auth supplied, verification fails
        assert!(!verify_auth("u", "c", None, Some("k")));
    }

    #[test]
    fn test_verify_auth_with_wrong_auth_fails() {
        // Wrong auth string should fail even if key provided
        assert!(!verify_auth(
            "user",
            "chan",
            Some("deadbeef"),
            Some("secret")
        ));
    }
}
