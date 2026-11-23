/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2023 MediaTek Inc.
 * PCIe LPCTL ownership sequence derived from mt76/mt792x_core.c and
 * mt792x_regs.h; see ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include <stdint.h>

#define AW_LPCTL_BAR_OFFSET 0xe0010u
#define AW_LPCTL_SET_FW_OWN 1u
#define AW_LPCTL_CLEAR_FW_OWN 2u
#define AW_LPCTL_FW_OWN_SYNC 4u
#define AW_OWN_RETRIES 10u
#define AW_OWN_POLLS 50u

typedef struct aw_own_io {
    void *context;
    int (*read)(void *context, uint32_t *value);
    int (*write)(void *context, uint32_t value);
    int (*delay_ms)(void *context, uint32_t ms);
    int (*inspect_dma)(void *context, uint32_t *flags);
} aw_own_io;

typedef enum aw_own_result {
    AW_OWN_OK = 1,
    AW_OWN_BUSY = 2,
    AW_OWN_UNAVAILABLE = 3,
    AW_OWN_CLAIM_FAILED_RESTORED = 4,
    AW_OWN_RECOVERY_FAILED = 5,
    AW_OWN_INSPECTION_FAILED_RESTORED = 6
} aw_own_result;

/* Bounded handoff based on mt792xe_mcu_{drv,fw}_pmctrl. A failed write can
 * have reached the device, so the caller must treat ownership as uncertain. */
static int aw_own_switch(const aw_own_io *io, int firmware) {
    uint32_t attempt, poll, value;
    for (attempt = 0; attempt < AW_OWN_RETRIES; ++attempt) {
        if (!io->write(io->context, firmware ? AW_LPCTL_SET_FW_OWN : AW_LPCTL_CLEAR_FW_OWN)) return 0;
        /* A two-millisecond wake delay covers the ASPM case in mt76. */
        if (!firmware && !io->delay_ms(io->context, 2)) return 0;
        for (poll = 0; poll < AW_OWN_POLLS; ++poll) {
            if (!io->read(io->context, &value) || value == UINT32_MAX) return 0;
            if (!!(value & AW_LPCTL_FW_OWN_SYNC) == !!firmware) return 1;
            if (!io->delay_ms(io->context, 1)) return 0;
        }
    }
    return 0;
}

/* Diagnostic transaction, never used to initialize the radio. It refuses to
 * take over an initially driver-owned device, and always tries to return the
 * device to firmware after issuing a claim, even if that claim timed out. */
static aw_own_result aw_own_roundtrip(const aw_own_io *io, uint32_t *dma_flags, int *dma_valid) {
    uint32_t value;
    int claimed, inspected = 0;
    if (!dma_flags || !dma_valid) return AW_OWN_UNAVAILABLE;
    *dma_flags = 0;
    *dma_valid = 0;
    if (!io || !io->read || !io->write || !io->delay_ms || !io->inspect_dma ||
        !io->read(io->context, &value) || value == UINT32_MAX) return AW_OWN_UNAVAILABLE;
    if (!(value & AW_LPCTL_FW_OWN_SYNC)) return AW_OWN_BUSY;
    claimed = aw_own_switch(io, 0);
    if (claimed) {
        inspected = io->inspect_dma(io->context, &value) && value != UINT32_MAX;
        if (inspected) { *dma_flags = value & 0xfu; *dma_valid = 1; }
    }
    if (!aw_own_switch(io, 1)) return AW_OWN_RECOVERY_FAILED;
    if (!claimed) return AW_OWN_CLAIM_FAILED_RESTORED;
    return inspected ? AW_OWN_OK : AW_OWN_INSPECTION_FAILED_RESTORED;
}
