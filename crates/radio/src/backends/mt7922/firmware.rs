// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020 MediaTek Inc.
// Format and bounds rules derived from mt76 revision
// 41d5d1dd63111870ef74a5b9645416d98979a145, mt76_connac_mcu.h and
// mt76_connac_mcu.c. See LICENSE-mt7922-rx.txt.

//! Parse Connac2 firmware download containers without loading or executing
//! them. A future device implementation must separately authenticate the
//! exact firmware and implement the MCU download protocol.

use std::{fs::File, io::{self, Read}, path::Path};
const PATCH_HEADER: usize = 96;
const PATCH_REGION: usize = 64;
const RAM_TRAILER: usize = 36;
const RAM_REGION: usize = 40;
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
pub const PATCH_FILE: &str = "WIFI_MT7922_patch_mcu_1_1_hdr.bin";
pub const RAM_FILE: &str = "WIFI_RAM_CODE_MT7922_1.bin";

/// Explicitly selected local firmware. Validation checks the full download
/// plan before returning; it does not authenticate its source or execute it.
pub struct FirmwareBundle { patch: Vec<u8>, ram: Vec<u8> }
impl FirmwareBundle {
    pub fn from_directory(dir: &Path) -> io::Result<Self> {
        fn read_image(path: &Path) -> io::Result<Vec<u8>> {
            let file = File::open(path)?;
            if !file.metadata()?.is_file() { return Err(invalid()); }
            let mut bytes = Vec::new();
            file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_IMAGE_BYTES { return Err(invalid()); }
            Ok(bytes)
        }
        Self::from_bytes(read_image(&dir.join(PATCH_FILE))?, read_image(&dir.join(RAM_FILE))?)
    }
    pub fn from_bytes(patch: Vec<u8>, ram: Vec<u8>) -> io::Result<Self> {
        if patch.len() as u64 > MAX_IMAGE_BYTES || ram.len() as u64 > MAX_IMAGE_BYTES { return Err(invalid()); }
        // Planning additionally verifies encryption modes and address overflow.
        super::download_plan::patch(&patch)?;
        super::download_plan::ram(&ram)?;
        Ok(Self { patch, ram })
    }
    pub fn patch(&self) -> &[u8] { &self.patch }
    pub fn ram(&self) -> &[u8] { &self.ram }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Region<'a> { pub address: u32, pub flags: u32, pub data: &'a [u8] }

fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "invalid Connac2 firmware container") }
fn be32(data: &[u8], offset: usize) -> io::Result<u32> {
    Ok(u32::from_be_bytes(data.get(offset..offset + 4).ok_or_else(invalid)?.try_into().unwrap()))
}
fn le32(data: &[u8], offset: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(data.get(offset..offset + 4).ok_or_else(invalid)?.try_into().unwrap()))
}

pub fn patch_regions(data: &[u8]) -> io::Result<Vec<Region<'_>>> {
    if data.len() < PATCH_HEADER { return Err(invalid()); }
    let count = be32(data, 44)? as usize;
    if count > (data.len() - PATCH_HEADER) / PATCH_REGION { return Err(invalid()); }
    let table_end = PATCH_HEADER + count * PATCH_REGION;
    let mut regions = Vec::with_capacity(count);
    for i in 0..count {
        let start = PATCH_HEADER + i * PATCH_REGION;
        if be32(data, start)? & 0xffff != 2 { return Err(invalid()); }
        let offset = be32(data, start + 4)? as usize;
        let length = be32(data, start + 16)? as usize;
        let end = offset.checked_add(length).ok_or_else(invalid)?;
        if offset < table_end || length == 0 { return Err(invalid()); }
        let bytes = data.get(offset..end).ok_or_else(invalid)?;
        regions.push(Region { address: be32(data, start + 12)?, flags: be32(data, start + 20)?, data: bytes });
    }
    if regions.is_empty() { return Err(invalid()); }
    Ok(regions)
}

