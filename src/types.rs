/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::SystemTime;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use warp::ws::Message;

#[derive(Serialize, Deserialize, Hash)]
pub(crate) struct MlesHeader {
    pub(crate) uid: String,
    pub(crate) channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auth: Option<String>,
}

pub(crate) struct ChannelInfo {
    pub(crate) messages: VecDeque<Message>,
    pub(crate) last_activity: SystemTime,
}

#[derive(Debug)]
pub(crate) enum WsEvent {
    Init(
        u64,
        u64,
        Sender<Option<Result<Message, warp::Error>>>,
        oneshot::Sender<u64>,
        Message,
    ),
    Msg(u64, u64, Message),
    Logoff(u64, u64),
}

#[derive(Debug)]
pub(crate) enum ReplyHeaders {
    NONE,
    Zstd,
    Br,
    AllowOrigin,
    ZstdWithAllowOrigin,
    BrWithAllowOrigin,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;
    use std::time::SystemTime;

    #[test]
    fn mlesheader_serde_roundtrip_with_auth() {
        let hdr = MlesHeader {
            uid: "user_a".to_string(),
            channel: "chan_x".to_string(),
            auth: Some("token123".to_string()),
        };

        let s = serde_json::to_string(&hdr).expect("serialize");
        let parsed: MlesHeader = serde_json::from_str(&s).expect("deserialize");

        assert_eq!(parsed.uid, "user_a");
        assert_eq!(parsed.channel, "chan_x");
        assert_eq!(parsed.auth.as_deref(), Some("token123"));
    }

    #[test]
    fn mlesheader_serde_roundtrip_without_auth() {
        let hdr = MlesHeader {
            uid: "user_b".to_string(),
            channel: "chan_y".to_string(),
            auth: None,
        };

        let s = serde_json::to_string(&hdr).expect("serialize");
        let parsed: MlesHeader = serde_json::from_str(&s).expect("deserialize");

        assert_eq!(parsed.uid, "user_b");
        assert_eq!(parsed.channel, "chan_y");
        assert!(parsed.auth.is_none());
    }

    #[test]
    fn channelinfo_message_push_pop() {
        let mut ch = ChannelInfo {
            messages: VecDeque::new(),
            last_activity: SystemTime::now(),
        };

        let m = Message::text("hello");
        ch.messages.push_back(m.clone());
        assert_eq!(ch.messages.len(), 1);

        let popped = ch.messages.pop_front().unwrap();
        assert_eq!(popped.as_bytes(), m.as_bytes());
    }

    #[test]
    fn wsevent_variant_creation_and_matching() {
        // Create a Msg variant and ensure pattern matching works
        let msg = Message::text("hi");
        let evt = WsEvent::Msg(0x1, 0x2, msg.clone());

        match evt {
            WsEvent::Msg(uid, ch, m) => {
                assert_eq!(uid, 0x1);
                assert_eq!(ch, 0x2);
                assert_eq!(m.as_bytes(), msg.as_bytes());
            }
            _ => panic!("expected Msg variant"),
        }
    }

    #[test]
    fn replyheaders_debug_contains_variant_name() {
        let v = ReplyHeaders::Zstd;
        let s = format!("{:?}", v);
        assert!(s.contains("Zstd"));
    }
}
