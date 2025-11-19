/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use async_compression::Level::Precise;
use async_compression::tokio::write::{BrotliEncoder, ZstdEncoder};
use tokio::io::AsyncWriteExt as _;

pub(crate) const BR: &str = "br";
pub(crate) const ZSTD: &str = "zstd";

pub(crate) async fn compress(comptype: &str, in_data: &[u8]) -> std::io::Result<Vec<u8>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Smoke test to ensure both compressors produce output for small input.
    #[tokio::test]
    async fn test_compress_br_and_zstd_smoke() {
        let data = b"Hello compression test".to_vec();
        let b = compress(BR, &data).await.expect("br compress");
        let z = compress(ZSTD, &data).await.expect("zstd compress");
        assert!(!b.is_empty(), "brotli output should not be empty");
        assert!(!z.is_empty(), "zstd output should not be empty");
    }
}
