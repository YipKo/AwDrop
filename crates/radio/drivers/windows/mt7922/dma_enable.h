/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2023 MediaTek Inc.
 * MT7922 prefetch and WFDMA setup derived from mt76/mt792x_dma.c and
 * mt792x_regs.h. See ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include "dma_program.h"

#define AW_DMA_SETUP_BITS 0x5020b870u
#define AW_DMA_ENABLE_BITS 5u
#define AW_DMA_ADDR_EXT 0x04000000u
#define AW_DMA_OMIT_RX_INFO 0x08000000u

typedef struct aw_dma_enable_state {
    uint32_t attempted, enable_attempted, ready;
} aw_dma_enable_state;

static inline int aw_register_equal(const aw_ring_program_io *io, uint32_t address, uint32_t expected) {
    uint32_t value;
    return io->read(io->context, address, &value) && value == expected;
}

/* This is a hardware operation, not currently exposed by the diagnostic
 * driver. The caller must already have exclusive ownership, a safely reset
 * device, disabled interrupts and DMA, and coherent, sealed ring buffers.
 * A failed write may have enabled DMA: on ANY error the caller must retain all
 * published allocations and perform a verified device/OS recovery fence. */
static inline int aw_enable_firmware_dma(const aw_ring_program_io *io, const aw_mcu_memory *memory,
                                         const aw_ring_program_state *rings, aw_dma_enable_state *state) {
    static const uint32_t prefetch[][2] = {
        {0xd4680u,0x00000004u}, {0xd4688u,0x00400004u}, {0xd468cu,0x00800004u},
        {0xd4690u,0x00c00004u}, {0xd4694u,0x01000004u},
        {0xd4600u,0x01400004u}, {0xd4604u,0x01800004u}, {0xd4608u,0x01c00004u},
        {0xd460cu,0x02000004u}, {0xd4610u,0x02400004u}, {0xd4614u,0x02800004u},
        {0xd4618u,0x02c00004u}, {0xd4640u,0x03400004u}, {0xd4644u,0x03800004u}
    };
    uint32_t cfg, i;
    if (!io || !memory || !rings || !state || !io->read || !io->write || !io->barrier ||
        state->attempted || !memory->published || !aw_mcu_memory_valid(memory) || !rings->ready || !rings->published) return 0;
    if (!io->read(io->context, AW_WFDMA_CONFIG, &cfg) || cfg == UINT32_MAX || (cfg & 15u) ||
        !aw_register_equal(io, 0xd4204u, 0) || !aw_register_equal(io, 0x10188u, 0) ||
        !aw_register_equal(io, AW_MCU_RING_REGS, (uint32_t)memory->blocks[0].address) ||
        !aw_register_equal(io, AW_MCU_RING_REGS + 4, AW_MCU_TX_COUNT) ||
        !aw_register_equal(io, AW_FWDL_RING_REGS, (uint32_t)memory->blocks[1].address) ||
        !aw_register_equal(io, AW_FWDL_RING_REGS + 4, AW_FWDL_TX_COUNT) ||
        !aw_register_equal(io, AW_EVENT_RING_REGS, (uint32_t)memory->blocks[2].address) ||
        !aw_register_equal(io, AW_EVENT_RING_REGS + 4, AW_EVENT_RX_COUNT) ||
        !aw_register_equal(io, AW_EVENT_RING_REGS + 8, AW_EVENT_BUFFERS)) return 0;
    state->attempted = 1;
    for (i = 0; i < sizeof(prefetch) / sizeof(prefetch[0]); ++i)
        if (!io->write(io->context, prefetch[i][0], prefetch[i][1])) return 0;
    if (!io->write(io->context, 0xd420cu, UINT32_MAX) || !io->write(io->context, 0xd42f0u, 0)) return 0;
    if (!io->read(io->context, AW_WFDMA_CONFIG, &cfg) || cfg == UINT32_MAX || (cfg & 15u)) return 0;
    cfg = (cfg & ~(AW_DMA_ADDR_EXT | AW_DMA_OMIT_RX_INFO)) | AW_DMA_SETUP_BITS;
    if (!io->write(io->context, AW_WFDMA_CONFIG, cfg)) return 0;
    io->barrier(io->context);
    state->enable_attempted = 1; /* Set before a write that might still land on error. */
    if (!io->write(io->context, AW_WFDMA_CONFIG, cfg | AW_DMA_ENABLE_BITS)) return 0;
    if (!io->read(io->context, AW_WFDMA_CONFIG, &cfg) || cfg == UINT32_MAX ||
        (cfg & (AW_DMA_SETUP_BITS | AW_DMA_ENABLE_BITS)) != (AW_DMA_SETUP_BITS | AW_DMA_ENABLE_BITS) ||
        (cfg & (AW_DMA_ADDR_EXT | AW_DMA_OMIT_RX_INFO))) return 0;
    /* mt792x_dma_enable marks WFDMA initialization in the MCU dummy CSR. */
    if (!io->read(io->context, 0x2120u, &cfg) || cfg == UINT32_MAX ||
        !io->write(io->context, 0x2120u, cfg | 2u) ||
        !io->read(io->context, 0x2120u, &cfg) || cfg == UINT32_MAX || !(cfg & 2u)) return 0;
    state->ready = 1;
    return 1;
}
