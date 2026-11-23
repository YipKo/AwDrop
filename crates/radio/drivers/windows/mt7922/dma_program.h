/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2016 Felix Fietkau <nbd@nbd.name>
 * Copyright (C) 2020, 2023 MediaTek Inc.
 * MT7922 queue numbering and ring setup follow mt76/dma.c, mt7921/pci.c and
 * mt7921/mt7921.h. See ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include "dma_memory.h"

#define AW_WFDMA_CONFIG 0xd4208u
#define AW_MCU_RING_REGS 0xd4410u /* TX queue 17, not MT7925 queue 15 */
#define AW_FWDL_RING_REGS 0xd4400u /* TX queue 16 */
#define AW_EVENT_RING_REGS 0xd4500u /* RX queue 0 */

typedef struct aw_ring_program_io {
    void *context;
    int (*read)(void *, uint32_t, uint32_t *);
    int (*write)(void *, uint32_t, uint32_t);
    void (*barrier)(void *);
} aw_ring_program_io;
typedef struct aw_ring_program_state {
    uint32_t modified;  /* A register write was attempted. */
    uint32_t published; /* A device DMA address write was attempted. */
    uint32_t ready;     /* All register readbacks matched. */
} aw_ring_program_state;

static int aw_disjoint32(uint64_t a, uint32_t alen, uint64_t b, uint32_t blen) {
    return a + alen <= b || b + blen <= a;
}
static int aw_program_one_ring(const aw_ring_program_io *io, aw_ring_program_state *state,
                               uint32_t reg, uint32_t address, uint32_t count, uint32_t cpu) {
    uint32_t value;
    state->modified = 1;
    if (!io->write(io->context, reg + 4, 0) || !io->write(io->context, reg + 8, 0) ||
        !io->write(io->context, reg + 12, 0)) return 0;
    /* Set BEFORE attempting the write: an error can be reported after the
     * device has already observed the address. */
    state->published = 1;
    if (!io->write(io->context, reg, address) || !io->write(io->context, reg + 8, cpu) ||
        !io->write(io->context, reg + 4, count)) return 0;
    if (!io->read(io->context, reg, &value) || value != address ||
        !io->read(io->context, reg + 4, &value) || value != count ||
        !io->read(io->context, reg + 8, &value) || value != cpu ||
        !io->read(io->context, reg + 12, &value) || value != 0) return 0;
    return 1;
}

/* Programs three firmware-stage ring register blocks, but NEVER enables DMA.
 * Caller must have exclusive ownership, masked interrupts, initialized and
 * synchronized coherent descriptors, stopped DMA, and retained every buffer.
 * Even success does not establish an OS/device teardown fence. On any error
 * after `published` is set, buffers must not be freed until platform recovery
 * has proved that the device can no longer access them. Not wired to IOCTLs. */
static int aw_program_firmware_rings_unchecked(const aw_ring_program_io *io, aw_ring_program_state *state,
                                    uint64_t command, uint64_t download, uint64_t event) {
    uint32_t config;
    if (!io || !state || !io->read || !io->write || !io->barrier ||
        state->modified || state->published || state->ready) return 0;
    if (!aw_dma_span32(command, AW_RING_BYTES(AW_MCU_TX_COUNT), 16) ||
        !aw_dma_span32(download, AW_RING_BYTES(AW_FWDL_TX_COUNT), 16) ||
        !aw_dma_span32(event, AW_RING_BYTES(AW_EVENT_RX_COUNT), 16) ||
        !aw_disjoint32(command, AW_RING_BYTES(AW_MCU_TX_COUNT), download, AW_RING_BYTES(AW_FWDL_TX_COUNT)) ||
        !aw_disjoint32(command, AW_RING_BYTES(AW_MCU_TX_COUNT), event, AW_RING_BYTES(AW_EVENT_RX_COUNT)) ||
        !aw_disjoint32(download, AW_RING_BYTES(AW_FWDL_TX_COUNT), event, AW_RING_BYTES(AW_EVENT_RX_COUNT))) return 0;
    if (!io->read(io->context, AW_WFDMA_CONFIG, &config) || config == UINT32_MAX || (config & 15u)) return 0;
    io->barrier(io->context);
    if (!aw_program_one_ring(io, state, AW_MCU_RING_REGS, (uint32_t)command, AW_MCU_TX_COUNT, 0) ||
        !aw_program_one_ring(io, state, AW_FWDL_RING_REGS, (uint32_t)download, AW_FWDL_TX_COUNT, 0) ||
        !aw_program_one_ring(io, state, AW_EVENT_RING_REGS, (uint32_t)event, AW_EVENT_RX_COUNT, AW_EVENT_BUFFERS)) return 0;
    state->ready = 1;
    return 1;
}

/* The normal caller uses this wrapper so buffer lifetime is sealed before any
 * attempted register write. Conservative on failure: even if no address was
 * actually written, release remains blocked until the platform fences DMA. */
static inline int aw_program_mcu_memory(const aw_ring_program_io *io, aw_ring_program_state *state,
                                 aw_mcu_memory *memory) {
    if (!io || !state || !memory || !memory->prepared || memory->published ||
        state->modified || state->published || state->ready) return 0;
    if (!aw_mcu_memory_mark_published(memory)) return 0;
    return aw_program_firmware_rings_unchecked(io, state, memory->blocks[0].address,
        memory->blocks[1].address, memory->blocks[2].address);
}
