// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020-2021 MediaTek Inc.
// Connac2 commands and response layout follow mt76_connac_mcu.c and
// mt7921/mcu.c at 41d5d1dd63111870ef74a5b9645416d98979a145.
// See LICENSE-mt7922-rx.txt.

//! Transport-independent Connac2 firmware MCU command implementation.
//!
//! There is currently no device-backed `McuIo` implementation. A driver must
//! provide queue allocation, DMA synchronization, matching and timeout of MCU
//! events, recovery and teardown before these commands can touch hardware.
use super::{download::{FirmwareTransport, PatchSemaphore}, download_plan::InitCommand, mcu};
use std::io;

const TARGET_ADDRESS_LEN: u32 = 0x01;
const FIRMWARE_START: u32 = 0x02;
const NIC_POWER_CTRL: u32 = 0x04;
const PATCH_START: u32 = 0x05;
const PATCH_FINISH: u32 = 0x07;
const PATCH_SEMAPHORE: u32 = 0x10;
const RX_SEQUENCE_OFFSET: usize = 29;
const RX_HEADER_SIZE: usize = 36;
const PATCH_STATUS_OFFSET: usize = RX_HEADER_SIZE - 4;
const EFUSE_ACCESS_QUERY: u32 = 0x01ed | mcu::QUERY;
const EFUSE_BUFFER_MODE: u32 = 0x21ed;
const EFUSE_SIZE: u32 = 3584;
const EFUSE_BLOCK: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Queue { Command, FirmwareDownload }

/// The device implementation must serialize all exchanges and match response
/// sequences without losing unsolicited events. `expected_sequence=None`
/// submits without waiting. `Some` must return a matching MCU event or error
/// after a bounded timeout; completion must be bounded to actual DMA bytes.
pub trait McuIo {
    fn exchange(&mut self, queue: Queue, bytes: &[u8], expected_sequence: Option<u8>) -> io::Result<Option<Vec<u8>>>;
    fn drain_download(&mut self) -> io::Result<()>;
    fn quiesce_download(&mut self) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareState { PowerOn, Running }

/// A platform transport must read the device's firmware state with a bounded
/// deadline. This interface cannot start the radio or prove it is usable.
pub trait FirmwareStateIo {
    fn wait_firmware_state(&mut self, state: FirmwareState) -> io::Result<()>;
}

pub struct FirmwareMcu<T> { io: T, sequence: u8, poisoned: bool }
impl<T: McuIo> FirmwareMcu<T> {
    pub fn new(io: T) -> Self { Self { io, sequence: 0, poisoned: false } }
    pub fn into_inner(self) -> T { self.io }
    /// Once an exchange fails, a later response may belong to the failed
    /// command even after the four-bit sequence wraps. A fresh transport and
    /// verified hardware recovery are required; this flag is never cleared.
    pub fn is_poisoned(&self) -> bool { self.poisoned }
    /// Ask the MCU to restart into the download path; no response is expected.
    pub fn restart(&mut self) -> io::Result<()> {
        self.send(NIC_POWER_CTRL, &[1, 0, 0, 0], false).map(|_| ())
    }

    /// Configure the running firmware to use factory eFuse calibration data.
    /// The caller must have already booted the appropriate firmware and must
    /// not infer successful RF calibration from this command alone.
    pub fn use_factory_efuse(&mut self) -> io::Result<()> {
        self.ack(EFUSE_BUFFER_MODE, &[0, 1, 0, 0])
    }

