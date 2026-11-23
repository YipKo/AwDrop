// SPDX-License-Identifier: BSD-3-Clause-Clear
// Copyright (C) 2020 MediaTek Inc.
// Connac2 PCIe mapping follows mt7921/pci.c and mt7921/regs.h at
// 41d5d1dd63111870ef74a5b9645416d98979a145. See LICENSE-mt7922-rx.txt.

//! Calculate MT7922 PCIe register mapping without performing any MMIO.
//! A live driver must serialize remaps and the access as one transaction,
//! flush posted writes, enforce power/removal state, and quiesce before unmap.
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapping {
    Direct { bar_offset: u32 },
    Remapped { bar_offset: u32, selector_offset: u32, selector_mask: u32, selector_value: u32 },
}

// Physical base, BAR offset, exclusive size. Entries are kept in upstream order.
const FIXED: &[(u32, u32, u32)] = &[
    (0x820d0000,0x30000,0x10000),(0x820ed000,0x24800,0x00800),
    (0x820e4000,0x21000,0x00400),(0x820e7000,0x21e00,0x00200),
    (0x820eb000,0x24200,0x00400),(0x820e2000,0x20800,0x00400),
    (0x820e3000,0x20c00,0x00400),(0x820e5000,0x21400,0x00800),
    (0x00400000,0x80000,0x10000),(0x00410000,0x90000,0x10000),
    (0x40000000,0x70000,0x10000),(0x54000000,0x02000,0x01000),
    (0x55000000,0x03000,0x01000),(0x58000000,0x06000,0x01000),
    (0x59000000,0x07000,0x01000),(0x7c000000,0xf0000,0x10000),
    (0x7c020000,0xd0000,0x10000),(0x7c060000,0xe0000,0x10000),
    (0x80020000,0xb0000,0x10000),(0x81020000,0xc0000,0x10000),
    (0x820c0000,0x08000,0x04000),(0x820c8000,0x0c000,0x02000),
    (0x820cc000,0x0e000,0x01000),(0x820cd000,0x0f000,0x01000),
    (0x74030000,0x10000,0x10000),(0x820ce000,0x21c00,0x00200),
    (0x820cf000,0x22000,0x01000),(0x820e0000,0x20000,0x00400),
    (0x820e1000,0x20400,0x00200),(0x820e9000,0x23400,0x00200),
    (0x820ea000,0x24000,0x00200),(0x820ec000,0x24600,0x00200),
    (0x820f0000,0xa0000,0x00400),(0x820f1000,0xa0600,0x00200),
    (0x820f2000,0xa0800,0x00400),(0x820f3000,0xa0c00,0x00400),
    (0x820f4000,0xa1000,0x00400),(0x820f5000,0xa1400,0x00800),
    (0x820f7000,0xa1e00,0x00200),(0x820f9000,0xa3400,0x00200),
    (0x820fa000,0xa4000,0x00200),(0x820fb000,0xa4200,0x00400),
    (0x820fc000,0xa4600,0x00200),(0x820fd000,0xa4800,0x00800),
];

/// Reject unaligned, unsupported, out-of-BAR, cross-region and overflowed
/// accesses. BAR size must come from a verified OS resource, never PCI IDs.
pub fn map(address: u32, width: u32, bar_bytes: u32) -> io::Result<Mapping> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid MT7922 register mapping");
    if !matches!(width, 1 | 2 | 4) || address % width != 0 { return Err(invalid()); }
    let end = address.checked_add(width - 1).ok_or_else(invalid)?;
    let bar_check = |offset: u32| -> io::Result<()> {
        if offset.checked_add(width).filter(|end| *end <= bar_bytes).is_none() { Err(invalid()) } else { Ok(()) }
    };
    if address < 0x100000 && end < 0x100000 {
        bar_check(address)?;
        return Ok(Mapping::Direct { bar_offset: address });
    }
    for &(physical, mapped, size) in FIXED {
        if let Some(offset) = address.checked_sub(physical) {
            if offset < size && width <= size - offset {
                let bar_offset = mapped + offset;
                bar_check(bar_offset)?;
                return Ok(Mapping::Direct { bar_offset });
            }
        }
    }
    let allowed = |addr| (0x18000000..0x18c00000).contains(&addr)
        || (0x70000000..0x78000000).contains(&addr)
        || (0x7c000000..0x7c400000).contains(&addr);
    if !allowed(address) || !allowed(end) || address >> 16 != end >> 16 { return Err(invalid()); }
    let offset = 0x40000 + (address & 0xffff);
    bar_check(offset)?;
    // The selector itself is a four-byte BAR register, regardless of width.
    if 0xfe24cu32.checked_add(4).filter(|end| *end <= bar_bytes).is_none() { return Err(invalid()); }
    Ok(Mapping::Remapped { bar_offset: offset, selector_offset: 0xfe24c, selector_mask: 0xffff, selector_value: address >> 16 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn direct_and_fixed_regions_are_resolved() {
        assert_eq!(map(0xd4208, 4, 0x100000).unwrap(), Mapping::Direct { bar_offset: 0xd4208 });
        assert_eq!(map(0x820e4004, 4, 0x100000).unwrap(), Mapping::Direct { bar_offset: 0x21004 });
        assert!(map(0x820e4400, 4, 0x100000).is_err());
    }
    #[test] fn dynamic_window_requires_both_selector_and_data_inside_bar() {
        assert_eq!(map(0x18000140, 4, 0x100000).unwrap(), Mapping::Remapped { bar_offset: 0x40140, selector_offset: 0xfe24c, selector_mask: 0xffff, selector_value: 0x1800 });
        assert!(map(0x18000140, 4, 0xfe24f).is_err());
        assert!(map(0x18000141, 4, 0x100000).is_err());
        assert!(map(0xffffffff, 2, 0x100000).is_err());
    }
}
