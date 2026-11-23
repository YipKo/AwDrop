/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2023 MediaTek Inc.
 * Initial WFDMA disable and busy poll follow mt76/mt792x_dma.c.
 * See ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include <stdint.h>

#define AW_WFDMA0_CFG 0xd4208u
#define AW_WFDMA_DISABLE_MASK 0x18208005u
#define AW_WFDMA_ENABLE_BUSY 0x0000000fu
#define AW_WFDMA_STOP_POLLS 100u

typedef struct aw_dma_stop_io {
    void *context;
    int (*read)(void *context, uint32_t offset, uint32_t *value);
    int (*write)(void *context, uint32_t offset, uint32_t value);
    int (*delay_ms)(void *context, uint32_t ms);
} aw_dma_stop_io;
typedef enum aw_dma_stop_result {
    AW_DMA_STOP_IDLE = 1,
    AW_DMA_STOP_UNAVAILABLE = 2, /* No disable write was attempted. */
    AW_DMA_STOP_UNCERTAIN = 3    /* A write was attempted or the poll failed. */
} aw_dma_stop_result;

/* Only the first stage of mt792x_dma_disable. Observing idle is not by itself
 * an OS/device-removal fence and does not authorize freeing DMA memory. The
 * caller must hold device ownership, serialize PnP/MMIO, and separately handle
 * scheduler disable, reset, DMA synchronization, timeout and removal. */
static aw_dma_stop_result aw_dma_stop_primary(const aw_dma_stop_io *io) {
    uint32_t value, i;
    if (!io || !io->read || !io->write || !io->delay_ms ||
        !io->read(io->context, AW_WFDMA0_CFG, &value) || value == UINT32_MAX)
        return AW_DMA_STOP_UNAVAILABLE;
    if (!io->write(io->context, AW_WFDMA0_CFG, value & ~AW_WFDMA_DISABLE_MASK))
        return AW_DMA_STOP_UNCERTAIN;
    for (i = 0; i < AW_WFDMA_STOP_POLLS; ++i) {
        /* Read also flushes the preceding posted disable write. */
        if (!io->read(io->context, AW_WFDMA0_CFG, &value) || value == UINT32_MAX)
            return AW_DMA_STOP_UNCERTAIN;
        if (!(value & AW_WFDMA_ENABLE_BUSY)) return AW_DMA_STOP_IDLE;
        if (!io->delay_ms(io->context, 1)) return AW_DMA_STOP_UNCERTAIN;
    }
    return AW_DMA_STOP_UNCERTAIN;
}

/* Complete the force-disable sequence in mt792x_dma_disable. This does not
 * free buffers or itself guarantee an OS/DMA fence. Any partial write is an
 * uncertain device state and requires recovery. Not connected to an IOCTL. */
static aw_dma_stop_result aw_dma_stop_force(const aw_dma_stop_io *io) {
    uint32_t value;
    aw_dma_stop_result result = aw_dma_stop_primary(io);
    if (result != AW_DMA_STOP_IDLE) return result;
    if (!io->read(io->context, 0xd42b0u, &value) || value == UINT32_MAX ||
        !io->write(io->context, 0xd42b0u, value & ~0x40u) ||
        !io->read(io->context, 0xd42b0u, &value) || value == UINT32_MAX || (value & 0x40u)) return AW_DMA_STOP_UNCERTAIN;
    if (!io->read(io->context, 0xd6004u, &value) || value == UINT32_MAX ||
        !io->write(io->context, 0xd6004u, value | 0x10000000u) ||
        !io->read(io->context, 0xd6004u, &value) || value == UINT32_MAX || !(value & 0x10000000u)) return AW_DMA_STOP_UNCERTAIN;
    if (!io->read(io->context, 0xd4100u, &value) || value == UINT32_MAX ||
        !io->write(io->context, 0xd4100u, value & ~0x30u) ||
        !io->read(io->context, 0xd4100u, &value) || value == UINT32_MAX || (value & 0x30u) ||
        !io->write(io->context, 0xd4100u, value | 0x30u) ||
        !io->read(io->context, 0xd4100u, &value) || value == UINT32_MAX || (value & 0x30u) != 0x30u)
        return AW_DMA_STOP_UNCERTAIN;
    return AW_DMA_STOP_IDLE;
}
