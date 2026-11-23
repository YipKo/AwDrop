// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020 MediaTek Inc.
// Connac2 download sequencing and flags follow mt76_connac_mcu.c and
// mt76_connac_mcu.h at revision 41d5d1dd63111870ef74a5b9645416d98979a145.
// See LICENSE-mt7922-rx.txt.

//! Offline PCIe firmware download plans. These describe a possible sequence;
//! they cannot access hardware or establish that firmware is trustworthy.
//! A future transport must handle MCU responses, timeouts, patch semaphore
//! ownership/release on all error paths, device resets and DMA lifetime.
use super::firmware::{patch_regions, ram_regions, Region};
use std::io;

const CHUNK_SIZE: usize = 4096;
const NEED_RESPONSE: u32 = 1 << 31;
const ENCRYPT: u32 = 1;
const RESET_IV: u32 = 1 << 3;
const ENCRYPT_MODE: u32 = 1 << 6;
const OVERRIDE: u32 = 1 << 5;
const NON_DOWNLOAD: u32 = 1 << 6;

#[derive(Debug, PartialEq, Eq)]
pub enum InitCommand { PatchStart, TargetAddressLength }

#[derive(Debug, PartialEq, Eq)]
pub enum DownloadStep<'a> {
    /// The caller must first obtain the patch semaphore. If the firmware is
    /// already patched, it must skip the other patch steps and not release it.
    AcquirePatch,
    Init { command: InitCommand, address: u32, length: u32, mode: u32 },
    Scatter(&'a [u8]),
    FinishPatch,
    ReleasePatch,
    StartRam { address: u32, override_address: bool },
}

fn invalid(message: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }

fn append_region<'a>(steps: &mut Vec<DownloadStep<'a>>, region: &Region<'a>, mode: u32) -> io::Result<()> {
    let length = u32::try_from(region.data.len()).map_err(|_| invalid("firmware region too large"))?;
    if length == 0 || region.address.checked_add(length - 1).is_none() {
        return Err(invalid("firmware region has an invalid address range"));
    }
    let command = if matches!(region.address, 0x200000 | 0x900000) {
        InitCommand::PatchStart
    } else { InitCommand::TargetAddressLength };
    steps.push(DownloadStep::Init { command, address: region.address, length, mode });
    steps.extend(region.data.chunks(CHUNK_SIZE).map(DownloadStep::Scatter));
    Ok(())
}

fn patch_mode(info: u32) -> io::Result<u32> {
    if info == u32::MAX { return Ok(NEED_RESPONSE); }
    match info >> 24 {
        0 => Ok(NEED_RESPONSE),
        1 => Ok(NEED_RESPONSE | ENCRYPT | RESET_IV | ((info & 0xff) << 1 & 0x6)),
        2 => Ok(NEED_RESPONSE | ENCRYPT | RESET_IV | ENCRYPT_MODE),
        _ => Err(invalid("unsupported patch encryption type")),
    }
}

/// Plan the patch path when the device reports that a patch must be loaded.
/// A semaphore response indicating 'already loaded' takes a separate path.
pub fn patch(data: &[u8]) -> io::Result<Vec<DownloadStep<'_>>> {
    let regions = patch_regions(data)?;
    let mut steps = vec![DownloadStep::AcquirePatch];
    for region in &regions { append_region(&mut steps, region, patch_mode(region.flags)?)?; }
    steps.extend([DownloadStep::FinishPatch, DownloadStep::ReleasePatch]);
    Ok(steps)
}

/// Plan the MT7922 WM RAM path. Non-download regions advance the input offset
/// but do not produce a download operation. The WA/CR4 path is not modeled.
pub fn ram(data: &[u8]) -> io::Result<Vec<DownloadStep<'_>>> {
    let regions = ram_regions(data)?;
    let mut steps = Vec::new();
    let mut override_addr = 0;
    for region in &regions {
        let flags = region.flags;
        if flags & NON_DOWNLOAD != 0 { continue; }
        if flags & OVERRIDE != 0 { override_addr = region.address; }
        let mut mode = NEED_RESPONSE | (flags & 0x6);
        if flags & 1 != 0 { mode |= ENCRYPT | RESET_IV; }
        if flags & (1 << 4) != 0 { mode |= ENCRYPT_MODE; }
        append_region(&mut steps, region, mode)?;
    }
    steps.push(DownloadStep::StartRam { address: override_addr, override_address: override_addr != 0 });
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put_be(data: &mut [u8], offset: usize, value: u32) { data[offset..offset+4].copy_from_slice(&value.to_be_bytes()); }
    fn put_le(data: &mut [u8], offset: usize, value: u32) { data[offset..offset+4].copy_from_slice(&value.to_le_bytes()); }
    #[test]
    fn patch_chunks_and_security_flags_are_preserved() {
        let mut data = vec![0; 160 + 4100];
        put_be(&mut data, 44, 1); put_be(&mut data, 96, 2); put_be(&mut data, 100, 160);
        put_be(&mut data, 108, 0x200000); put_be(&mut data, 112, 4100); put_be(&mut data, 116, 0x01000003);
        let plan = patch(&data).unwrap();
        assert_eq!(plan[0], DownloadStep::AcquirePatch);
        assert_eq!(plan[1], DownloadStep::Init { command: InitCommand::PatchStart, address: 0x200000, length: 4100, mode: NEED_RESPONSE | 0xf });
        assert!(matches!(&plan[2], DownloadStep::Scatter(bytes) if bytes.len() == 4096));
        assert!(matches!(&plan[3], DownloadStep::Scatter(bytes) if bytes.len() == 4));
        assert_eq!(&plan[4..], &[DownloadStep::FinishPatch, DownloadStep::ReleasePatch]);
        put_be(&mut data, 116, 0x7f000000);
        assert!(patch(&data).is_err());
    }
    #[test]
    fn ram_skips_non_download_and_sets_start_address() {
        let mut data = vec![0; 8 + 2*40 + 36];
        data[..8].copy_from_slice(&[1,2,3,4,5,6,7,8]);
        put_le(&mut data, 8+16, 0x1000); put_le(&mut data, 8+20, 4); data[8+24] = 0x40;
        put_le(&mut data, 48+16, 0x2000); put_le(&mut data, 48+20, 4); data[48+24] = 0x33;
        data[88+2] = 2;
        assert_eq!(ram(&data).unwrap(), [
            DownloadStep::Init { command: InitCommand::TargetAddressLength, address: 0x2000, length: 4, mode: NEED_RESPONSE | 0x4b },
            DownloadStep::Scatter(&[5,6,7,8]),
            DownloadStep::StartRam { address: 0x2000, override_address: true },
        ]);
    }
}
