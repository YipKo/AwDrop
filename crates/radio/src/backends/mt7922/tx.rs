// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020, 2022 MediaTek Inc.
// Connac2 TXD/TXP layout and injected frame handling follow mt76_connac_mac.c,
// mt76_connac2_mac.h and mt7921/pci_mac.c at 41d5d1dd63111870ef74a5b9645416d98979a145.
// See LICENSE-mt7922-rx.txt.

//! Conservative MT7922 PCIe transmit metadata encoder.
//!
//! Encoding is not transmission. A hardware owner must first initialize the
//! matching WCID and MAC, enforce regulatory limits, program data rings,
//! synchronize DMA, handle TX-free tokens and establish a teardown fence.
//! No implementation currently submits these bytes to the device.

use std::io;

/// Connac2 long TXD (32 bytes) plus one hardware TXP (32 bytes).
pub const TXWI_LEN: usize = 64;
const MAX_FRAME: usize = 0x0fff; // MT_TXD_LEN_MASK, excludes FCS.

/// Legacy OFDM rate codes from mt76's rate table, encoded as half-Mbit/s.
fn ofdm_rate(half_mbit: u8) -> Option<u16> {
    let index = match half_mbit {
        12 => 11, 18 => 15, 24 => 10, 36 => 14,
        48 => 9, 72 => 13, 96 => 8, 108 => 12,
        _ => return None,
    };
    Some((1 << 6) | index) // MT_PHY_TYPE_OFDM, MT_TX_RATE_IDX.
}

/// A checked radio-link transmit request. Parsing only; no radio is opened.
pub struct RadiotapTx<'a> { pub frame: &'a [u8], pub rate_half_mbit: u8, pub no_ack: bool }

/// Decode the radiotap subset needed by the existing radio-link interface.
/// Only a supplied, already-configured 2.4/5 GHz channel is checked here;
/// channel permission and RF power must be established by the hardware owner.
/// Unknown rate-control and extended fields are rejected rather than ignored.
pub fn parse_radiotap(packet: &[u8], channel: u8) -> io::Result<RadiotapTx<'_>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid or unsupported transmit radiotap");
    if packet.len() < 8 || packet.len() > crate::MAX_PAYLOAD || packet[0] != 0 || packet[1] != 0 { return Err(invalid()); }
    let end = u16::from_le_bytes([packet[2], packet[3]]) as usize;
    let present = u32::from_le_bytes(packet[4..8].try_into().unwrap());
    // TSFT, flags, legacy rate, channel and TX flags only.
    if end < 8 || end > packet.len() || present & !0x800f != 0 { return Err(invalid()); }
    let frequency = match channel {
        1..=13 => 2407 + u16::from(channel) * 5,
        14 => 2484,
        32..=177 => 5000 + u16::from(channel) * 5,
        _ => return Err(invalid()),
    };
    let mut offset = 8usize;
    let mut rate = 24u8; // 12 Mbps legacy OFDM, as in the reference adapter.
    let mut no_ack = false;
    for (bit, alignment, size) in [(0, 8, 8), (1, 1, 1), (2, 1, 1), (3, 2, 4), (15, 2, 2)] {
        if present & (1 << bit) == 0 { continue; }
        offset = (offset + alignment - 1) & !(alignment - 1);
        let data = packet.get(offset..offset + size).filter(|_| offset + size <= end).ok_or_else(invalid)?;
        match bit {
            // TX frame contract excludes FCS; do not silently remove bytes.
            1 if data[0] != 0 => return Err(invalid()),
            2 => rate = data[0],
            3 if u16::from_le_bytes([data[0], data[1]]) != frequency => return Err(invalid()),
            15 => {
                let flags = u16::from_le_bytes([data[0], data[1]]);
                if flags & !0x0008 != 0 { return Err(invalid()); }
                no_ack = flags & 0x0008 != 0;
            },
            _ => (),
        }
        offset += size;
    }
    if ofdm_rate(rate).is_none() { return Err(invalid()); }
    let frame = &packet[end..];
    if frame.len() < 24 || frame.len() > MAX_FRAME { return Err(invalid()); }
    Ok(RadiotapTx { frame, rate_half_mbit: rate, no_ack })
}

