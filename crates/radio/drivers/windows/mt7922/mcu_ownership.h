/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2021 MediaTek Inc.
 * Firmware-MCU driver ownership follows mt76/mt7921/pci_mcu.c and the
 * MT7921 L1 remap helper. See ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include "wfsys_reset.h"

#define AW_L1_MCU_SELECTOR 0x1806u
#define AW_L1_MCU_LPCR 0x40010u
#define AW_MCU_DRV_OWN 2u
#define AW_MCU_FW_OWN 1u

/* Separate from PCIe LPCTL ownership. Caller owns and serializes the L1
 * remap window. On any failed write or timeout, device state is uncertain;
 * the original remap selector is still restored on a best-effort basis. */
static inline aw_reset_result aw_mcu_driver_own(const aw_reset_io *io) {
    uint32_t saved, value, i;
    int success = 0, restored;
    if (!io || !io->read || !io->write || !io->delay_ms ||
        !io->read(io->context, AW_L1_SELECTOR, &saved) || saved == UINT32_MAX) return AW_RESET_UNAVAILABLE;
    if (!io->write(io->context, AW_L1_SELECTOR, (saved & ~0xffffu) | AW_L1_MCU_SELECTOR)) goto restore;
    if (!io->read(io->context, AW_L1_SELECTOR, &value) ||
        value != ((saved & ~0xffffu) | AW_L1_MCU_SELECTOR)) goto restore;
    if (!io->write(io->context, AW_L1_MCU_LPCR, AW_MCU_DRV_OWN)) goto restore;
    for (i = 0; i < 500; ++i) {
        if (!io->read(io->context, AW_L1_MCU_LPCR, &value) || value == UINT32_MAX) goto restore;
        if (!(value & AW_MCU_FW_OWN)) { success = 1; break; }
        if (!io->delay_ms(io->context, 1)) goto restore;
    }
restore:
    restored = io->write(io->context, AW_L1_SELECTOR, saved);
    if (restored) restored = io->read(io->context, AW_L1_SELECTOR, &value) && value == saved;
    return success && restored ? AW_RESET_OK : AW_RESET_UNCERTAIN;
}