pub fn ram_regions(data: &[u8]) -> io::Result<Vec<Region<'_>>> {
    if data.len() < RAM_TRAILER { return Err(invalid()); }
    let trailer = data.len() - RAM_TRAILER;
    let count = data[trailer + 2] as usize;
    if count == 0 || count > trailer / RAM_REGION { return Err(invalid()); }
    let table = trailer - count * RAM_REGION;
    let mut offset = 0usize;
    let mut regions = Vec::with_capacity(count);
    for i in 0..count {
        let entry = table + i * RAM_REGION;
        let length = le32(data, entry + 20)? as usize;
        let end = offset.checked_add(length).ok_or_else(invalid)?;
        if length == 0 || end > table { return Err(invalid()); }
        regions.push(Region { address: le32(data, entry + 16)?, flags: data[entry + 24] as u32, data: &data[offset..end] });
        offset = end;
    }
    Ok(regions)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put_be(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at+4].copy_from_slice(&value.to_be_bytes()); }
    fn put_le(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at+4].copy_from_slice(&value.to_le_bytes()); }
    #[test] fn patch_checks_region_table_and_payload_bounds() {
        let mut bytes = vec![0; PATCH_HEADER + PATCH_REGION + 4];
        put_be(&mut bytes, 44, 1); put_be(&mut bytes, 96, 2); put_be(&mut bytes, 100, 160);
        put_be(&mut bytes, 108, 0x1234); put_be(&mut bytes, 112, 4); bytes[160..].copy_from_slice(&[1,2,3,4]);
        assert_eq!(patch_regions(&bytes).unwrap(), [Region { address: 0x1234, flags: 0, data: &[1,2,3,4] }]);
        put_be(&mut bytes, 100, 159); assert!(patch_regions(&bytes).is_err());
        put_be(&mut bytes, 100, u32::MAX); assert!(patch_regions(&bytes).is_err());
        put_be(&mut bytes, 44, u32::MAX); assert!(patch_regions(&bytes).is_err());
    }
    #[test] fn ram_checks_trailer_table_and_payload_bounds() {
        let mut bytes = vec![0; 4 + RAM_REGION + RAM_TRAILER];
        bytes[0..4].copy_from_slice(&[4,3,2,1]); put_le(&mut bytes, 20, 0x5678);
        put_le(&mut bytes, 24, 4); bytes[28] = 7; bytes[46] = 1;
        assert_eq!(ram_regions(&bytes).unwrap(), [Region { address: 0x5678, flags: 7, data: &[4,3,2,1] }]);
        put_le(&mut bytes, 24, 5); assert!(ram_regions(&bytes).is_err());
        bytes[46] = 255; assert!(ram_regions(&bytes).is_err());
    }
    #[test] fn selected_local_bundle_must_be_valid_and_bounded() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("awdrop-fw-test-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let patch_path = dir.join(PATCH_FILE);
        let ram_path = dir.join(RAM_FILE);
        let mut patch = vec![0; PATCH_HEADER + PATCH_REGION + 4];
        put_be(&mut patch, 44, 1); put_be(&mut patch, 96, 2); put_be(&mut patch, 100, 160);
        put_be(&mut patch, 108, 0x200000); put_be(&mut patch, 112, 4);
        let mut ram = vec![0; 4 + RAM_REGION + RAM_TRAILER];
        put_le(&mut ram, 20, 0x1000); put_le(&mut ram, 24, 4); ram[46] = 1;
        std::fs::write(&patch_path, &patch).unwrap();
        std::fs::write(&ram_path, &ram).unwrap();
        let bundle = FirmwareBundle::from_directory(&dir).unwrap();
        assert_eq!(bundle.patch(), patch);
        assert_eq!(bundle.ram(), ram);
        File::create(&patch_path).unwrap().set_len(MAX_IMAGE_BYTES + 1).unwrap();
        assert!(FirmwareBundle::from_directory(&dir).is_err());
        std::fs::remove_file(patch_path).unwrap();
        std::fs::remove_file(ram_path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
