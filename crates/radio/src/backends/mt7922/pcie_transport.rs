// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020, 2023 MediaTek Inc.
// Connac2 event routing and queue selection are based on mt76/mt7921/mcu.c,
// mt7921/mac.c and the MT7925 PCIe transport. See LICENSE-mt7922-rx.txt and
// LICENSE-mt7925.txt. The actual Windows device queues do not yet exist.

//! Serialized, bounded firmware transport over a platform-provided queue set.
//! This module is not a driver and cannot create, start, or stop DMA.

use super::firmware_mcu::{FirmwareState, FirmwareStateIo, McuIo, Queue};
use std::{collections::VecDeque, io, time::{Duration, Instant}};

const EVENT_LIMIT: usize = 32;
const MAX_EVENT_BYTES: usize = 16_384;
const WAIT_SLICE: Duration = Duration::from_millis(20);

/// Platform implementation owns DMA memory, ordering, device access, and
/// recovery. All calls must be serialized and bounded. `wait` must block for
/// no longer than the supplied duration; it may return early for any event.
/// Dropping a platform implementation must not free buffers still accessible
/// by the device; it must first reach its OS/device-specific shutdown fence.
pub trait DeviceMcuQueues {
    /// Success means the packet was published to the correct device queue;
    /// a command-queue success also requires its DMA buffer to be reclaimed.
    /// On failure, the device may still have observed it.
    fn submit(&mut self, queue: Queue, bytes: &[u8]) -> io::Result<()>;
    /// Return only a complete DMA-bounded event. No event is represented by None.
    fn poll_event(&mut self) -> io::Result<Option<Vec<u8>>>;
    /// True only when every published firmware-download packet has completed.
    fn download_idle(&mut self) -> io::Result<bool>;
    fn wait(&mut self, maximum: Duration) -> io::Result<()>;
    /// Fence all pending download DMA, leaving the command queue usable.
    /// Failure requires device recovery and retaining any still-owned buffers.
    fn quiesce_download(&mut self) -> io::Result<()>;
    /// Read MT_CONN_ON_MISC from the owned device. Unsupported by default.
    fn firmware_state(&mut self) -> io::Result<u32> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "firmware state unavailable"))
    }
}

impl<T: DeviceMcuQueues> FirmwareStateIo for PcieTransport<T> {
    fn wait_firmware_state(&mut self, state: FirmwareState) -> io::Result<()> {
        if self.poisoned { return Err(io::Error::other("MCU transport requires device recovery")); }
        let result = (|| {
            let started = Instant::now();
            // mt792x_load_firmware tests the entire three-bit state for the
            // download-ready value, not just the power bit. Accepting 3 here
            // could send firmware while the old image is still running.
            let (mask, expected) = match state {
                FirmwareState::PowerOn => (7, 1),
                FirmwareState::Running => (3, 3),
            };
            loop {
                let wait = self.remaining(started)?;
                let value = self.queues.firmware_state()?;
                if value == u32::MAX { return Err(io::Error::new(io::ErrorKind::InvalidData, "firmware state unavailable")); }
                if let Some(event) = self.queues.poll_event()? {
                    let solicited = Self::event_kind(&event)?.is_some();
                    self.retain_event(event)?;
                    if solicited { return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected response while waiting for firmware")); }
                } else if value & mask == expected {
                    return Ok(());
                } else { self.queues.wait(wait)?; }
            }
        })();
        if result.is_err() { self.poisoned = true; }
        result
    }
}

pub struct PcieTransport<T> {
    queues: T,
    events: VecDeque<Vec<u8>>,
    timeout: Duration,
    poisoned: bool,
}
impl<T: DeviceMcuQueues> PcieTransport<T> {
    pub fn new(queues: T, timeout: Duration) -> io::Result<Self> {
        if timeout.is_zero() || timeout > Duration::from_secs(30) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid MCU timeout"));
        }
        Ok(Self { queues, events: VecDeque::new(), timeout, poisoned: false })
    }
    /// Returns events that were not replies to the in-flight command. The
    /// caller must route them; the transport will fail closed if this fills.
    pub fn next_unsolicited(&mut self) -> Option<Vec<u8>> { self.events.pop_front() }
    pub fn is_poisoned(&self) -> bool { self.poisoned }
    pub fn into_inner(self) -> T { self.queues }

