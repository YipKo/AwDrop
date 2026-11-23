// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2016 Felix Fietkau <nbd@nbd.name>
// Copyright (C) 2020 MediaTek Inc.
// Descriptor and queue rules derived from mt76 and the reference project's
// driver/mt7925/core.c. See LICENSE-mt7922-rx.txt.

//! Offline 32-bit MT76 TX descriptor and ring bookkeeping model.
//! Not a DMA allocator: never use this owned-memory model directly as a
//! hardware ring; a driver must supply coherent buffers, barriers and MMIO.

use std::io;
const DONE: u32 = 0x8000_0000;
const LAST: u32 = 0x4000_0000;
const MAX_LENGTH: u32 = 0x3fff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor { pub address: u32, pub control: u32, pub address1: u32, pub info: u32 }
impl Descriptor {
    pub fn bytes(self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        for (chunk, word) in bytes.chunks_exact_mut(4).zip([self.address, self.control, self.address1, self.info]) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

pub fn tx_descriptor(address: u64, length: u32) -> io::Result<Descriptor> {
    if length == 0 || length > MAX_LENGTH || address > u32::MAX as u64
        || address + (length as u64 - 1) > u32::MAX as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "DMA span is outside the 32-bit descriptor"));
    }
    Ok(Descriptor { address: address as u32, control: (length << 16) | LAST, address1: 0, info: 0 })
}

#[derive(Debug)]
pub struct Ring { entries: Vec<Descriptor>, head: usize, tail: usize, queued: usize }
impl Ring {
    pub fn new(count: usize) -> io::Result<Self> {
        if !(4..=4096).contains(&count) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid ring size")); }
        Ok(Self { entries: vec![Descriptor { address: 0, control: DONE, address1: 0, info: 0 }; count], head: 0, tail: 0, queued: 0 })
    }
    pub fn submit(&mut self, address: u64, length: u32) -> io::Result<usize> {
        let descriptor = tx_descriptor(address, length)?;
        if self.queued >= self.entries.len() - 2 { return Err(io::Error::other("ring full")); }
        if self.entries[self.head].control & DONE == 0 { return Err(io::Error::other("descriptor not ready")); }
        let index = self.head;
        self.entries[index] = descriptor;
        self.head = (self.head + 1) % self.entries.len();
        self.queued += 1;
        Ok(index)
    }
    pub fn reclaim(&mut self, dma_index: usize) -> io::Result<Option<usize>> {
        if dma_index >= self.entries.len() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "completion index outside ring")); }
        let completed = (dma_index + self.entries.len() - self.tail) % self.entries.len();
        if completed > self.queued { return Err(io::Error::new(io::ErrorKind::InvalidData, "completion exceeds queued entries")); }
        if completed == 0 { return Ok(None); }
        let index = self.tail;
        self.entries[index].control = DONE;
        self.tail = (self.tail + 1) % self.entries.len();
        self.queued -= 1;
        Ok(Some(index))
    }
    pub fn descriptor(&self, index: usize) -> Option<Descriptor> { self.entries.get(index).copied() }
    pub fn queued(&self) -> usize { self.queued }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn descriptor_encodes_little_endian_and_bounds_dma() {
        assert_eq!(tx_descriptor(0x12345678, 4).unwrap().bytes(), [0x78,0x56,0x34,0x12,0,0,4,0x40,0,0,0,0,0,0,0,0]);
        assert!(tx_descriptor(u32::MAX as u64, 2).is_err());
        assert!(tx_descriptor(0, 0).is_err());
        assert!(tx_descriptor(0, MAX_LENGTH + 1).is_err());
    }
    #[test] fn ring_reserves_slots_and_checks_completion_across_wrap() {
        let mut ring = Ring::new(4).unwrap();
        assert_eq!(ring.submit(100, 4).unwrap(), 0); assert_eq!(ring.submit(200, 4).unwrap(), 1);
        assert!(ring.submit(300, 4).is_err());
        assert!(ring.reclaim(3).is_err());
        assert_eq!(ring.reclaim(2).unwrap(), Some(0)); assert_eq!(ring.reclaim(2).unwrap(), Some(1));
        assert_eq!(ring.submit(300, 4).unwrap(), 2); assert_eq!(ring.submit(400, 4).unwrap(), 3);
        assert_eq!(ring.reclaim(0).unwrap(), Some(2)); assert_eq!(ring.reclaim(0).unwrap(), Some(3));
        assert_eq!(ring.reclaim(0).unwrap(), None); assert_eq!(ring.queued(), 0);
        assert_eq!(ring.submit(500, 4).unwrap(), 0);
    }
}
