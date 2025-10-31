/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use async_compression::Level::Precise;
use async_compression::tokio::write::{BrotliEncoder, ZstdEncoder};
use tokio::io::AsyncWriteExt as _;

pub const BR: &str = "br";
pub const ZSTD: &str = "zstd";

pub async fn compress(comptype: &str, in_data: &[u8]) -> std::io::Result<Vec<u8>> {
    if comptype == ZSTD {
        let mut encoder = ZstdEncoder::with_quality(Vec::new(), Precise(2));
        encoder.write_all(in_data).await?;
        encoder.shutdown().await?;
        Ok(encoder.into_inner())
    } else {
        let mut encoder = BrotliEncoder::with_quality(Vec::new(), Precise(2));
        encoder.write_all(in_data).await?;
        encoder.shutdown().await?;
        Ok(encoder.into_inner())
    }
}