/// Bridge the common radio-link radiotap contract to MT7922 PCIe metadata.
/// Returns only encoded bytes and the borrowed payload; never submits DMA.
pub fn encode_radiotap(packet: &[u8], channel: u8, payload_address: u64,
                       token: u16, wcid: u16, spe_index: u8) -> io::Result<([u8; TXWI_LEN], &[u8])> {
    let request = parse_radiotap(packet, channel)?;
    let txwi = encode(request.frame, payload_address, token, wcid,
                      request.rate_half_mbit, spe_index, request.no_ack)?;
    Ok((txwi, request.frame))
}

/// A firmware TX-free event releases a token; `status` is firmware's result,
/// not proof of a link-layer ACK or successful AirDrop transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxFree { pub token: u16, pub wcid: Option<u16>, pub status: u8, pub attempts: u16 }

/// Parse a complete, DMA-bounded Connac2 TXRX_NOTIFY packet. The declared
/// MSDU count excludes WCID-pair entries; each pair still consumes bytes.
/// Validate the entire notification before returning any tokens to a caller.
pub fn parse_tx_free(packet: &[u8]) -> io::Result<Vec<TxFree>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid MT7922 TX-free notification");
    if packet.len() < 8 { return Err(invalid()); }
    let header = u32::from_le_bytes(packet[..4].try_into().unwrap());
    if header >> 27 != 6 { return Err(invalid()); }
    let declared_bytes = (header & 0xffff) as usize;
    if declared_bytes < 8 || declared_bytes > packet.len() { return Err(invalid()); }
    let total = ((header >> 16) & 0x03ff) as usize;
    let mut cursor = 8usize;
    let mut current_wcid = None;
    let mut results = Vec::with_capacity(total);
    while results.len() < total {
        let bytes = packet.get(cursor..cursor + 4).filter(|_| cursor + 4 <= declared_bytes).ok_or_else(invalid)?;
        cursor += 4;
        let entry = u32::from_le_bytes(bytes.try_into().unwrap());
        if entry & (1 << 31) != 0 {
            current_wcid = Some(((entry >> 14) & 0x03ff) as u16);
            continue;
        }
        let token = ((entry >> 16) & 0x7fff) as u16;
        if results.iter().any(|result: &TxFree| result.token == token) { return Err(invalid()); }
        results.push(TxFree { token, wcid: current_wcid, status: ((entry >> 13) & 3) as u8,
            attempts: (entry & 0x1fff) as u16 });
    }
    Ok(results)
}

#[derive(Debug, Clone, Copy)]
struct TokenState { dma_done: bool, firmware_done: bool }

/// Tracks the two independent completion signals for each submitted buffer.
/// This does not prove a platform DMA fence after reset/removal; the caller
/// must retain the actual allocations whenever that fence is uncertain.
pub struct TxTokens { slots: Vec<Option<TokenState>> }
impl TxTokens {
    pub fn new(count: usize) -> io::Result<Self> {
        if count == 0 || count > 0x8000 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid TX token count"));
        }
        Ok(Self { slots: vec![None; count] })
    }
    /// Reserve before publishing the descriptor; no silent token reuse.
    pub fn reserve(&mut self, token: u16) -> io::Result<()> {
        let slot = self.slots.get_mut(token as usize).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "TX token outside pool"))?;
        if slot.is_some() { return Err(io::Error::other("TX token is already in flight")); }
        *slot = Some(TokenState { dma_done: false, firmware_done: false });
        Ok(())
    }
    /// Returns true only once both completion signals have been observed.
    pub fn dma_complete(&mut self, token: u16) -> io::Result<bool> {
        let slot = self.slots.get_mut(token as usize).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TX token outside pool"))?;
        let state = slot.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown TX token"))?;
        if state.dma_done { return Err(io::Error::new(io::ErrorKind::InvalidData, "duplicate DMA completion")); }
        state.dma_done = true;
        let ready = state.firmware_done;
        if ready { *slot = None; }
        Ok(ready)
    }
    /// Validate every token before applying any firmware completions, so a
    /// malformed batch cannot partially release buffers. Returns recyclable IDs.
    pub fn firmware_complete(&mut self, events: &[TxFree]) -> io::Result<Vec<u16>> {
        for (i, event) in events.iter().enumerate() {
            let state = self.slots.get(event.token as usize).and_then(Option::as_ref)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown TX-free token"))?;
            if state.firmware_done || events[..i].iter().any(|other| other.token == event.token) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "duplicate TX-free token"));
            }
        }
        let mut ready = Vec::new();
        for event in events {
            let slot = &mut self.slots[event.token as usize];
            let state = slot.as_mut().unwrap();
            state.firmware_done = true;
            if state.dma_done { *slot = None; ready.push(event.token); }
        }
        Ok(ready)
    }
}

