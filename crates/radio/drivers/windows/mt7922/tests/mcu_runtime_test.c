#include "../mcu_runtime.h"
#include <assert.h>

_Alignas(16) static uint8_t storage[AW_MCU_BLOCK_COUNT][16384];
typedef struct fake { uint32_t regs[256]; uint64_t time; unsigned writes, barriers; int complete, fail_write; } fake;
static uint32_t *reg(fake *f, uint32_t offset) {
    assert(offset >= 0xd4300u && offset < 0xd4700u && !(offset & 3));
    return &f->regs[(offset - 0xd4300u) / 4];
}
static int read_reg(void *context, uint32_t offset, uint32_t *value) { *value = *reg(context, offset); return 1; }
static int write_reg(void *context, uint32_t offset, uint32_t value) {
    fake *f = context;
    ++f->writes;
    *reg(f, offset) = value;
    if (f->complete && (offset == AW_MCU_RING_REGS + 8 || offset == AW_FWDL_RING_REGS + 8))
        *reg(f, offset + 4) = value;
    return !f->fail_write;
}
static void barrier(void *context) { ++((fake *)context)->barriers; }
static uint64_t milliseconds(void *context) { return ((fake *)context)->time; }
static int pause_one(void *context) { ((fake *)context)->time += 1000; return 1; }
static void setup(fake *f, aw_mcu_memory *memory, aw_mcu_runtime *runtime) {
    const uint32_t sizes[AW_MCU_BLOCK_COUNT] = {4096, 2048, 128, 14336, 4096, 4096};
    aw_dma_enable_state dma = {1,1,1};
    aw_mcu_runtime_io io = {f, read_reg, write_reg, barrier, milliseconds, pause_one};
    uint32_t i;
    *memory = (aw_mcu_memory){0}; *runtime = (aw_mcu_runtime){0};
    memory->attempted = memory->prepared = memory->published = 1;
    for (i = 0; i < AW_MCU_BLOCK_COUNT; ++i) {
        memory->blocks[i].handle = storage[i]; memory->blocks[i].virtual_address = storage[i];
        memory->blocks[i].address = 0x10000u + (uint64_t)i * 0x10000u;
        memory->blocks[i].bytes = sizes[i];
    }
    assert(aw_ring_initialize((aw_dma_descriptor *)storage[0], (aw_dma_descriptor *)storage[1],
                              (aw_dma_descriptor *)storage[2], memory->blocks[3].address));
    assert(aw_mcu_runtime_init(runtime, memory, &dma, &io) == AW_MCU_OK);
}
int main(void) {
    fake f = {.complete = 1}; aw_mcu_memory memory; aw_mcu_runtime runtime;
    uint8_t data[] = {1,2,3,4}, output[16]; uint32_t length;
    aw_dma_descriptor *events = (aw_dma_descriptor *)storage[2];
    setup(&f, &memory, &runtime);
    assert(aw_mcu_runtime_transmit(&runtime, 0, data, sizeof(data)) == AW_MCU_OK);
    assert(storage[4][0] == 1 && storage[4][3] == 4 && runtime.command.queued == 0);
    assert(aw_mcu_runtime_transmit(&runtime, 1, data, sizeof(data)) == AW_MCU_OK);
    assert(storage[5][0] == 1 && runtime.download.queued == 0);
    assert(aw_mcu_runtime_receive(&runtime, output, sizeof(output), &length) == AW_MCU_PENDING);
    storage[3][0] = 0xaa; storage[3][1] = 0xbb;
    events[0].control = 0xc0000000u | (2u << 16);
    assert(aw_mcu_runtime_receive(&runtime, output, sizeof(output), &length) == AW_MCU_OK);
    assert(length == 2 && output[0] == 0xaa && output[1] == 0xbb && *reg(&f, AW_EVENT_RING_REGS + 8) == 0);

    f = (fake){0}; setup(&f, &memory, &runtime);
    *reg(&f, AW_MCU_RING_REGS + 12) = AW_MCU_TX_COUNT;
    assert(aw_mcu_runtime_transmit(&runtime, 0, data, sizeof(data)) == AW_MCU_ERROR);
    assert(runtime.faulted && memory.published && runtime.command.queued == 1);
    assert(aw_mcu_runtime_transmit(&runtime, 0, data, sizeof(data)) == AW_MCU_ERROR);

    f = (fake){0}; setup(&f, &memory, &runtime);
    assert(aw_mcu_runtime_transmit(&runtime, 0, data, sizeof(data)) == AW_MCU_ERROR);
    assert(runtime.faulted && f.time >= AW_MCU_TIMEOUT_MS && memory.published);

    f = (fake){.complete = 1}; setup(&f, &memory, &runtime);
    events[0].control = 0xc0000000u | (17u << 16);
    assert(aw_mcu_runtime_receive(&runtime, output, sizeof(output), &length) == AW_MCU_ERROR);
    assert(runtime.faulted && runtime.events.borrowed == 1 && memory.published);
    return 0;
}
