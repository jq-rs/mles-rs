/**
 * Copyright (c) 2016 Alex Crichton
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software
 * is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 *
 * Mles-support Copyright (c) 2020  Mles developers
 *
 */
use bytes::BytesMut;
use std::io::{self};
use mles_utils::MsgHdr;

pub fn msg_decode(buf: &mut BytesMut) -> io::Result<Option<BytesMut>> {
    // HDRKEYL is header min size
    if buf.len() >= MsgHdr::get_hdrkey_len() {
        let msghdr = MsgHdr::decode(buf.to_vec());
        if msghdr.get_type() != b'M' {
            let len = buf.len();
            buf.split_to(len);
            return Ok(None);
        }
        let hdr_len = msghdr.get_len() as usize;
        if 0 == hdr_len {
            let len = buf.len();
            buf.split_to(len);
            return Ok(None);
        }
        let len = buf.len();
        if len < (MsgHdr::get_hdrkey_len() + hdr_len) {
            return Ok(None);
        }
        if MsgHdr::get_hdrkey_len() + hdr_len < len {
            buf.split_to(MsgHdr::get_hdrkey_len());
            return Ok(Some(buf.split_to(hdr_len)));
        }
        buf.split_to(MsgHdr::get_hdrkey_len());
        return Ok(Some(buf.split_to(hdr_len)));
    }
    Ok(None)
}

pub fn msg_encode(data: Vec<u8>, buf: &mut BytesMut) -> io::Result<()> {
        buf.extend_from_slice(&data[..]);
        Ok(())
}

