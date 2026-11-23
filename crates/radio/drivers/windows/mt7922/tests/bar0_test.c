#include "../bar0.h"
#include "../dma_bounds.h"
#include "../dma_rings.h"
#include "../dma_tx_queue.h"
#include "../dma_event_queue.h"
#include <assert.h>

int main(void) {
    uint64_t address = 0;
    aw_dma_descriptor command[AW_MCU_TX_COUNT] = {{0}};
    aw_dma_descriptor download[AW_FWDL_TX_COUNT] = {{0}};
    aw_dma_descriptor event[AW_EVENT_RX_COUNT] = {{0}};
    aw_tx_queue queue;
    uint32_t index;
    uint32_t event_buffer, event_length;
    aw_event_queue events;
    assert(aw_bar0_address(0x12345000u, 0xffffffffu, &address));
    assert(address == 0x12345000u);
    assert(aw_bar0_address(0x23456004u, 1u, &address));
    assert(address == UINT64_C(0x123456000));
    assert(!aw_bar0_address(0x12345001u, 0, &address));
    assert(!aw_bar0_address(0x12345002u, 0, &address));
    assert(!aw_bar0_address(0x12345006u, 0, &address));
    assert(!aw_bar0_address(0, 0, &address));
    assert(!aw_bar0_address(UINT32_MAX, UINT32_MAX, &address));
    assert(!aw_bar0_address(0x1000, 0, 0));
    assert(aw_bar0_resource_matches(0x12345000, 0x12345000, 0x100000, 0x100000));
    assert(!aw_bar0_resource_matches(0x12345000, 0x12346000, 0x100000, 0x100000));
    assert(!aw_bar0_resource_matches(0x12345000, 0x12345000, 0, 0));
    assert(!aw_bar0_resource_matches(0x12345000, 0x12345000, 4096, 8192));
    assert(!aw_bar0_resource_matches(UINT64_MAX - 1, UINT64_MAX - 1, 4, 4));
    assert(aw_dma_span32(0x2000, 6272, 16));
    assert(!aw_dma_span32(0x2001, 6272, 16));
    assert(!aw_dma_span32(UINT32_MAX - 15, 32, 16));
    assert(!aw_dma_span32(UINT64_C(0x100000000), 16, 16));
    assert(!aw_dma_span32(0, 0, 16));
    assert(!aw_dma_span32(0, 16, 3));
    assert(sizeof(aw_dma_descriptor) == AW_DESC_SIZE);
    assert(AW_RING_TOTAL_BYTES == 20608);
    assert(!aw_ring_initialize(command, download, event, UINT64_C(0xffffe000)));
    assert(command[0].control == 0);
    assert(aw_ring_initialize(command, download, event, 0x10000));
    assert(command[0].control == 0x80000000u && command[255].control == 0x80000000u);
    assert(download[0].control == 0x80000000u && download[127].control == 0x80000000u);
    assert(event[0].buffer0 == 0x10000 && event[0].control == (2048u << 16));
    assert(event[6].buffer0 == 0x13000 && event[6].control == (2048u << 16));
    assert(event[7].control == 0x80000000u && event[7].buffer0 == 0);
    assert(!aw_tx_queue_init(&queue, command, 3));
    assert(aw_tx_queue_init(&queue, command, 4));
    assert(!aw_tx_queue_submit(&queue, UINT32_MAX, 2, &index));
    assert(aw_tx_queue_submit(&queue, 0x1000, 32, &index) && index == 0);
    assert(command[0].buffer0 == 0x1000 && command[0].control == (0x40000000u | (32u << 16)));
    assert(aw_tx_queue_submit(&queue, 0x2000, 64, &index) && index == 1);
    assert(!aw_tx_queue_submit(&queue, 0x3000, 64, &index));
    assert(aw_tx_queue_reclaim(&queue, 3, &index) == -1 && queue.queued == 2);
    assert(aw_tx_queue_reclaim(&queue, 2, &index) == 1 && index == 0);
    assert(aw_tx_queue_reclaim(&queue, 2, &index) == 1 && index == 1);
    assert(aw_tx_queue_submit(&queue, 0x3000, 64, &index) && index == 2);
    assert(aw_tx_queue_submit(&queue, 0x4000, 64, &index) && index == 3);
    assert(aw_tx_queue_reclaim(&queue, 0, &index) == 1 && index == 2);
    assert(aw_tx_queue_reclaim(&queue, 0, &index) == 1 && index == 3);
    assert(aw_tx_queue_reclaim(&queue, 0, &index) == 0 && queue.queued == 0);
    assert(aw_event_queue_init(&events, event, 0x10000));
    assert(aw_event_queue_take(&events, &event_buffer, &event_length) == 0);
    event[0].control = 0x80000000u | 0x40000000u | (36u << 16);
    assert(aw_event_queue_take(&events, &event_buffer, &event_length) == 1);
    assert(event_buffer == 0 && event_length == 36 && events.posted == 6);
    assert(aw_event_queue_repost(&events, event_buffer));
    assert(event[7].buffer0 == 0x10000 && events.head == 0 && events.posted == 7);
    assert(!aw_event_queue_repost(&events, event_buffer));
    event[1].control = 0x80000000u | 0x40000000u | (2049u << 16);
    assert(aw_event_queue_take(&events, &event_buffer, &event_length) == -1);
    assert(events.tail == 1 && events.posted == 7);
    event[1].control = 0x80000000u | (36u << 16);
    assert(aw_event_queue_take(&events, &event_buffer, &event_length) == -1);
    event[1].control = 0x80000000u | 0x40000000u | (1u << 14) | (36u << 16);
    assert(aw_event_queue_take(&events, &event_buffer, &event_length) == -1);
    /* All seven buffers may be borrowed; an empty ring has no completion.
     * Reposting must not permit a duplicate physical buffer. */
    assert(aw_ring_initialize(command, download, event, 0x10000));
    assert(aw_event_queue_init(&events, event, 0x10000));
    for (index = 0; index < AW_EVENT_BUFFERS; ++index) {
        event[index].control = 0xc0000000u | ((index + 1) << 16);
        assert(aw_event_queue_take(&events, &event_buffer, &event_length) == 1);
        assert(event_buffer == index && event_length == index + 1);
    }
    assert(aw_event_queue_take(&events, &event_buffer, &event_length) == 0);
    for (index = AW_EVENT_BUFFERS; index > 0; --index)
        assert(aw_event_queue_repost(&events, index - 1));
    assert(events.posted == AW_EVENT_BUFFERS && events.borrowed == 0);
    assert(!aw_event_queue_repost(&events, 0));
    return 0;
}
