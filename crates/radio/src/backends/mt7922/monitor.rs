// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020 MediaTek Inc.
// Connac2 sniffer messages follow mt7921/mcu.c at
// 41d5d1dd63111870ef74a5b9645416d98979a145; see LICENSE-mt7922-rx.txt.

//! MT7922 firmware monitor command sequence, over an abstract MCU transport.
//! This does not initialize the device, set its regulatory domain, configure
//! a BSS, establish an RX filter, or provide hardware-backed queues.
use super::{firmware_mcu::{FirmwareMcu, McuIo}, mcu};
use std::io;

const SNIFFER: u32 = mcu::UNIFIED | 0x24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band { Ghz2, Ghz5 }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width { Mhz20, Mhz40, Mhz80 }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel { pub band: Band, pub primary: u8, pub center: u8, pub width: Width }

impl Channel {
    /// Validate only packet geometry, not local regulatory permission. The
    /// hardware backend must independently check all channel and power rules.
    fn payload(self) -> io::Result<[u8; 20]> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid monitor channel geometry");
        let in_band = |v| match self.band { Band::Ghz2 => (1..=14).contains(&v), Band::Ghz5 => (32..=177).contains(&v) };
        let delta = self.primary.abs_diff(self.center);
        if !in_band(self.primary) || !in_band(self.center) ||
            !match self.width { Width::Mhz20 => delta == 0, Width::Mhz40 => delta == 2,
                Width::Mhz80 => self.band == Band::Ghz5 && matches!(delta, 2 | 6) } { return Err(invalid()); }
        let mut data = [0u8; 20];
        data[4..6].copy_from_slice(&1u16.to_le_bytes()); // config TLV
        data[6..8].copy_from_slice(&16u16.to_le_bytes());
        data[10] = match self.band { Band::Ghz2 => 1, Band::Ghz5 => 2 };
        data[11] = u8::from(self.width == Width::Mhz80);
        data[12] = self.primary;
        data[13] = if self.primary < self.center { 1 } else if self.primary > self.center { 3 } else { 0 };
        data[14] = self.center;
        data[16] = 1; // discard FCS-error frames, as required by current parser
        Ok(data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State { Stopped, Running(Channel), RecoveryRequired }

/// Serializes monitor setup over an already initialized MCU. The owner must
/// independently prove channel permission, power limits, RX filter, BSS and
/// device recovery before enabling this on real hardware. State `Running`
/// means replies arrived, not that the radio is functional or receives AWDL.
pub struct Monitor<T> { mcu: FirmwareMcu<T>, state: State }
impl<T: McuIo> Monitor<T> {
    pub fn new(mcu: FirmwareMcu<T>) -> Self { Self { mcu, state: State::Stopped } }
    pub fn state(&self) -> State { self.state }
    pub fn into_inner(self) -> FirmwareMcu<T> { self.mcu }
    pub fn start(&mut self, channel: Channel) -> io::Result<()> {
        if self.state != State::Stopped { return Err(io::Error::other("monitor not stopped")); }
        let config = channel.payload()?; // Validate before any firmware write.
        let enable = [0,0,0,0, 0,0,8,0, 1,0,0,0];
        self.state = State::RecoveryRequired; // A failed send may have landed.
        self.mcu.ack(SNIFFER, &enable)?;
        self.mcu.ack(SNIFFER, &config)?;
        self.state = State::Running(channel);
        Ok(())
    }
    pub fn set_channel(&mut self, channel: Channel) -> io::Result<()> {
        if !matches!(self.state, State::Running(_)) { return Err(io::Error::other("monitor is not running")); }
        let config = channel.payload()?;
        self.state = State::RecoveryRequired;
        self.mcu.ack(SNIFFER, &config)?;
        self.state = State::Running(channel);
        Ok(())
    }
    pub fn stop(&mut self) -> io::Result<()> {
        if self.state == State::Stopped { return Ok(()); }
        if !matches!(self.state, State::Running(_)) { return Err(io::Error::other("monitor needs device recovery")); }
        self.state = State::RecoveryRequired;
        self.mcu.ack(SNIFFER, &[0,0,0,0, 0,0,8,0, 0,0,0,0])?;
        self.state = State::Stopped;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::mt7922::firmware_mcu::Queue;
    #[derive(Default)] struct Fake { sent: Vec<Vec<u8>>, fail_at: Option<usize> }
    impl McuIo for Fake {
        fn exchange(&mut self, _: Queue, bytes: &[u8], seq: Option<u8>) -> io::Result<Option<Vec<u8>>> {
            self.sent.push(bytes.to_vec());
            if self.fail_at == Some(self.sent.len()) { return Err(io::Error::other("injected failure")); }
            let mut response = vec![0u8; 36];
            response[..4].copy_from_slice(&(7u32 << 27).to_le_bytes());
            response[29] = seq.unwrap();
            Ok(Some(response))
        }
        fn drain_download(&mut self) -> io::Result<()> { Ok(()) }
        fn quiesce_download(&mut self) -> io::Result<()> { Ok(()) }
    }
    fn ch() -> Channel { Channel { band: Band::Ghz5, primary: 44, center: 42, width: Width::Mhz80 } }
    #[test] fn enables_configures_and_stops_in_order() {
        let mut monitor = Monitor::new(FirmwareMcu::new(Fake::default()));
        monitor.start(ch()).unwrap(); monitor.stop().unwrap();
        let fake = monitor.into_inner().into_inner();
        assert_eq!(fake.sent.len(), 3);
        assert_eq!(&fake.sent[0][34..36], &[0x24, 0]);
        assert_eq!(&fake.sent[0][48..], &[0,0,0,0,0,0,8,0,1,0,0,0]);
        assert_eq!(&fake.sent[1][48..], &[0,0,0,0,1,0,16,0,0,0,2,1,44,3,42,0,1,0,0,0]);
        assert_eq!(&fake.sent[2][48..], &[0,0,0,0,0,0,8,0,0,0,0,0]);
    }
    #[test] fn invalid_geometry_writes_nothing_and_partial_failure_requires_recovery() {
        let mut monitor = Monitor::new(FirmwareMcu::new(Fake { fail_at: Some(2), ..Default::default() }));
        assert!(monitor.start(Channel { center: 44, ..ch() }).is_err());
        assert_eq!(monitor.state(), State::Stopped);
        assert!(monitor.start(ch()).is_err());
        assert_eq!(monitor.state(), State::RecoveryRequired);
        assert!(monitor.stop().is_err());
        assert_eq!(monitor.into_inner().into_inner().sent.len(), 2);
    }
}
