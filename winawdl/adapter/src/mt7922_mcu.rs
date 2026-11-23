// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020 MediaTek Inc.
// Connac2 layout follows mt76_connac2_mcu_fill_message, revision
// 41d5d1dd63111870ef74a5b9645416d98979a145. See LICENSE-mt7922-rx.txt.

//! Offline Connac2 MCU message encoding for the MT7921/MT7922 family.
//! Encoded bytes are not sent anywhere by this module.
use std::io;

pub const QUERY: u32 = 1 << 16;
pub const UNIFIED: u32 = 1 << 17;
pub const CE: u32 = 1 << 18;
pub const WA: u32 = 1 << 19;
pub const WM: u32 = 1 << 20;
pub const FW_SCATTER: u32 = 0xee;

/// Returns `(nonzero sequence, wire bytes)`. The caller must synchronize
/// sequence allocation and correlate responses. No hardware operation occurs.
pub fn encode(command: u32, sequence: &mut u8, payload: &[u8]) -> io::Result<(u8, Vec<u8>)> {
    if command & !0x1f_ffff != 0 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown command flags")); }
    let header = if command == FW_SCATTER { 0 } else if command & UNIFIED != 0 { 48 } else { 64 };
    if payload.len() > u16::MAX as usize - header || (header == 0 && payload.is_empty()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid MCU message length"));
    }
    let next = ((*sequence as u16 + 1) & 15) as u8;
    let next = if next == 0 { 1 } else { next };
    let mut out = vec![0u8; header + payload.len()];
    out[header..].copy_from_slice(payload);
    if header > 0 {
        let len = out.len() as u32;
        out[..4].copy_from_slice(&(0x4100_0000u32 | len).to_le_bytes());
        out[4..8].copy_from_slice(&0x8001_0000u32.to_le_bytes());
        out[32..34].copy_from_slice(&((len - 32) as u16).to_le_bytes());
        out[37] = 0xa0;
        out[39] = next;
        if command & UNIFIED != 0 {
            out[34..36].copy_from_slice(&((command & 0xff) as u16).to_le_bytes());
            // The mt7921 Connac2 path requests the unified set+ack option.
            out[43] = 7;
        } else {
            out[34..36].copy_from_slice(&0x8000u16.to_le_bytes());
            out[36] = command as u8;
            let ext = (command >> 8) as u8;
            out[41] = ext;
            out[38] = if ext != 0 || command & CE != 0 { if command & QUERY != 0 { 0 } else { 1 } } else { 3 };
            out[42] = if command & WA != 0 { 2 } else { 0 };
            out[43] = u8::from(ext != 0);
        }
    }
    *sequence = next;
    Ok((next, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn connac2_extended_query_matches_descriptor_layout() {
        let mut seq = 0;
        let (n, msg) = encode(0x01ed | QUERY, &mut seq, &[0xaa,0xbb,0xcc]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(&msg[..8], &[0x43,0,0,0x41,0,0,1,0x80]);
        assert_eq!(&msg[32..44], &[35,0,0,0x80,0xed,0xa0,0,1,0,1,0,1]);
        assert_eq!(&msg[64..], &[0xaa,0xbb,0xcc]);
    }
    #[test] fn unified_and_scatter_and_sequence_wrap() {
        let mut seq = 14;
        let (_, msg) = encode(UNIFIED | 7, &mut seq, &[1,2]).unwrap();
        assert_eq!(&msg[32..44], &[18,0,7,0,0,0xa0,0,15,0,0,0,7]);
        let (n, msg) = encode(FW_SCATTER, &mut seq, &[3,4]).unwrap();
        assert_eq!((n, msg), (1, vec![3,4]));
        assert!(encode(FW_SCATTER, &mut seq, &[]).is_err());
        assert_eq!(seq, 1);
        assert!(encode(1 << 25, &mut seq, &[1]).is_err());
        assert_eq!(seq, 1);
    }
}
