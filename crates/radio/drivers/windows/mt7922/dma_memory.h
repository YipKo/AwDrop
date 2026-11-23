/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Coherent-buffer ownership pattern adapted from Airdrop-for-Windows'
 * MT7925 data/mcu memory helpers. See ../../../LICENSE-mt7925.txt.
 */
#pragma once
#include <stdint.h>
#include "dma_rings.h"

#define AW_MCU_BLOCK_COUNT 6u
#define AW_MCU_PACKET_BYTES 4096u
#define AW_MCU_MEMORY_BYTES (AW_RING_TOTAL_BYTES + 2u * AW_MCU_PACKET_BYTES)

typedef struct aw_mcu_block {
    void *handle;
    void *virtual_address;
    uint64_t address;
    uint32_t bytes;
} aw_mcu_block;
typedef struct aw_mcu_allocator {
    void *context;
    int (*create)(void *context, uint32_t bytes, aw_mcu_block *out);
    void (*release)(void *context, void *handle);
} aw_mcu_allocator;
typedef struct aw_mcu_memory {
    aw_mcu_block blocks[AW_MCU_BLOCK_COUNT];
    uint32_t attempted, prepared, published;
} aw_mcu_memory;

/* A platform fence must prove that hardware cannot access any published
 * allocation. Merely observing an idle ring is not such a proof. */
static inline int aw_mcu_memory_release(aw_mcu_memory *memory, const aw_mcu_allocator *allocator, int fenced) {
    uint32_t i;
    if (!memory || !allocator || !allocator->release || (memory->published && !fenced)) return 0;
    for (i = AW_MCU_BLOCK_COUNT; i > 0; --i) {
        aw_mcu_block *block = &memory->blocks[i - 1];
        if (block->handle) allocator->release(allocator->context, block->handle);
        block->handle = 0; block->virtual_address = 0; block->address = 0; block->bytes = 0;
    }
    memory->attempted = memory->prepared = memory->published = 0;
    return 1;
}

static inline int aw_mcu_memory_prepare(aw_mcu_memory *memory, const aw_mcu_allocator *allocator) {
    const uint32_t lengths[AW_MCU_BLOCK_COUNT] = {AW_RING_BYTES(AW_MCU_TX_COUNT),
        AW_RING_BYTES(AW_FWDL_TX_COUNT), AW_RING_BYTES(AW_EVENT_RX_COUNT),
        AW_EVENT_PAYLOAD_BYTES, AW_MCU_PACKET_BYTES, AW_MCU_PACKET_BYTES};
    uint32_t i, j;
    if (!memory || !allocator || !allocator->create || !allocator->release || memory->attempted ||
        memory->prepared || memory->published) return 0;
    for (i = 0; i < AW_MCU_BLOCK_COUNT; ++i) if (memory->blocks[i].handle) return 0;
    memory->attempted = 1;
    for (i = 0; i < AW_MCU_BLOCK_COUNT; ++i) {
        aw_mcu_block *block = &memory->blocks[i];
        uintptr_t va;
        if (!allocator->create(allocator->context, lengths[i], block)) goto fail;
        va = (uintptr_t)block->virtual_address;
        if (!block->handle || !va || block->bytes != lengths[i] || (va & 15u) ||
            va > UINTPTR_MAX - lengths[i] || !aw_dma_span32(block->address, block->bytes, 16)) goto fail;
        for (j = 0; j < i; ++j) {
            aw_mcu_block *previous = &memory->blocks[j];
            uintptr_t old = (uintptr_t)previous->virtual_address;
            if (!(block->address + block->bytes <= previous->address || previous->address + previous->bytes <= block->address) ||
                !(va + block->bytes <= old || old + previous->bytes <= va)) goto fail;
        }
    }
    memory->prepared = 1;
    return 1;
fail:
    (void)aw_mcu_memory_release(memory, allocator, 0);
    return 0;
}

/* Revalidate the immutable block table immediately before publication. */
static inline int aw_mcu_memory_valid(const aw_mcu_memory *memory) {
    const uint32_t lengths[AW_MCU_BLOCK_COUNT] = {AW_RING_BYTES(AW_MCU_TX_COUNT),
        AW_RING_BYTES(AW_FWDL_TX_COUNT), AW_RING_BYTES(AW_EVENT_RX_COUNT),
        AW_EVENT_PAYLOAD_BYTES, AW_MCU_PACKET_BYTES, AW_MCU_PACKET_BYTES};
    uint32_t i, j;
    if (!memory || !memory->attempted || !memory->prepared) return 0;
    for (i = 0; i < AW_MCU_BLOCK_COUNT; ++i) {
        const aw_mcu_block *block = &memory->blocks[i];
        uintptr_t va = (uintptr_t)block->virtual_address;
        if (!block->handle || !va || block->bytes != lengths[i] || (va & 15u) ||
            va > UINTPTR_MAX - lengths[i] || !aw_dma_span32(block->address, block->bytes, 16)) return 0;
        for (j = 0; j < i; ++j) {
            const aw_mcu_block *previous = &memory->blocks[j];
            uintptr_t old = (uintptr_t)previous->virtual_address;
            if (!(block->address + block->bytes <= previous->address || previous->address + previous->bytes <= block->address) ||
                !(va + block->bytes <= old || old + previous->bytes <= va)) return 0;
        }
    }
    return 1;
}

/* Must run before the FIRST attempt to publish any buffer address. */
static inline int aw_mcu_memory_mark_published(aw_mcu_memory *memory) {
    if (!memory || memory->published || !aw_mcu_memory_valid(memory)) return 0;
    memory->published = 1;
    return 1;
}
