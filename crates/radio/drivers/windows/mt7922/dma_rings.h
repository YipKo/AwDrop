/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * Connac2 descriptor layout based on mt76/dma.h and mt76/dma.c.
 * Copyright (C) 2016 Felix Fietkau <nbd@nbd.name>
 * See ../../../LICENSE-mt7922-rx.txt.
 * This is a diagnostic-only image; it must not be submitted to hardware.
 */
#pragma once
#include <stdint.h>
#include "dma_bounds.h"

#define AW_MCU_TX_COUNT 256u
#define AW_FWDL_TX_COUNT 128u
#define AW_EVENT_RX_COUNT 8u
#define AW_EVENT_BUFFERS (AW_EVENT_RX_COUNT - 1u)
#define AW_EVENT_BUFFER_SIZE 2048u
#define AW_DESC_SIZE 16u
#define AW_RING_BYTES(n) ((n) * AW_DESC_SIZE)
#define AW_EVENT_PAYLOAD_BYTES (AW_EVENT_BUFFERS * AW_EVENT_BUFFER_SIZE)
#define AW_RING_TOTAL_BYTES (AW_RING_BYTES(AW_MCU_TX_COUNT) + AW_RING_BYTES(AW_FWDL_TX_COUNT) + AW_RING_BYTES(AW_EVENT_RX_COUNT) + AW_EVENT_PAYLOAD_BYTES)

typedef struct aw_dma_descriptor {
    uint32_t buffer0;
    uint32_t control;
    uint32_t buffer1;
    uint32_t info;
} aw_dma_descriptor;

/* Diagnostic construction uses little-endian Windows x64 only. */
static inline int aw_ring_initialize(aw_dma_descriptor *command, aw_dma_descriptor *download,
                              aw_dma_descriptor *event, uint64_t event_buffers) {
    uint32_t i;
    if (!command || !download || !event ||
        !aw_dma_span32(event_buffers, AW_EVENT_PAYLOAD_BYTES, 16u)) return 0;
    for (i = 0; i < AW_MCU_TX_COUNT; ++i)
        command[i] = (aw_dma_descriptor){0, 0x80000000u, 0, 0};
    for (i = 0; i < AW_FWDL_TX_COUNT; ++i)
        download[i] = (aw_dma_descriptor){0, 0x80000000u, 0, 0};
    for (i = 0; i < AW_EVENT_RX_COUNT; ++i)
        event[i] = (aw_dma_descriptor){0, 0x80000000u, 0, 0};
    for (i = 0; i < AW_EVENT_BUFFERS; ++i) {
        event[i].buffer0 = (uint32_t)(event_buffers + i * AW_EVENT_BUFFER_SIZE);
        event[i].control = AW_EVENT_BUFFER_SIZE << 16;
    }
    return 1;
}