/// Encode a single unencrypted, unfragmented action or data MPDU.
/// `frame` must not include FCS. Caller supplies a *device logical* address,
/// an allocated token, an initialized WCID, and a verified antenna SPE index.
/// The actual payload is not copied by this function. Token and DMA memory
/// must stay live until both DMA and firmware TX-free completions are handled.
pub fn encode(frame: &[u8], payload_address: u64, token: u16, wcid: u16,
              rate_half_mbit: u8, spe_index: u8, no_ack: bool) -> io::Result<[u8; TXWI_LEN]> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid or unsupported MT7922 transmit request");
    if frame.len() < 24 || frame.len() > MAX_FRAME || token >= 0x8000 || wcid > 1023 || spe_index > 31
        || payload_address > u32::MAX as u64
        || payload_address.checked_add(frame.len() as u64 - 1).filter(|v| *v <= u32::MAX as u64).is_none() {
        return Err(invalid());
    }
    let rate = ofdm_rate(rate_half_mbit).ok_or_else(invalid)?;
    let fc = u16::from_le_bytes([frame[0], frame[1]]);
    let seq_control = u16::from_le_bytes([frame[22], frame[23]]);
    let kind = ((fc >> 2) & 3) as u32;
    let subtype = ((fc >> 4) & 15) as u32;
    // Reject non-zero protocol version, fragmentation, encryption and all
    // unhandled control, management and data header forms.
    if fc & 0x4403 != 0 || seq_control & 15 != 0
        || !((kind == 0 && subtype == 13) || (kind == 2 && (subtype == 0 || subtype == 8))) {
        return Err(invalid());
    }
    let mut header_len = 24usize;
    let mut tid = 0u32;
    if kind == 0 {
        if fc & 0x8300 != 0 || frame.len() < 25 { return Err(invalid()); }
    } else {
        if fc & 0x0300 == 0x0300 { header_len += 6; }
        if subtype == 8 {
            if frame.len() < header_len + 2 { return Err(invalid()); }
            let qos = frame[header_len];
            if qos & 0x80 != 0 || qos & 0x0f > 7 { return Err(invalid()); }
            tid = (qos & 0x0f) as u32;
            header_len += 2;
            if fc & 0x8000 != 0 { header_len += 4; }
        } else if fc & 0x8000 != 0 { return Err(invalid()); }
    }
    if frame.len() < header_len { return Err(invalid()); }
    let multicast = frame[4] & 1 != 0;
    let seq = (seq_control >> 4) as u32;
    let words = [
        (0x10 << 25) | (frame.len() as u32 + 32), // CT, ALTX0
        (1 << 31) | (2 << 16) | ((header_len as u32 / 2) << 11) | (tid << 20) | u32::from(wcid),
        (1 << 31) | (1 << 13) | (u32::from(multicast) << 10) | (kind << 4) | subtype,
        (1 << 31) | (1 << 28) | (seq << 16) | (15 << 11) | u32::from(no_ack || multicast),
        0, 0, (u32::from(rate) << 16) | (1 << 2),
        (kind << 20) | (subtype << 16) | (u32::from(spe_index) << 11),
    ];
    let mut result = [0u8; TXWI_LEN];
    for (chunk, word) in result[..32].chunks_exact_mut(4).zip(words) { chunk.copy_from_slice(&word.to_le_bytes()); }
    result[32..34].copy_from_slice(&(token | 0x8000).to_le_bytes());
    result[40..44].copy_from_slice(&(payload_address as u32).to_le_bytes());
    result[44..46].copy_from_slice(&((frame.len() as u16) | 0x8000).to_le_bytes());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn word(data: &[u8], index: usize) -> u32 { u32::from_le_bytes(data[index*4..index*4+4].try_into().unwrap()) }
    fn action() -> Vec<u8> {
        let mut frame = vec![0u8; 30];
        frame[0] = 0xd0; frame[4] = 1; frame[22..24].copy_from_slice(&0x1230u16.to_le_bytes());
        frame
    }
    #[test] fn vendor_action_uses_connac2_injected_sequence_and_single_txp() {
        let tx = encode(&action(), 0x12345678, 42, 19, 12, 2, false).unwrap();
        assert_eq!(word(&tx, 0), 0x2000003e);
        assert_eq!(word(&tx, 1), 0x80026013);
        assert_eq!(word(&tx, 2), 0x8000240d);
        assert_eq!(word(&tx, 3), 0x91237801);
        assert_eq!(word(&tx, 6), 0x004b0004);
        assert_eq!(word(&tx, 7), 0x000d1000);
        assert_eq!(&tx[32..34], &[42, 0x80]);
        assert_eq!(&tx[40..46], &[0x78, 0x56, 0x34, 0x12, 30, 0x80]);
    }
    #[test] fn radio_link_radiotap_reaches_the_mt7922_encoder() {
        // FLAGS + RATE + CHANNEL + TX_FLAGS; channel 6, 6 Mbps, NOACK.
        let mut packet = vec![0,0,16,0, 0x0e,0x80,0,0, 0,12, 0x85,0x09,0,0, 8,0];
        packet.extend_from_slice(&action());
        let (tx, frame) = encode_radiotap(&packet, 6, 0x1000, 2, 19, 0).unwrap();
        assert_eq!(frame, action());
        assert_eq!(word(&tx, 6), 0x004b0004);
        assert_eq!(word(&tx, 3) & 1, 1);
        assert!(encode_radiotap(&packet, 44, 0x1000, 2, 19, 0).is_err());
        packet[8] = 0x10; // FCS flag conflicts with radio-link TX contract.
        assert!(encode_radiotap(&packet, 6, 0x1000, 2, 19, 0).is_err());
    }
    #[test] fn radiotap_rejects_extended_fields_and_truncated_headers() {
        let mut packet = vec![0,0,8,0,0,0,0,0x80]; packet.extend_from_slice(&action());
        assert!(parse_radiotap(&packet, 6).is_err());
        packet[7] = 0; packet[4] = 4; // rate advertised but outside header
        assert!(parse_radiotap(&packet, 6).is_err());
    }
    #[test] fn accepts_owl_rate_only_header_and_keeps_80211_sequence() {
        // OWL emits a 9-byte radiotap header with legacy 12 Mbps (value 24).
        let mut packet = vec![0,0,9,0,4,0,0,0,24];
        packet.extend_from_slice(&action());
        let (tx, payload) = encode_radiotap(&packet, 6, 0x2000, 4, 19, 0).unwrap();
        assert_eq!(payload, action());
        assert_eq!((word(&tx, 3) >> 16) & 0xfff, 0x123);
        assert_eq!(word(&tx, 6), 0x004a0004);
    }
    #[test] fn qos_data_validates_header_and_encodes_tid() {
        let mut frame = vec![0u8; 26]; frame[0] = 0x88; frame[24] = 5;
        let tx = encode(&frame, 0x1000, 0, 2, 48, 0, false).unwrap();
        assert_eq!((word(&tx, 1) >> 20) & 7, 5);
        assert_eq!((word(&tx, 1) >> 11) & 31, 13);
        frame[24] = 0x80; assert!(encode(&frame, 0x1000, 0, 2, 48, 0, false).is_err());
    }
    #[test] fn rejects_unsupported_frames_and_invalid_hardware_fields() {
        let mut frame = action();
        assert!(encode(&frame, u32::MAX as u64, 0, 0, 12, 0, false).is_err());
        assert!(encode(&frame, 0, 0x8000, 0, 12, 0, false).is_err());
        assert!(encode(&frame, 0, 0, 1024, 12, 0, false).is_err());
        assert!(encode(&frame, 0, 0, 0, 11, 0, false).is_err());
        frame[1] = 0x40; assert!(encode(&frame, 0, 0, 0, 12, 0, false).is_err());
        frame = action(); frame[22] |= 1; assert!(encode(&frame, 0, 0, 0, 12, 0, false).is_err());
    }
    #[test] fn tx_free_counts_tokens_separately_from_wcid_pairs() {
        let mut packet = Vec::new();
        packet.extend_from_slice(&(20u32 | (2 << 16) | (6 << 27)).to_le_bytes());
        packet.extend_from_slice(&[0; 4]);
        packet.extend_from_slice(&(0x80000000u32 | (19 << 14)).to_le_bytes());
        packet.extend_from_slice(&((42u32 << 16) | 2).to_le_bytes());
        packet.extend_from_slice(&((43u32 << 16) | (1 << 13) | 3).to_le_bytes());
        assert_eq!(parse_tx_free(&packet).unwrap(), [
            TxFree { token: 42, wcid: Some(19), status: 0, attempts: 2 },
            TxFree { token: 43, wcid: Some(19), status: 1, attempts: 3 },
        ]);
        packet.truncate(16); assert!(parse_tx_free(&packet).is_err());
    }
    #[test] fn tx_free_rejects_duplicate_and_truncated_tokens_before_return() {
        let mut packet = Vec::new();
        packet.extend_from_slice(&(16u32 | (2 << 16) | (6 << 27)).to_le_bytes());
        packet.extend_from_slice(&[0; 4]);
        packet.extend_from_slice(&(42u32 << 16).to_le_bytes());
        packet.extend_from_slice(&(42u32 << 16).to_le_bytes());
        assert!(parse_tx_free(&packet).is_err());
        packet[0] = 12; assert!(parse_tx_free(&packet).is_err());
    }
    #[test] fn token_reuse_requires_both_completion_signals_in_either_order() {
        let mut tokens = TxTokens::new(2).unwrap();
        let event = |token| TxFree { token, wcid: None, status: 0, attempts: 1 };
        tokens.reserve(0).unwrap(); tokens.reserve(1).unwrap();
        assert!(!tokens.dma_complete(0).unwrap());
        assert!(tokens.firmware_complete(&[event(1)]).unwrap().is_empty());
        assert!(tokens.reserve(0).is_err() && tokens.reserve(1).is_err());
        assert_eq!(tokens.firmware_complete(&[event(0)]).unwrap(), [0]);
        assert!(tokens.dma_complete(1).unwrap());
        tokens.reserve(0).unwrap(); tokens.reserve(1).unwrap();
    }
    #[test] fn invalid_firmware_batch_never_partially_releases_tokens() {
        let mut tokens = TxTokens::new(2).unwrap();
        let event = |token| TxFree { token, wcid: None, status: 0, attempts: 1 };
        tokens.reserve(0).unwrap(); tokens.dma_complete(0).unwrap();
        assert!(tokens.firmware_complete(&[event(0), event(1)]).is_err());
        assert!(tokens.reserve(0).is_err());
        assert_eq!(tokens.firmware_complete(&[event(0)]).unwrap(), [0]);
    }
}
