// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020, 2022 MediaTek Inc.
// Descriptor layout and group order derived from mt76 at revision
// 41d5d1dd63111870ef74a5b9645416d98979a145, mt7921/mac.c,
// mt76_connac2_mac.h and mt76_connac.h. See LICENSE-mt7922-rx.txt.

//! Conservative extraction of an unencrypted, unaggregated 802.11 frame
//! from a Connac2 normal RX record. This is an offline parser, not a driver.
//! FCS presence is deliberately unspecified until real hardware is measured.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxError {
    Truncated,
    NotNormalData,
    InvalidDescriptor,
    UnsupportedLayout,
    BadFrame,
}

/// Borrow an 802.11 frame from a completed RX record. `record` must already
/// be bounded to the DMA completion's actual byte count by the driver.
/// Translated, encrypted and A-MSDU records are rejected rather than guessed.
pub fn raw_frame(record: &[u8]) -> Result<&[u8], RxError> {
    fn word(record: &[u8], index: usize) -> Result<u32, RxError> {
        let bytes = record.get(index * 4..index * 4 + 4).ok_or(RxError::Truncated)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    if record.len() < 24 { return Err(RxError::Truncated); }
    let dw0 = word(record, 0)?;
    let dw1 = word(record, 1)?;
    let dw2 = word(record, 2)?;
    let dw4 = word(record, 4)?;
    // PKT_TYPE_NORMAL or PKT_TYPE_NORMAL_MCU (mt76_connac.h).
    if !matches!((dw0 >> 27) & 0x1f, 2 | 8) { return Err(RxError::NotNormalData); }
    // Secondary band, checksum/MIC/FCS errors, malformed A-MSDU or length.
    if dw1 & ((1 << 25) | (1 << 26) | (1 << 27) | (1 << 28)) != 0
        || dw2 & ((1 << 23) | (1 << 24) | (1 << 25)) != 0 {
        return Err(RxError::BadFrame);
    }
    // A translated or protected frame requires additional reconstruction.
    if dw2 & (1 << 13) != 0 || dw1 & (0x1f << 16) != 0 || dw4 & 3 != 0 {
        return Err(RxError::UnsupportedLayout);
    }
    let group = |bit: u32| (dw1 & (1u32 << bit)) != 0u32;
    if group(15) && !group(13) { return Err(RxError::InvalidDescriptor); }
    // Groups 4, 1, 2, 3, then 5, as consumed by mt7921_mac_fill_rx.
    let mut offset = 24usize;
    for (present, bytes) in [(group(14), 16), (group(11), 16), (group(12), 8),
                             (group(13), 8), (group(15), 72)] {
        if present {
            offset += bytes;
            if offset >= record.len() { return Err(RxError::Truncated); }
        }
    }
    offset += (((dw2 >> 14) & 3) * 2) as usize;
    let frame = record.get(offset..).ok_or(RxError::Truncated)?;
    // Even a short 802.11 control frame has frame control, duration and RA.
    if frame.len() < 10 { return Err(RxError::Truncated); }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(dw1: u32, dw2: u32, dw4: u32, groups: usize, padding: usize) -> Vec<u8> {
        let mut data = vec![0; 24 + groups + padding];
        data[0..4].copy_from_slice(&(2u32 << 27).to_le_bytes());
        data[4..8].copy_from_slice(&dw1.to_le_bytes());
        data[8..12].copy_from_slice(&dw2.to_le_bytes());
        data[16..20].copy_from_slice(&dw4.to_le_bytes());
        data.extend_from_slice(&[0xd4, 0, 0, 0, 1, 2, 3, 4, 5, 6]); data
    }
    #[test] fn extracts_base_and_optional_groups_with_padding() {
        let frame = [0xd4, 0, 0, 0, 1, 2, 3, 4, 5, 6];
        assert_eq!(raw_frame(&record(0, 0, 0, 0, 0)).unwrap(), frame);
        let groups = (1 << 14) | (1 << 11) | (1 << 12) | (1 << 13) | (1 << 15);
        assert_eq!(raw_frame(&record(groups, 2 << 14, 0, 120, 4)).unwrap(), frame);
    }
    #[test] fn rejects_unhandled_or_damaged_records() {
        assert_eq!(raw_frame(&[0; 23]), Err(RxError::Truncated));
        assert_eq!(raw_frame(&record(1 << 15, 0, 0, 0, 0)), Err(RxError::InvalidDescriptor));
        assert_eq!(raw_frame(&record(0, 1 << 13, 0, 0, 0)), Err(RxError::UnsupportedLayout));
        assert_eq!(raw_frame(&record(1 << 16, 0, 0, 0, 0)), Err(RxError::UnsupportedLayout));
        assert_eq!(raw_frame(&record(0, 0, 1, 0, 0)), Err(RxError::UnsupportedLayout));
        assert_eq!(raw_frame(&record(1 << 27, 0, 0, 0, 0)), Err(RxError::BadFrame));
        let mut wrong_type = record(0, 0, 0, 0, 0); wrong_type[3] = 0;
        assert_eq!(raw_frame(&wrong_type), Err(RxError::NotNormalData));
        let mut partial = record(1 << 14, 0, 0, 16, 0); partial.truncate(35);
        assert_eq!(raw_frame(&partial), Err(RxError::Truncated));
    }
}
