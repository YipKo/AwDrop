//! Offline assembler for one observed MediaTek stock-driver WPP RX format.
//!
//! This code does not start ETW, access a device, change a channel, or provide
//! TX. The caller must independently verify the driver and message GUID, feed
//! events in trace order, and detect ETW event loss. A decoded frame is not an
//! AWDL connection. The format has been replayed from one MT7922 / driver
//! version; it is not a stable vendor API.

use std::{collections::HashMap, io};

use crate::MAX_PAYLOAD;

const MAX_PENDING_THREADS: usize = 64;
const PCAP_HEADER: u16 = 0xaf;
const RECORD_HEADER: u16 = 0xb1;
const FRAME_BYTES: u16 = 0xb3;

#[derive(Debug)]
struct Pending {
    captured_len: usize,
    radiotap: Option<Vec<u8>>,
}

/// Assembles ordered WPP fragments into bounded radiotap + 802.11 frames.
///
/// Any malformed relevant event permanently fails the assembler. ETW can lose
/// events: a caller must also watch loss counters, and discard the entire
/// session after loss; this assembler cannot detect every dropped fragment.
#[derive(Debug, Default)]
pub struct FrameAssembler {
    header_seen: bool,
    failed: bool,
    pending: HashMap<u32, Pending>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn le16(bytes: &[u8]) -> u16 { u16::from_le_bytes([bytes[0], bytes[1]]) }
fn le32(bytes: &[u8]) -> u32 { u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) }

impl FrameAssembler {
    pub fn new() -> Self { Self::default() }

    /// `user_data` contains the observed two-byte length prefix and fragment.
    /// Unrelated message IDs are ignored; message GUID filtering is the caller's
    /// responsibility. The supplied thread ID must come from the WPP event.
    pub fn ingest(&mut self, message: u16, thread: u32, user_data: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if self.failed { return Err(invalid("assembler previously failed")); }
        if !matches!(message, PCAP_HEADER | RECORD_HEADER | FRAME_BYTES | 0xa7..=0xad) {
            return Ok(None);
        }
        let result = self.ingest_inner(message, thread, user_data);
        if result.is_err() { self.failed = true; self.pending.clear(); }
        result
    }

    fn ingest_inner(&mut self, message: u16, thread: u32, user_data: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if user_data.len() < 2 { return Err(invalid("short WPP fragment length")); }
        let length = le16(user_data) as usize;
        let fragment = &user_data[2..];
        if length != fragment.len() { return Err(invalid("WPP fragment length mismatch")); }
        match message {
            PCAP_HEADER => {
                if self.header_seen || !self.pending.is_empty() || fragment.len() != 24 {
                    return Err(invalid("invalid or repeated PCAP header"));
                }
                if le32(&fragment[0..4]) != 0xa1b2c3d4 || le16(&fragment[4..6]) != 2 ||
                    le16(&fragment[6..8]) != 4 || le32(&fragment[20..24]) != 127 {
                    return Err(invalid("unsupported PCAP format"));
                }
                // This driver has emitted snaplen=0; use our own MAX_PAYLOAD
                // bound rather than trusting or modifying the driver header.
                self.header_seen = true;
                Ok(None)
            }
            RECORD_HEADER => {
                if !self.header_seen || fragment.len() != 16 || self.pending.contains_key(&thread) {
                    return Err(invalid("unexpected PCAP record header"));
                }
                if self.pending.len() >= MAX_PENDING_THREADS { return Err(invalid("too many pending threads")); }
                let micros = le32(&fragment[4..8]);
                let captured_len = le32(&fragment[8..12]) as usize;
                let original_len = le32(&fragment[12..16]) as usize;
                if micros >= 1_000_000 || captured_len < 10 || captured_len > MAX_PAYLOAD || captured_len > original_len {
                    return Err(invalid("invalid PCAP record length or timestamp"));
                }
                self.pending.insert(thread, Pending { captured_len, radiotap: None });
                Ok(None)
            }
            0xa7..=0xad => {
                let pending = self.pending.get_mut(&thread).ok_or_else(|| invalid("radiotap without record"))?;
                if pending.radiotap.is_some() || fragment.len() < 8 || fragment[0] != 0 ||
                    le16(&fragment[2..4]) as usize != fragment.len() || fragment.len() + 2 > pending.captured_len {
                    return Err(invalid("invalid radiotap fragment"));
                }
                pending.radiotap = Some(fragment.to_vec());
                Ok(None)
            }
            FRAME_BYTES => {
                let pending = self.pending.remove(&thread).ok_or_else(|| invalid("frame without record"))?;
                let mut frame = pending.radiotap.ok_or_else(|| invalid("frame without radiotap"))?;
                if frame.len().checked_add(fragment.len()) != Some(pending.captured_len) {
                    return Err(invalid("frame size does not match record"));
                }
                frame.extend_from_slice(fragment);
                Ok(Some(frame))
            }
            _ => unreachable!(),
        }
    }

