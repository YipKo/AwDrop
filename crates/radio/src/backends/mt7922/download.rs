// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020 MediaTek Inc.
// Download sequence derived from mt76_connac_mcu.c at revision
// 41d5d1dd63111870ef74a5b9645416d98979a145. See LICENSE-mt7922-rx.txt.

//! Transport-agnostic Connac2 firmware download sequencing.
//!
//! This module alone does not access a card. A future transport must implement
//! device-specific MCU responses, timeouts, DMA ownership and error recovery.
//! The caller must verify firmware provenance and acquire exclusive device
//! ownership before invoking it with a real transport.
use super::download_plan::{self, DownloadStep, InitCommand};
use std::{fmt, io};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchSemaphore { AlreadyLoaded, Acquired }

pub trait FirmwareTransport {
    type Error: std::error::Error + Send + Sync + 'static;
    fn acquire_patch(&mut self) -> Result<PatchSemaphore, Self::Error>;
    fn init(&mut self, command: &InitCommand, address: u32, length: u32, mode: u32) -> Result<(), Self::Error>;
    fn scatter(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Wait until all submitted firmware download descriptors have completed.
    /// Command and download queues are independent, so returning early may
    /// let a later command overtake firmware data.
    fn drain_download(&mut self) -> Result<(), Self::Error>;
    /// Fence pending download data after an error. Success means no pending
    /// data can reach the MCU and the command queue is still usable; otherwise
    /// the owner must reset/recover the device rather than release a semaphore.
    fn quiesce_download(&mut self) -> Result<(), Self::Error>;
    fn finish_patch(&mut self) -> Result<(), Self::Error>;
    fn release_patch(&mut self) -> Result<(), Self::Error>;
    fn start_ram(&mut self, address: u32, override_address: bool) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum DownloadError<E> {
    InvalidFirmware(io::Error),
    Transport(E),
    Release(E),
    /// Both errors are preserved so a failed release does not hide the first failure.
    TransportAndRelease { operation: E, release: E },
    /// Outstanding DMA could not be fenced; the caller must recover the device.
    TransportAndQuiesce { operation: E, quiesce: E },
}
impl<E: fmt::Display> fmt::Display for DownloadError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFirmware(e) => write!(f, "invalid firmware: {e}"),
            Self::Transport(e) => write!(f, "firmware operation failed: {e}"),
            Self::Release(e) => write!(f, "patch semaphore release failed: {e}"),
            Self::TransportAndRelease { operation, release } => write!(f, "firmware operation failed: {operation}; semaphore release also failed: {release}"),
            Self::TransportAndQuiesce { operation, quiesce } => write!(f, "firmware operation failed: {operation}; download queue could not be fenced: {quiesce}"),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for DownloadError<E> {}

/// Validate the entire patch before acquiring the semaphore. If acquired,
/// release is attempted once after success or after pending downloads have
/// been fenced. A failed fence requires device recovery instead of release.
pub fn load_patch<T: FirmwareTransport>(transport: &mut T, image: &[u8]) -> Result<(), DownloadError<T::Error>> {
    let plan = download_plan::patch(image).map_err(DownloadError::InvalidFirmware)?;
    match transport.acquire_patch().map_err(DownloadError::Transport)? {
        PatchSemaphore::AlreadyLoaded => return Ok(()),
        PatchSemaphore::Acquired => {}
    }
    let mut pending_data = false;
    let operation = (|| {
        for step in plan {
            match step {
                DownloadStep::Init { command, address, length, mode } => {
                    if pending_data { transport.drain_download()?; pending_data = false; }
                    transport.init(&command, address, length, mode)?;
                }
                DownloadStep::Scatter(bytes) => { pending_data = true; transport.scatter(bytes)?; }
                DownloadStep::FinishPatch => {
                    if pending_data { transport.drain_download()?; pending_data = false; }
                    transport.finish_patch()?;
                }
                DownloadStep::AcquirePatch | DownloadStep::ReleasePatch => {},
                DownloadStep::StartRam { .. } => unreachable!("patch plan contains RAM start"),
            }
        }
        Ok(())
    })();
    if let Err(operation) = operation {
        if pending_data {
            if let Err(quiesce) = transport.quiesce_download() {
                return Err(DownloadError::TransportAndQuiesce { operation, quiesce });
            }
        }
        return match transport.release_patch() {
            Ok(()) => Err(DownloadError::Transport(operation)),
            Err(release) => Err(DownloadError::TransportAndRelease { operation, release }),
        };
    }
    match transport.release_patch() {
        Ok(()) => Ok(()),
        Err(error) => Err(DownloadError::Release(error)),
    }
}

/// The transport owns reset/recovery after a partial RAM download failure.
pub fn load_ram<T: FirmwareTransport>(transport: &mut T, image: &[u8]) -> Result<(), DownloadError<T::Error>> {
    let plan = download_plan::ram(image).map_err(DownloadError::InvalidFirmware)?;
    let mut pending_data = false;
    let operation = (|| {
        for step in plan {
            match step {
            DownloadStep::Init { command, address, length, mode } => {
                if pending_data { transport.drain_download()?; pending_data = false; }
                transport.init(&command, address, length, mode)?;
            }
            DownloadStep::Scatter(bytes) => {
                pending_data = true;
                transport.scatter(bytes)?;
            }
            DownloadStep::StartRam { address, override_address } => {
                if pending_data { transport.drain_download()?; pending_data = false; }
                transport.start_ram(address, override_address)?;
            }
            DownloadStep::AcquirePatch | DownloadStep::FinishPatch | DownloadStep::ReleasePatch => unreachable!("RAM plan contains patch operation"),
            }
        }
        Ok(())
    })();
    if let Err(operation) = operation {
        if pending_data {
            if let Err(quiesce) = transport.quiesce_download() {
                return Err(DownloadError::TransportAndQuiesce { operation, quiesce });
            }
        }
        return Err(DownloadError::Transport(operation));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake { events: Vec<&'static str>, already: bool, fail_scatter: bool, fail_release: bool, fail_drain: bool, fail_quiesce: bool }
    impl FirmwareTransport for Fake {
        type Error = io::Error;
        fn acquire_patch(&mut self) -> io::Result<PatchSemaphore> { self.events.push("acquire"); Ok(if self.already { PatchSemaphore::AlreadyLoaded } else { PatchSemaphore::Acquired }) }
        fn init(&mut self, _: &InitCommand, _: u32, _: u32, _: u32) -> io::Result<()> { self.events.push("init"); Ok(()) }
        fn scatter(&mut self, _: &[u8]) -> io::Result<()> { self.events.push("scatter"); if self.fail_scatter { Err(io::Error::other("scatter")) } else { Ok(()) } }
        fn drain_download(&mut self) -> io::Result<()> { self.events.push("drain"); if self.fail_drain { Err(io::Error::other("drain")) } else { Ok(()) } }
        fn quiesce_download(&mut self) -> io::Result<()> { self.events.push("quiesce"); if self.fail_quiesce { Err(io::Error::other("quiesce")) } else { Ok(()) } }
        fn finish_patch(&mut self) -> io::Result<()> { self.events.push("finish"); Ok(()) }
        fn release_patch(&mut self) -> io::Result<()> { self.events.push("release"); if self.fail_release { Err(io::Error::other("release")) } else { Ok(()) } }
        fn start_ram(&mut self, _: u32, _: bool) -> io::Result<()> { self.events.push("start"); Ok(()) }
    }
    fn patch_fixture() -> Vec<u8> {
        let mut data = vec![0; 164];
        for (offset, value) in [(44, 1u32), (96, 2), (100, 160), (108, 0x200000), (112, 4)] {
            data[offset..offset+4].copy_from_slice(&value.to_be_bytes());
        }
        data
    }
    #[test] fn success_and_already_loaded_paths() {
        let image = patch_fixture();
        let mut fake = Fake::default();
        load_patch(&mut fake, &image).unwrap();
        assert_eq!(fake.events, ["acquire", "init", "scatter", "drain", "finish", "release"]);
        let mut fake = Fake { already: true, ..Default::default() };
        load_patch(&mut fake, &image).unwrap();
        assert_eq!(fake.events, ["acquire"]);
    }
    #[test] fn error_paths_release_once_and_preserve_both_failures() {
        let mut fake = Fake { fail_scatter: true, fail_release: true, ..Default::default() };
        let error = load_patch(&mut fake, &patch_fixture()).unwrap_err();
        assert!(matches!(error, DownloadError::TransportAndRelease { .. }));
        assert_eq!(fake.events, ["acquire", "init", "scatter", "quiesce", "release"]);
        let mut fake = Fake::default();
        assert!(matches!(load_patch(&mut fake, &[0]), Err(DownloadError::InvalidFirmware(_))));
        assert!(fake.events.is_empty());
    }
    #[test] fn ram_is_not_started_after_scatter_failure() {
        let mut image = vec![0; 4 + 40 + 36];
        image[20..24].copy_from_slice(&0x2000u32.to_le_bytes());
        image[24..28].copy_from_slice(&4u32.to_le_bytes());
        image[46] = 1;
        let mut fake = Fake { fail_scatter: true, ..Default::default() };
        assert!(matches!(load_ram(&mut fake, &image), Err(DownloadError::Transport(_))));
        assert_eq!(fake.events, ["init", "scatter", "quiesce"]);
    }
    #[test] fn patch_drain_failure_releases_and_does_not_finish() {
        let mut fake = Fake { fail_drain: true, ..Default::default() };
        assert!(matches!(load_patch(&mut fake, &patch_fixture()), Err(DownloadError::Transport(_))));
        assert_eq!(fake.events, ["acquire", "init", "scatter", "drain", "quiesce", "release"]);
    }
    #[test] fn failed_quiesce_does_not_send_semaphore_release() {
        let mut fake = Fake { fail_drain: true, fail_quiesce: true, ..Default::default() };
        assert!(matches!(load_patch(&mut fake, &patch_fixture()), Err(DownloadError::TransportAndQuiesce { .. })));
        assert_eq!(fake.events, ["acquire", "init", "scatter", "drain", "quiesce"]);
    }
}
