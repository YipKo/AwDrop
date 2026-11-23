/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Copyright (C) 2016 Felix Fietkau <nbd@nbd.name>
 * RX descriptor completion and length fields follow mt76/dma.c and dma.h.
 * See ../../../LICENSE-mt7922-rx.txt.
 */
#pragma once
#include "dma_rings.h"

/* No hardware synchronization is performed here. Before inspecting a DMA
 * completion, the device owner must use the platform's DMA ordering rules;
 * before publishing a refill it must issue a write barrier and ring the
 * device doorbell. These helpers are currently exercised only in simulation. */
typedef struct aw_event_queue {
    aw_dma_descriptor *descriptors;
    uint32_t payload_base, head, tail, posted, borrowed;
    uint8_t slot_to_buffer[AW_EVENT_RX_COUNT];
} aw_event_queue;

static int aw_event_queue_init(aw_event_queue *queue, aw_dma_descriptor *descriptors, uint64_t payload_base) {
    uint32_t i;
    if (!queue || !descriptors || !aw_dma_span32(payload_base, AW_EVENT_PAYLOAD_BYTES, 16)) return 0;
    queue->descriptors = descriptors;
    queue->payload_base = (uint32_t)payload_base;
    queue->head = AW_EVENT_BUFFERS;
    queue->tail = 0;
    queue->posted = AW_EVENT_BUFFERS;
    queue->borrowed = 0;
    for (i = 0; i < AW_EVENT_BUFFERS; ++i) queue->slot_to_buffer[i] = (uint8_t)i;
    queue->slot_to_buffer[AW_EVENT_BUFFERS] = 0xff;
    return 1;
}

/* Returns 1 for a complete single-buffer event, 0 when no event is ready,
 * and -1 for a malformed/unsupported completion requiring recovery. The
 * buffer remains borrowed until aw_event_queue_repost succeeds. */
static int aw_event_queue_take(aw_event_queue *queue, uint32_t *buffer, uint32_t *length) {
    uint32_t control, id, bytes;
    if (!queue || !queue->descriptors || !buffer || !length) return -1;
    if (!queue->posted) return 0;
    /* Device writes this word asynchronously. The owner still supplies the
     * required DMA acquire barrier before reading the payload. */
    control = *(volatile uint32_t *)&queue->descriptors[queue->tail].control;
    if (!(control & 0x80000000u)) return 0;
    bytes = (control >> 16) & 0x3fffu;
    id = queue->slot_to_buffer[queue->tail];
    /* Reject fragmented, malformed, dropped, or WED-versioned events. */
    if (!(control & 0x40000000u) || !bytes || bytes > AW_EVENT_BUFFER_SIZE ||
        (control & ((1u << 14) | (1u << 12) | (1u << 7))) ||
        id >= AW_EVENT_BUFFERS || (queue->borrowed & (1u << id))) return -1;
    *buffer = id;
    *length = bytes;
    queue->borrowed |= 1u << id;
    queue->slot_to_buffer[queue->tail] = 0xff;
    queue->tail = (queue->tail + 1) % AW_EVENT_RX_COUNT;
    --queue->posted;
    return 1;
}

static int aw_event_queue_repost(aw_event_queue *queue, uint32_t buffer) {
    aw_dma_descriptor *desc;
    if (!queue || !queue->descriptors || buffer >= AW_EVENT_BUFFERS ||
        !(queue->borrowed & (1u << buffer)) || queue->posted >= AW_EVENT_BUFFERS ||
        queue->slot_to_buffer[queue->head] != 0xff) return 0;
    desc = &queue->descriptors[queue->head];
    desc->buffer0 = queue->payload_base + buffer * AW_EVENT_BUFFER_SIZE;
    desc->buffer1 = 0;
    desc->info = 0;
    *(volatile uint32_t *)&desc->control = AW_EVENT_BUFFER_SIZE << 16;
    queue->slot_to_buffer[queue->head] = (uint8_t)buffer;
    queue->borrowed &= ~(1u << buffer);
    queue->head = (queue->head + 1) % AW_EVENT_RX_COUNT;
    ++queue->posted;
    return 1;
}