    fn retain_event(&mut self, event: Vec<u8>) -> io::Result<()> {
        if event.len() > MAX_EVENT_BYTES || self.events.len() >= EVENT_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MCU event backlog exceeded"));
        }
        self.events.push_back(event);
        Ok(())
    }
    fn event_kind(event: &[u8]) -> io::Result<Option<u8>> {
        if event.len() < 4 || event.len() > MAX_EVENT_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid MCU event length"));
        }
        let rxd0 = u32::from_le_bytes(event[..4].try_into().unwrap());
        if rxd0 >> 27 != 7 || (rxd0 >> 16) & 0xf == 1 { return Ok(None); }
        if event.len() < 33 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated MCU event"));
        }
        let (eid, sequence, option, ext) = (event[28], event[29], event[30], event[32]);
        if sequence == 0 || option & 4 != 0 || matches!(eid, 0x07 | 0x0d | 0x0f | 0x13 | 0x23 | 0x27 | 0x96 | 0xf0)
            || (eid != 0x06 && ext == 0x87) { return Ok(None); }
        Ok(Some(sequence))
    }
    fn remaining(&self, started: Instant) -> io::Result<Duration> {
        self.timeout.checked_sub(started.elapsed()).filter(|left| !left.is_zero())
            .map(|left| left.min(WAIT_SLICE))
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "MCU queue timed out"))
    }
}

