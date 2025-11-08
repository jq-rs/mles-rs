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
