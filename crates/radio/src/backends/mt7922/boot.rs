// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020, 2023 MediaTek Inc.
// Boot ordering follows mt76/mt792x_core.c. See LICENSE-mt7922-rx.txt.

//! Firmware boot orchestration for an already owned, initialized device.
//! No hardware queue implementation exists yet; tests use a simulated MCU.
use super::{download::{self, DownloadError}, firmware::FirmwareBundle,
    firmware_mcu::{FirmwareMcu, FirmwareState, FirmwareStateIo, McuIo}};
use std::{fmt, io};

#[derive(Debug)]
pub enum BootError {
    Restart(io::Error),
    WaitPower(io::Error),
    Patch(DownloadError<io::Error>),
    Ram(DownloadError<io::Error>),
    WaitRunning(io::Error),
}
impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restart(e) => write!(f, "MCU restart: {e}"),
            Self::WaitPower(e) => write!(f, "MCU power-on state: {e}"),
            Self::Patch(e) => write!(f, "firmware patch: {e}"),
            Self::Ram(e) => write!(f, "RAM firmware: {e}"),
            Self::WaitRunning(e) => write!(f, "firmware ready state: {e}"),
        }
    }
}
impl std::error::Error for BootError {}

/// `bundle` has already passed complete container and plan validation. The
/// caller must verify provenance, exclusive ownership, DMA setup and recovery.
/// Any error requires device recovery; this routine never frees device buffers.
pub fn boot<T: McuIo + FirmwareStateIo>(mcu: &mut FirmwareMcu<T>, bundle: &FirmwareBundle) -> Result<(), BootError> {
    mcu.restart().map_err(BootError::Restart)?;
    // Unlike mt76's warning-only initial check, fail closed if power-on cannot
    // be observed: no firmware bytes should be sent to an unknown state.
    mcu.wait_state(FirmwareState::PowerOn).map_err(BootError::WaitPower)?;
    download::load_patch(mcu, bundle.patch()).map_err(BootError::Patch)?;
    download::load_ram(mcu, bundle.ram()).map_err(BootError::Ram)?;
    mcu.wait_state(FirmwareState::Running).map_err(BootError::WaitRunning)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::mt7922::firmware_mcu::Queue;
    #[derive(Default)]
    struct Fake { commands: Vec<u8>, power_fails: bool, running_fails: bool, waits: Vec<FirmwareState> }
    impl McuIo for Fake {
        fn exchange(&mut self, queue: Queue, bytes: &[u8], seq: Option<u8>) -> io::Result<Option<Vec<u8>>> {
            if queue == Queue::FirmwareDownload { return Ok(None); }
            let command = bytes[36];
            self.commands.push(command);
            let Some(seq) = seq else { return Ok(None) };
            let mut event = vec![0u8; 36];
            event[..4].copy_from_slice(&(7u32 << 27).to_le_bytes());
            event[29] = seq;
            if command == 0x10 { event[32] = if bytes[64] == 1 { 2 } else { 3 }; }
            Ok(Some(event))
        }
        fn drain_download(&mut self) -> io::Result<()> { Ok(()) }
        fn quiesce_download(&mut self) -> io::Result<()> { Ok(()) }
    }
    impl FirmwareStateIo for Fake {
        fn wait_firmware_state(&mut self, state: FirmwareState) -> io::Result<()> {
            self.waits.push(state);
            if (state == FirmwareState::PowerOn && self.power_fails) || (state == FirmwareState::Running && self.running_fails) {
                Err(io::Error::new(io::ErrorKind::TimedOut, "state timeout"))
            } else { Ok(()) }
        }
    }
    fn bundle() -> FirmwareBundle {
        let mut patch = vec![0; 164];
        for (at, n) in [(44, 1u32), (96, 2), (100, 160), (108, 0x200000), (112, 4)] {
            patch[at..at+4].copy_from_slice(&n.to_be_bytes());
        }
        let mut ram = vec![0; 80];
        ram[20..24].copy_from_slice(&0x1000u32.to_le_bytes());
        ram[24..28].copy_from_slice(&4u32.to_le_bytes());
        ram[46] = 1;
        FirmwareBundle::from_bytes(patch, ram).unwrap()
    }
    #[test] fn boots_in_order_and_waits_for_running_state() {
        let mut mcu = FirmwareMcu::new(Fake::default());
        boot(&mut mcu, &bundle()).unwrap();
        let fake = mcu.into_inner();
        assert_eq!(fake.commands, [4, 0x10, 5, 7, 0x10, 1, 2]);
        assert_eq!(fake.waits, [FirmwareState::PowerOn, FirmwareState::Running]);
    }
    #[test] fn power_failure_prevents_firmware_writes_and_poisoned_session() {
        let mut mcu = FirmwareMcu::new(Fake { power_fails: true, ..Default::default() });
        assert!(matches!(boot(&mut mcu, &bundle()), Err(BootError::WaitPower(_))));
        assert!(mcu.is_poisoned());
        assert!(mcu.restart().is_err());
        assert_eq!(mcu.into_inner().commands, [4]);
    }
    #[test] fn running_timeout_is_not_reported_as_success() {
        let mut mcu = FirmwareMcu::new(Fake { running_fails: true, ..Default::default() });
        assert!(matches!(boot(&mut mcu, &bundle()), Err(BootError::WaitRunning(_))));
        assert!(mcu.is_poisoned());
    }
}
