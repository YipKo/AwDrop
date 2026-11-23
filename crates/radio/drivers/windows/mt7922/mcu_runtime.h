/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2016 Felix Fietkau <nbd@nbd.name>
 * Copyright (C) 2020, 2023 MediaTek Inc.
 * Connac2 PCIe queues follow mt76/dma.c, mt7921/pci_mcu.c, and the
 * Airdrop-for-Windows PCIe MCU transport. See ../../../LICENSE-mt7922-rx.txt
 * and ../../../LICENSE-mt7925.txt.
 */
#pragma once
#include "dma_enable.h"
#include "dma_tx_queue.h"
#include "dma_event_queue.h"

#define AW_MCU_OK 0
#define AW_MCU_PENDING 1
#define AW_MCU_ERROR (-1)
#define AW_MCU_TIMEOUT_MS 3000u
#define AW_MCU_MAX_POLLS 10000u

typedef struct aw_mcu_runtime_io {
    void *context;
    int (*read)(void *, uint32_t, uint32_t *);
    int (*write)(void *, uint32_t, uint32_t);
    void (*barrier)(void *);
    uint64_t (*milliseconds)(void *);
    int (*pause)(void *);
} aw_mcu_runtime_io;
typedef struct aw_mcu_runtime {
    aw_mcu_memory *memory;
    aw_mcu_runtime_io io;
    aw_tx_queue command, download;
    aw_event_queue events;
    uint32_t initialized, faulted;
} aw_mcu_runtime;

/* Caller owns all memory, serializes every call, enforces device power and
 * removal state, and supplies coherent WDF buffers with platform DMA barriers.
 * No teardown or fencing occurs here. This runtime is not yet exposed by the
 * diagnostic driver and must not be used without a validated hardware owner. */
static inline int aw_mcu_runtime_init(aw_mcu_runtime *runtime, aw_mcu_memory *memory,
                                      const aw_dma_enable_state *dma, const aw_mcu_runtime_io *io) {
    if (!runtime || !memory || !dma || !io || !io->read || !io->write || !io->barrier ||
        !io->milliseconds || !io->pause || runtime->initialized || !memory->published ||
        !aw_mcu_memory_valid(memory) || !dma->ready) return AW_MCU_ERROR;
    runtime->memory = memory;
    runtime->io = *io;
    if (!aw_tx_queue_init(&runtime->command, (aw_dma_descriptor *)memory->blocks[0].virtual_address, AW_MCU_TX_COUNT) ||
        !aw_tx_queue_init(&runtime->download, (aw_dma_descriptor *)memory->blocks[1].virtual_address, AW_FWDL_TX_COUNT) ||
        !aw_event_queue_init(&runtime->events, (aw_dma_descriptor *)memory->blocks[2].virtual_address, memory->blocks[3].address))
        return AW_MCU_ERROR;
    runtime->initialized = 1;
    return AW_MCU_OK;
}

/* One outstanding packet per queue; callers may reuse its payload buffer only
 * after the hardware DMA index advances. Firmware scatter uses queue 16 and
 * commands use queue 17. Submission failure poisons the entire runtime. */
static inline int aw_mcu_runtime_transmit(aw_mcu_runtime *runtime, int firmware, const uint8_t *data, uint32_t bytes) {
    aw_tx_queue *queue;
    aw_mcu_block *buffer;
    uint32_t reg, index, dma_index, i, polls;
    int completed;
    uint64_t start;
    if (!runtime || !runtime->initialized || runtime->faulted || !data || !bytes || bytes > AW_MCU_PACKET_BYTES ||
        (firmware != 0 && firmware != 1)) return AW_MCU_ERROR;
    queue = firmware ? &runtime->download : &runtime->command;
    buffer = &runtime->memory->blocks[firmware ? 5 : 4];
    reg = firmware ? AW_FWDL_RING_REGS : AW_MCU_RING_REGS;
    if (queue->queued) { runtime->faulted = 1; return AW_MCU_ERROR; }
    for (i = 0; i < bytes; ++i) ((uint8_t *)buffer->virtual_address)[i] = data[i];
    if (!aw_tx_queue_submit(queue, buffer->address, bytes, &index)) { runtime->faulted = 1; return AW_MCU_ERROR; }
    runtime->io.barrier(runtime->io.context);
    if (!runtime->io.write(runtime->io.context, reg + 8, queue->head)) { runtime->faulted = 1; return AW_MCU_ERROR; }
    start = runtime->io.milliseconds(runtime->io.context);
    for (polls = 0; polls < AW_MCU_MAX_POLLS; ++polls) {
        if (!runtime->io.read(runtime->io.context, reg + 12, &dma_index)) break;
        runtime->io.barrier(runtime->io.context);
        completed = aw_tx_queue_reclaim(queue, dma_index, &index);
        if (completed == 1) return AW_MCU_OK;
        if (completed != 0 || runtime->io.milliseconds(runtime->io.context) - start >= AW_MCU_TIMEOUT_MS ||
            !runtime->io.pause(runtime->io.context)) break;
    }
    runtime->faulted = 1;
    return AW_MCU_ERROR;
}

/* One complete event or PENDING. A malformed or oversized event poisons the
 * runtime; its buffer remains held until device recovery. A successful return
 * means the payload was copied and its RX descriptor was reposted. */
static inline int aw_mcu_runtime_receive(aw_mcu_runtime *runtime, uint8_t *output, uint32_t capacity, uint32_t *length) {
    uint32_t buffer, bytes, i;
    int result;
    if (!runtime || !runtime->initialized || runtime->faulted || !output || !length) return AW_MCU_ERROR;
    *length = 0;
    runtime->io.barrier(runtime->io.context);
    result = aw_event_queue_take(&runtime->events, &buffer, &bytes);
    if (result == 0) return AW_MCU_PENDING;
    if (result < 0 || bytes > capacity) { runtime->faulted = 1; return AW_MCU_ERROR; }
    runtime->io.barrier(runtime->io.context);
    for (i = 0; i < bytes; ++i)
        output[i] = ((const uint8_t *)runtime->memory->blocks[3].virtual_address)[buffer * AW_EVENT_BUFFER_SIZE + i];
    if (!aw_event_queue_repost(&runtime->events, buffer)) { runtime->faulted = 1; return AW_MCU_ERROR; }
    runtime->io.barrier(runtime->io.context);
    if (!runtime->io.write(runtime->io.context, AW_EVENT_RING_REGS + 8, runtime->events.head)) {
        runtime->faulted = 1;
        return AW_MCU_ERROR;
    }
    *length = bytes;
    return AW_MCU_OK;
}