impl<T: DeviceMcuQueues> McuIo for PcieTransport<T> {
    fn exchange(&mut self, queue: Queue, bytes: &[u8], expected: Option<u8>) -> io::Result<Option<Vec<u8>>> {
        if self.poisoned { return Err(io::Error::other("MCU transport requires device recovery")); }
        let result = (|| {
        if bytes.is_empty() || bytes.len() > MAX_EVENT_BYTES ||
            (queue == Queue::FirmwareDownload && expected.is_some()) ||
            expected.is_some_and(|seq| seq == 0 || seq > 15) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid MCU submission"));
        }
        let started = Instant::now();
        if expected.is_some() {
            // Do not let a queued response from an earlier transaction match
            // the new sequence, especially after the four-bit counter wraps.
            loop {
                self.remaining(started)?;
                let Some(event) = self.queues.poll_event()? else { break };
                let solicited = Self::event_kind(&event)?.is_some();
                self.retain_event(event)?;
                if solicited {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "stale MCU response before submission"));
                }
            }
        }
        self.queues.submit(queue, bytes)?;
        let Some(expected) = expected else { return Ok(None) };
        loop {
            // Check the deadline even during an event flood.
            let wait = self.remaining(started)?;
            match self.queues.poll_event()? {
                Some(event) => match Self::event_kind(&event)? {
                    None => self.retain_event(event)?,
                    Some(sequence) if sequence == expected => return Ok(Some(event)),
                    Some(_) => {
                        self.retain_event(event)?;
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected MCU response sequence"));
                    }
                },
                None => self.queues.wait(wait)?,
            }
        }
        })();
        if result.is_err() { self.poisoned = true; }
        result
    }
    fn drain_download(&mut self) -> io::Result<()> {
        if self.poisoned { return Err(io::Error::other("MCU transport requires device recovery")); }
        let result = (|| {
        let started = Instant::now();
        loop {
            let wait = self.remaining(started)?;
            if self.queues.download_idle()? { return Ok(()); }
            if let Some(event) = self.queues.poll_event()? {
                let solicited = Self::event_kind(&event)?.is_some();
                self.retain_event(event)?;
                if solicited {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected MCU response while draining"));
                }
            } else { self.queues.wait(wait)?; }
        }
        })();
        if result.is_err() { self.poisoned = true; }
        result
    }
    fn quiesce_download(&mut self) -> io::Result<()> {
        let result = self.queues.quiesce_download();
        if result.is_err() { self.poisoned = true; }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake { submitted: Vec<(Queue, Vec<u8>)>, preexisting: VecDeque<Vec<u8>>, events: VecDeque<Vec<u8>>, waits: usize, idle: bool, fenced: bool, state: u32 }
    impl DeviceMcuQueues for Fake {
        fn submit(&mut self, q: Queue, bytes: &[u8]) -> io::Result<()> { self.submitted.push((q, bytes.to_vec())); Ok(()) }
        fn poll_event(&mut self) -> io::Result<Option<Vec<u8>>> {
            if let Some(event) = self.preexisting.pop_front() { return Ok(Some(event)); }
            if self.submitted.is_empty() { Ok(None) } else { Ok(self.events.pop_front()) }
        }
        fn download_idle(&mut self) -> io::Result<bool> { Ok(self.idle) }
        fn wait(&mut self, max: Duration) -> io::Result<()> { self.waits += 1; std::thread::sleep(max); Ok(()) }
        fn quiesce_download(&mut self) -> io::Result<()> { self.fenced = true; Ok(()) }
        fn firmware_state(&mut self) -> io::Result<u32> { Ok(self.state) }
    }
    fn event(seq: u8, unsolicited: bool) -> Vec<u8> {
        let mut event = vec![0u8; 36];
        event[..4].copy_from_slice(&(7u32 << 27).to_le_bytes());
        event[29] = seq;
        if unsolicited { event[30] = 4; }
        event
    }
    #[test] fn routes_unsolicited_events_and_matches_reply() {
        let fake = Fake { events: VecDeque::from([event(2, true), event(3, false)]), ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_millis(100)).unwrap();
        assert_eq!(transport.exchange(Queue::Command, &[42], Some(3)).unwrap(), Some(event(3, false)));
        assert_eq!(transport.next_unsolicited(), Some(event(2, true)));
        assert_eq!(transport.into_inner().submitted, [(Queue::Command, vec![42])]);
    }
    #[test] fn wrong_sequence_is_preserved_and_fails_closed() {
        let fake = Fake { events: VecDeque::from([event(4, false)]), ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_millis(100)).unwrap();
        assert_eq!(transport.exchange(Queue::Command, &[1], Some(3)).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(transport.next_unsolicited(), Some(event(4, false)));
        assert!(transport.is_poisoned());
        assert!(transport.exchange(Queue::Command, &[2], Some(4)).is_err());
        assert_eq!(transport.into_inner().submitted.len(), 1);
    }
    #[test] fn timeout_waits_and_download_fence_is_explicit() {
        let mut transport = PcieTransport::new(Fake::default(), Duration::from_millis(5)).unwrap();
        assert_eq!(transport.exchange(Queue::Command, &[1], Some(1)).unwrap_err().kind(), io::ErrorKind::TimedOut);
        transport.quiesce_download().unwrap();
        let fake = transport.into_inner();
        assert!(fake.waits >= 1 && fake.fenced);
        assert!(PcieTransport::new(Fake::default(), Duration::ZERO).is_err());
    }
    #[test] fn bounds_unsolicited_backlog_and_download_submission() {
        let fake = Fake { events: (0..=EVENT_LIMIT).map(|_| event(1, true)).collect(), ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_secs(1)).unwrap();
        assert!(transport.exchange(Queue::Command, &[1], Some(1)).is_err());
        assert!(transport.exchange(Queue::FirmwareDownload, &[1], Some(1)).is_err());
    }
    #[test] fn stale_response_during_download_drain_requires_recovery() {
        let fake = Fake { preexisting: VecDeque::from([event(3, false)]), ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_millis(100)).unwrap();
        assert_eq!(transport.drain_download().unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(transport.is_poisoned());
        assert_eq!(transport.next_unsolicited(), Some(event(3, false)));
        assert!(transport.exchange(Queue::Command, &[1], Some(3)).is_err());
        assert!(transport.into_inner().submitted.is_empty());
    }
    #[test] fn queued_stale_response_is_not_reused_after_sequence_wrap() {
        let fake = Fake { preexisting: VecDeque::from([event(1, false)]), ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_millis(100)).unwrap();
        assert_eq!(transport.exchange(Queue::Command, &[1], Some(1)).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(transport.is_poisoned());
        assert!(transport.into_inner().submitted.is_empty());
    }
    #[test] fn ready_state_does_not_hide_a_queued_stale_response() {
        let fake = Fake { state: 3, preexisting: VecDeque::from([event(3, false)]), ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_millis(100)).unwrap();
        assert_eq!(transport.wait_firmware_state(FirmwareState::Running).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(transport.is_poisoned());
        assert_eq!(transport.next_unsolicited(), Some(event(3, false)));
    }
    #[test] fn ready_state_retains_queued_unsolicited_events() {
        let fake = Fake { state: 3, preexisting: VecDeque::from([event(2, true)]), ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_millis(100)).unwrap();
        transport.wait_firmware_state(FirmwareState::Running).unwrap();
        assert_eq!(transport.next_unsolicited(), Some(event(2, true)));
    }
    #[test] fn running_firmware_is_not_download_ready() {
        let fake = Fake { state: 3, ..Default::default() };
        let mut transport = PcieTransport::new(fake, Duration::from_millis(5)).unwrap();
        assert_eq!(transport.wait_firmware_state(FirmwareState::PowerOn).unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(transport.is_poisoned());
    }
}