    /// Read one aligned block of factory eFuse through the firmware. No eFuse
    /// write is performed. Device-backed transports are not implemented yet.
    pub fn read_efuse_block(&mut self, offset: u32) -> io::Result<[u8; 16]> {
        if offset % EFUSE_BLOCK != 0 || offset >= EFUSE_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid eFuse block address"));
        }
        let mut request = [0u8; 24];
        request[..4].copy_from_slice(&offset.to_le_bytes());
        let reply = self.send(EFUSE_ACCESS_QUERY, &request, true)?.unwrap();
        let payload = match reply.get(RX_HEADER_SIZE..RX_HEADER_SIZE + request.len()) {
            Some(payload) => payload,
            None => { self.poisoned = true; return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated eFuse response")); }
        };
        let returned_offset = u32::from_le_bytes(payload[..4].try_into().unwrap());
        if returned_offset != offset {
            self.poisoned = true;
            return Err(io::Error::new(io::ErrorKind::InvalidData, "eFuse response address mismatch"));
        }
        if u32::from_le_bytes(payload[4..8].try_into().unwrap()) == 0 {
            return Err(io::Error::new(io::ErrorKind::NotFound, "factory eFuse block not present"));
        }
        Ok(payload[8..24].try_into().unwrap())
    }

    fn send(&mut self, command: u32, payload: &[u8], wait: bool) -> io::Result<Option<Vec<u8>>> {
        if self.poisoned { return Err(io::Error::other("MCU transport requires device recovery")); }
        let result = self.send_once(command, payload, wait);
        if result.is_err() { self.poisoned = true; }
        result
    }
    fn send_once(&mut self, command: u32, payload: &[u8], wait: bool) -> io::Result<Option<Vec<u8>>> {
        let (sequence, bytes) = mcu::encode(command, &mut self.sequence, payload)?;
        let queue = if command == mcu::FW_SCATTER { Queue::FirmwareDownload } else { Queue::Command };
        let reply = self.io.exchange(queue, &bytes, if wait { Some(sequence) } else { None })?;
        if wait {
            let event = reply.as_ref().ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing MCU response"))?;
            if event.len() < PATCH_STATUS_OFFSET + 1 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated MCU response"));
            }
            let rxd0 = u32::from_le_bytes(event[..4].try_into().unwrap());
            // `mt7921_queue_rx_skb` treats flag 1 as a normal MCU data frame,
            // not an event. Unsolicited events may also carry a sequence.
            if (rxd0 >> 27) != 7 || ((rxd0 >> 16) & 0xf) == 1 || event[30] & 4 != 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "not a solicited MCU response"));
            }
            let eid = event[28];
            let unsolicited = matches!(eid, 0x07 | 0x0d | 0x0f | 0x13 | 0x23 | 0x27 | 0x96 | 0xf0)
                || (eid != 0x06 && event[32] == 0x87);
            if unsolicited {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "unsolicited MCU event"));
            }
            if event.get(RX_SEQUENCE_OFFSET) != Some(&sequence) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "MCU response sequence mismatch"));
            }
        } else if reply.is_some() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected firmware download response"));
        }
        Ok(reply)
    }
    pub(crate) fn ack(&mut self, command: u32, payload: &[u8]) -> io::Result<()> {
        let reply = self.send(command, payload, true)?.unwrap();
        if reply.len() < RX_HEADER_SIZE {
            self.poisoned = true;
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated MCU reply"));
        }
        Ok(())
    }
    fn patch_status(&mut self, command: u32, payload: &[u8]) -> io::Result<u8> {
        let reply = self.send(command, payload, true)?.unwrap();
        reply.get(PATCH_STATUS_OFFSET).copied().ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "truncated patch status"))
    }
}
impl<T: McuIo + FirmwareStateIo> FirmwareMcu<T> {
    pub fn wait_state(&mut self, state: FirmwareState) -> io::Result<()> {
        if self.poisoned { return Err(io::Error::other("MCU transport requires device recovery")); }
        let result = self.io.wait_firmware_state(state);
        if result.is_err() { self.poisoned = true; }
        result
    }
}
impl<T: McuIo> FirmwareTransport for FirmwareMcu<T> {
    type Error = io::Error;
    fn acquire_patch(&mut self) -> io::Result<PatchSemaphore> {
        match self.patch_status(PATCH_SEMAPHORE, &1u32.to_le_bytes())? {
            1 => Ok(PatchSemaphore::AlreadyLoaded),
            2 => Ok(PatchSemaphore::Acquired),
            _ => Err(io::Error::other("patch semaphore not acquired")),
        }
    }
    fn init(&mut self, command: &InitCommand, address: u32, length: u32, mode: u32) -> io::Result<()> {
        let mut payload = [0; 12];
        payload[0..4].copy_from_slice(&address.to_le_bytes());
        payload[4..8].copy_from_slice(&length.to_le_bytes());
        payload[8..12].copy_from_slice(&mode.to_le_bytes());
        self.ack(match command { InitCommand::PatchStart => PATCH_START, InitCommand::TargetAddressLength => TARGET_ADDRESS_LEN }, &payload)
    }
    fn scatter(&mut self, bytes: &[u8]) -> io::Result<()> { self.send(mcu::FW_SCATTER, bytes, false).map(|_| ()) }
    fn drain_download(&mut self) -> io::Result<()> {
        if self.poisoned { return Err(io::Error::other("MCU transport requires device recovery")); }
        let result = self.io.drain_download();
        if result.is_err() { self.poisoned = true; }
        result
    }
    // Even a poisoned transport must allow its owner to try fencing DMA. This
    // does not clear the poison or establish that a device reset succeeded.
    fn quiesce_download(&mut self) -> io::Result<()> {
        let result = self.io.quiesce_download();
        if result.is_err() { self.poisoned = true; }
        result
    }
    fn finish_patch(&mut self) -> io::Result<()> {
        match self.patch_status(PATCH_FINISH, &[0; 4])? { 0 => Ok(()), _ => Err(io::Error::other("firmware patch rejected")) }
    }
    fn release_patch(&mut self) -> io::Result<()> {
        match self.patch_status(PATCH_SEMAPHORE, &0u32.to_le_bytes())? { 3 => Ok(()), _ => Err(io::Error::other("patch semaphore not released")) }
    }
    fn start_ram(&mut self, address: u32, override_address: bool) -> io::Result<()> {
        let mut payload = [0; 8];
        payload[..4].copy_from_slice(&u32::from(override_address).to_le_bytes());
        payload[4..].copy_from_slice(&address.to_le_bytes());
        self.ack(FIRMWARE_START, &payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    #[derive(Default)]
    struct FakeIo { replies: VecDeque<u8>, requests: Vec<(Queue, Vec<u8>, Option<u8>)>, drains: usize, bad_sequence: bool, unsolicited: bool, short_reply: bool }
    impl McuIo for FakeIo {
        fn exchange(&mut self, queue: Queue, bytes: &[u8], seq: Option<u8>) -> io::Result<Option<Vec<u8>>> {
            self.requests.push((queue, bytes.to_vec(), seq));
            Ok(seq.map(|seq| {
                let mut event = vec![0u8; 36];
                event[..4].copy_from_slice(&(7u32 << 27).to_le_bytes());
                event[29] = if self.bad_sequence { 0 } else { seq };
                if self.unsolicited { event[30] = 4; }
                event[32] = self.replies.pop_front().unwrap_or(0);
                if self.short_reply { event.truncate(33); }
                event
            }))
        }
        fn drain_download(&mut self) -> io::Result<()> { self.drains += 1; Ok(()) }
        fn quiesce_download(&mut self) -> io::Result<()> { Ok(()) }
    }
    #[test]
    fn firmware_commands_use_connac2_headers_and_separate_queues() {
        let io = FakeIo { replies: VecDeque::from([2, 0, 3, 0]), ..Default::default() };
        let mut mcu = FirmwareMcu::new(io);
        assert_eq!(mcu.acquire_patch().unwrap(), PatchSemaphore::Acquired);
        mcu.init(&InitCommand::PatchStart, 0x200000, 3, 0x80000000).unwrap();
        mcu.scatter(&[1,2,3]).unwrap();
        mcu.drain_download().unwrap();
        mcu.release_patch().unwrap();
        mcu.start_ram(0x1234, true).unwrap();
        let io = mcu.into_inner();
        assert_eq!(io.requests[0].1[36], 0x10);
        assert_eq!(&io.requests[0].1[64..], &[1,0,0,0]);
        assert_eq!(io.requests[1].1[36], 0x05);
        assert_eq!(&io.requests[1].1[64..], &[0,0,0x20,0,3,0,0,0,0,0,0,0x80]);
        assert_eq!(io.requests[2], (Queue::FirmwareDownload, vec![1,2,3], None));
        assert_eq!(io.drains, 1);
        assert_eq!(io.requests[4].1[36], 0x02);
        assert_eq!(&io.requests[4].1[64..], &[1,0,0,0,0x34,0x12,0,0]);
    }
    #[test]
    fn rejected_semaphore_and_wrong_sequence_do_not_succeed() {
        let mut mcu = FirmwareMcu::new(FakeIo::default());
        assert!(mcu.acquire_patch().is_err());
        let mut mcu = FirmwareMcu::new(FakeIo { bad_sequence: true, ..Default::default() });
        assert!(mcu.acquire_patch().is_err());
        assert!(mcu.is_poisoned());
        assert!(mcu.release_patch().is_err());
        assert_eq!(mcu.into_inner().requests.len(), 1);
        let mut mcu = FirmwareMcu::new(FakeIo { unsolicited: true, replies: VecDeque::from([2]), ..Default::default() });
        assert!(mcu.acquire_patch().is_err());
        let mut mcu = FirmwareMcu::new(FakeIo { short_reply: true, ..Default::default() });
        assert!(mcu.start_ram(0, false).is_err());
        assert!(mcu.is_poisoned());
    }
    #[test]
    fn efuse_query_checks_alignment_response_and_identity() {
        struct Efuse { response_offset: u32, valid: u32, sent: Vec<Vec<u8>>, short: bool }
        impl McuIo for Efuse {
            fn exchange(&mut self, _: Queue, bytes: &[u8], seq: Option<u8>) -> io::Result<Option<Vec<u8>>> {
                self.sent.push(bytes.to_vec());
                let mut reply = vec![0u8; 60];
                reply[..4].copy_from_slice(&(7u32 << 27).to_le_bytes()); reply[29] = seq.unwrap();
                reply[36..40].copy_from_slice(&self.response_offset.to_le_bytes());
                reply[40..44].copy_from_slice(&self.valid.to_le_bytes());
                for (i, byte) in reply[44..60].iter_mut().enumerate() { *byte = i as u8; }
                if self.short { reply.truncate(50); }
                Ok(Some(reply))
            }
            fn drain_download(&mut self) -> io::Result<()> { Ok(()) }
            fn quiesce_download(&mut self) -> io::Result<()> { Ok(()) }
        }
        let fake = Efuse { response_offset: 16, valid: 1, sent: vec![], short: false };
        let mut mcu = FirmwareMcu::new(fake);
        assert!(mcu.read_efuse_block(1).is_err());
        assert_eq!(mcu.read_efuse_block(16).unwrap(), [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]);
        let fake = mcu.into_inner();
        assert_eq!(fake.sent.len(), 1);
        assert_eq!(fake.sent[0][36], 0xed); assert_eq!(fake.sent[0][41], 1);
        assert_eq!(&fake.sent[0][64..68], &16u32.to_le_bytes());
        let fake = Efuse { response_offset: 32, valid: 1, sent: vec![], short: false };
        let mut mcu = FirmwareMcu::new(fake);
        assert!(mcu.read_efuse_block(16).is_err()); assert!(mcu.is_poisoned());
        let fake = Efuse { response_offset: 16, valid: 0, sent: vec![], short: false };
        let mut mcu = FirmwareMcu::new(fake);
        assert_eq!(mcu.read_efuse_block(16).unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}
