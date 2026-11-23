/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2020, 2023 MediaTek Inc.
 * MT7921/MT7922 L1 remap and WFSYS reset are derived from mt76/mt7921/mt7921.h,
 * mt7921/regs.h and mt792x_dma.c. See ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include <stdint.h>

#define AW_L1_SELECTOR 0xfe24cu
#define AW_L1_RESET_WINDOW 0x40140u
#define AW_L1_RESET_SELECTOR 0x1800u
#define AW_WFSYS_RESET_BIT 1u
#define AW_WFSYS_READY_BIT 16u

typedef struct aw_reset_io {
    void *context;
    int (*read)(void *, uint32_t, uint32_t *);
    int (*write)(void *, uint32_t, uint32_t);
    int (*delay_ms)(void *, uint32_t);
} aw_reset_io;
typedef enum aw_reset_result {
    AW_RESET_OK = 1,
    AW_RESET_UNAVAILABLE = 2, /* No write was attempted. */
    AW_RESET_UNCERTAIN = 3   /* A write may have changed the device. */
} aw_reset_result;

/* Caller must have exclusively claimed the device, serialized the remap
 * window with every other register access, and handled DMA/interrupts before
 * reset. Any failure after a write requires platform/device recovery. This
 * function is NOT connected to the diagnostic IOCTL or executed at startup. */
static inline aw_reset_result aw_wfsys_reset(const aw_reset_io *io) {
    uint32_t saved, readback, value, attempt;
    int succeeded = 0, restore, mapped = 0, reset_maybe_asserted = 0;
    if (!io || !io->read || !io->write || !io->delay_ms ||
        !io->read(io->context, AW_L1_SELECTOR, &saved) || saved == UINT32_MAX)
        return AW_RESET_UNAVAILABLE;

    /* A failed write may still have reached the device, so even that path
     * attempts to restore the original selector before returning. */
    if (!io->write(io->context, AW_L1_SELECTOR, (saved & ~0xffffu) | AW_L1_RESET_SELECTOR)) goto restore_selector;
    if (!io->read(io->context, AW_L1_SELECTOR, &readback) ||
        readback != ((saved & ~0xffffu) | AW_L1_RESET_SELECTOR)) goto restore_selector;
    mapped = 1;
    if (!io->read(io->context, AW_L1_RESET_WINDOW, &value) || value == UINT32_MAX) goto restore_selector;
    reset_maybe_asserted = 1; /* A failed write may still have reached hardware. */
    if (!io->write(io->context, AW_L1_RESET_WINDOW, value & ~AW_WFSYS_RESET_BIT)) goto restore_selector;
    if (!io->delay_ms(io->context, 50)) goto restore_selector;
    if (!io->read(io->context, AW_L1_RESET_WINDOW, &value) || value == UINT32_MAX) goto restore_selector;
    if (!io->write(io->context, AW_L1_RESET_WINDOW, value | AW_WFSYS_RESET_BIT)) goto restore_selector;
    reset_maybe_asserted = 0;
    for (attempt = 0; attempt < 500; ++attempt) {
        if (!io->read(io->context, AW_L1_RESET_WINDOW, &value) || value == UINT32_MAX) goto restore_selector;
        if ((value & (AW_WFSYS_RESET_BIT | AW_WFSYS_READY_BIT)) ==
            (AW_WFSYS_RESET_BIT | AW_WFSYS_READY_BIT)) { succeeded = 1; break; }
        if (!io->delay_ms(io->context, 1)) goto restore_selector;
    }
restore_selector:
    if (mapped && reset_maybe_asserted && io->read(io->context, AW_L1_RESET_WINDOW, &value) && value != UINT32_MAX)
        (void)io->write(io->context, AW_L1_RESET_WINDOW, value | AW_WFSYS_RESET_BIT);
    restore = io->write(io->context, AW_L1_SELECTOR, saved);
    if (restore) restore = io->read(io->context, AW_L1_SELECTOR, &readback) && readback == saved;
    return succeeded && restore ? AW_RESET_OK : AW_RESET_UNCERTAIN;
}
