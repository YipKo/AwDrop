/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2016 Felix Fietkau <nbd@nbd.name>
 * Connac2 descriptor fields and DMA-index reclamation follow mt76/dma.c.
 * See ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include "dma_rings.h"

/* This helper does not synchronize memory, ring doorbells, or release payload
 * buffers. A real caller must serialize it, use WDF common buffers, supply
 * DMA barriers, stop hardware before teardown, and reclaim payloads itself. */
typedef struct aw_tx_queue {
    aw_dma_descriptor *descriptors;
    uint32_t count, head, tail, queued;
} aw_tx_queue;

static int aw_tx_queue_init(aw_tx_queue *queue, aw_dma_descriptor *descriptors, uint32_t count) {
    if (!queue || !descriptors || count < 4 || count > 4096) return 0;
    queue->descriptors = descriptors;
    queue->count = count;
    queue->head = queue->tail = queue->queued = 0;
    return 1;
}

/* Submit a single contiguous 32-bit DMA payload; no MMIO is performed. */
static int aw_tx_queue_submit(aw_tx_queue *queue, uint64_t address, uint32_t length, uint32_t *index) {
    aw_dma_descriptor *descriptor;
    if (!queue || !queue->descriptors || !index || !length || length > 0x3fffu ||
        !aw_dma_span32(address, length, 1) || queue->queued >= queue->count - 2) return 0;
    descriptor = &queue->descriptors[queue->head];
    if (!(*(volatile uint32_t *)&descriptor->control & 0x80000000u)) return 0;
    *index = queue->head;
    descriptor->buffer0 = (uint32_t)address;
    descriptor->buffer1 = 0;
    descriptor->info = 0;
    *(volatile uint32_t *)&descriptor->control = (length << 16) | 0x40000000u;
    queue->head = (queue->head + 1) % queue->count;
    ++queue->queued;
    return 1;
}

/* 1: one descriptor completed, 0: none, -1: invalid DMA index/state. A
 * caller may reclaim its corresponding buffer only on a return value of 1. */
static int aw_tx_queue_reclaim(aw_tx_queue *queue, uint32_t dma_index, uint32_t *index) {
    uint32_t completed;
    if (!queue || !queue->descriptors || !index || dma_index >= queue->count) return -1;
    completed = (dma_index + queue->count - queue->tail) % queue->count;
    if (completed > queue->queued) return -1;
    if (!completed) return 0;
    *index = queue->tail;
    *(volatile uint32_t *)&queue->descriptors[queue->tail].control = 0x80000000u;
    queue->tail = (queue->tail + 1) % queue->count;
    --queue->queued;
    return 1;
}
