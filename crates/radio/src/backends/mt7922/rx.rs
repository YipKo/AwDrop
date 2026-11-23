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
    InvalidAntennaMask,
    FrameTooLarge,
}

/// The FCS policy must come from a verified device configuration, never from
/// the descriptor's error bit. It is not known for this prototype yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fcs { Present, Absent }

/// Convert a completed RX record to the radiotap framing expected by the
/// common radio interface. No device-specific FCS default is assumed.
/// `antenna_mask` identifies enabled receive chains (bits 0 through 3).
pub fn radiotap_frame(record: &[u8], fcs: Fcs, antenna_mask: u8) -> Result<Vec<u8>, RxError> {
    if antenna_mask == 0 || antenna_mask & !0x0f != 0 {
        return Err(RxError::InvalidAntennaMask);
    }
    let frame = raw_frame(record)?;
    if fcs == Fcs::Present && frame.len() < 14 { return Err(RxError::Truncated); }
    let dw1 = u32::from_le_bytes(record[4..8].try_into().unwrap());
    let group = |bit: u32| dw1 & (1u32 << bit) != 0;
    // Group 3 has two words; in monitor mode group 5's seventh word
    // replaces group 3's second word as the RCPI source (mt7921/mac.c).
    let signal = if group(13) {
        let group3 = 24 + usize::from(group(14)) * 16 + usize::from(group(11)) * 16
            + usize::from(group(12)) * 8;
        let rcpi_offset = if group(15) { group3 + 8 + 24 } else { group3 + 4 };
        let rcpi = record.get(rcpi_offset..rcpi_offset + 4).ok_or(RxError::Truncated)?;
        rcpi.iter().enumerate().filter(|(i, _)| antenna_mask & (1 << i) != 0)
            .map(|(_, value)| ((*value as i16 - 220) / 2) as i8).max()
    } else { None };
    let header_len = if signal.is_some() { 10 } else { 9 };
    if frame.len() + header_len > crate::MAX_PAYLOAD { return Err(RxError::FrameTooLarge); }
    // Radiotap fields: flags followed by optional dBm antenna signal. Both
    // have one-byte alignment. FCS is bit 4 in the flags field.
    let mut output = Vec::with_capacity(header_len + frame.len());
    output.extend_from_slice(&[0, 0, header_len as u8, 0]);
    output.extend_from_slice(&(if signal.is_some() { 0x22u32 } else { 0x02u32 }).to_le_bytes());
    output.push(if fcs == Fcs::Present { 0x10 } else { 0 });
    if let Some(dbm) = signal { output.push(dbm as u8); }
    output.extend_from_slice(frame);
    Ok(output)
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
    #[test] fn wraps_frames_with_explicit_fcs_and_monitor_signal() {
        let mut base = record(0, 0, 0, 0, 0);
        let wrapped = radiotap_frame(&base, Fcs::Absent, 1).unwrap();
        assert_eq!(&wrapped[..9], &[0,0,9,0,2,0,0,0,0]);
        assert_eq!(&wrapped[9..], &base[24..]);
        assert_eq!(radiotap_frame(&base, Fcs::Present, 1), Err(RxError::Truncated));
        base.extend_from_slice(&[0; 4]);
        assert_eq!(radiotap_frame(&base, Fcs::Present, 1).unwrap()[8], 0x10);

        let mut normal = record(1 << 13, 0, 0, 8, 0);
        normal[28..32].copy_from_slice(&[100, 140, 160, 180]);
        let wrapped = radiotap_frame(&normal, Fcs::Absent, 0b0011).unwrap();
        assert_eq!(&wrapped[..10], &[0,0,10,0,0x22,0,0,0,0,(-40i8) as u8]);

        let mut monitor = record((1 << 13) | (1 << 15), 0, 0, 80, 0);
        monitor[28..32].copy_from_slice(&[220; 4]);
        monitor[56..60].copy_from_slice(&[100, 120, 140, 160]);
        assert_eq!(radiotap_frame(&monitor, Fcs::Absent, 0b0101).unwrap()[9], (-40i8) as u8);
        assert_eq!(radiotap_frame(&monitor, Fcs::Absent, 0), Err(RxError::InvalidAntennaMask));
        assert_eq!(radiotap_frame(&monitor, Fcs::Absent, 16), Err(RxError::InvalidAntennaMask));
        let mut oversized = record(0, 0, 0, 0, 0);
        oversized.resize(crate::MAX_PAYLOAD + 24, 0);
        assert_eq!(radiotap_frame(&oversized, Fcs::Absent, 1), Err(RxError::FrameTooLarge));
    }
}