    /// Check for incomplete fragments when an ETW session ends.
    pub fn finish(&self) -> io::Result<()> {
        if self.failed { return Err(invalid("assembler previously failed")); }
        if !self.header_seen || !self.pending.is_empty() { return Err(invalid("incomplete capture")); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrapped(bytes: &[u8]) -> Vec<u8> {
        let mut result = (bytes.len() as u16).to_le_bytes().to_vec();
        result.extend_from_slice(bytes);
        result
    }
    fn pcap() -> Vec<u8> {
        let mut bytes = vec![0; 24];
        bytes[0..4].copy_from_slice(&0xa1b2c3d4u32.to_le_bytes());
        bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&4u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&127u32.to_le_bytes());
        bytes
    }
    fn record(len: u32) -> Vec<u8> {
        let mut bytes = vec![0; 16];
        bytes[8..12].copy_from_slice(&len.to_le_bytes());
        bytes[12..16].copy_from_slice(&len.to_le_bytes());
        bytes
    }
    #[test]
    fn assembles_interleaved_threads_and_ignores_unrelated_events() {
        let mut a = FrameAssembler::new();
        let rt = [0,0,8,0,0,0,0,0];
        a.ingest(PCAP_HEADER, 0, &wrapped(&pcap())).unwrap();
        a.ingest(RECORD_HEADER, 3, &wrapped(&record(10))).unwrap();
        a.ingest(RECORD_HEADER, 4, &wrapped(&record(11))).unwrap();
        a.ingest(0xa7, 3, &wrapped(&rt)).unwrap();
        a.ingest(0xa8, 4, &wrapped(&rt)).unwrap();
        assert!(a.ingest(0x80, 3, &[]).unwrap().is_none());
        assert_eq!(a.ingest(FRAME_BYTES, 4, &wrapped(&[1,2,3])).unwrap().unwrap(), [&rt[..], &[1,2,3]].concat());
        assert_eq!(a.ingest(FRAME_BYTES, 3, &wrapped(&[4,5])).unwrap().unwrap(), [&rt[..], &[4,5]].concat());
        a.finish().unwrap();
    }
    #[test]
    fn rejects_damaged_or_cross_thread_frames_and_fails_closed() {
        let mut a = FrameAssembler::new();
        a.ingest(PCAP_HEADER, 0, &wrapped(&pcap())).unwrap();
        a.ingest(RECORD_HEADER, 3, &wrapped(&record(10))).unwrap();
        assert!(a.ingest(0xa7, 4, &wrapped(&[0,0,8,0,0,0,0,0])).is_err());
        assert!(a.ingest(0xa7, 3, &wrapped(&[0,0,8,0,0,0,0,0])).is_err());
        assert!(a.finish().is_err());
        let mut b = FrameAssembler::new();
        assert!(b.ingest(PCAP_HEADER, 0, &[24,0,1]).is_err());
    }
    #[test]
    fn rejects_incomplete_and_oversized_records() {
        let mut a = FrameAssembler::new();
        a.ingest(PCAP_HEADER, 0, &wrapped(&pcap())).unwrap();
        a.ingest(RECORD_HEADER, 3, &wrapped(&record(10))).unwrap();
        assert!(a.finish().is_err());
        let mut b = FrameAssembler::new();
        b.ingest(PCAP_HEADER, 0, &wrapped(&pcap())).unwrap();
        assert!(b.ingest(RECORD_HEADER, 3, &wrapped(&record(MAX_PAYLOAD as u32 + 1))).is_err());
    }
}
