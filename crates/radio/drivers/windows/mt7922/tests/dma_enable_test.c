#include "../dma_enable.h"
#include <assert.h>

_Alignas(16) static uint8_t storage[AW_MCU_BLOCK_COUNT][16384];
typedef struct fake { uint32_t regs[512], pcie_mask, mcu_dummy; unsigned writes, barriers, fail_write; } fake;
static uint32_t *reg(fake *f, uint32_t offset) {
    if (offset == 0x10188u) return &f->pcie_mask;
    if (offset == 0x2120u) return &f->mcu_dummy;
    assert(offset >= 0xd4000u && offset < 0xd4800u && !(offset & 3));
    return &f->regs[(offset - 0xd4000u) / 4];
}
static int read_reg(void *context, uint32_t offset, uint32_t *value) { *value = *reg(context, offset); return 1; }
static int write_reg(void *context, uint32_t offset, uint32_t value) {
    fake *f = context;
    ++f->writes;
    *reg(f, offset) = value; /* Failure may be reported after the write landed. */
    return f->writes != f->fail_write;
}
static void barrier(void *context) { ++((fake *)context)->barriers; }
static void prepare(fake *f, aw_mcu_memory *memory) {
    const uint32_t sizes[AW_MCU_BLOCK_COUNT] = {4096, 2048, 128, 14336, 4096, 4096};
    uint32_t i;
    *memory = (aw_mcu_memory){0};
    memory->attempted = memory->prepared = memory->published = 1;
    for (i = 0; i < AW_MCU_BLOCK_COUNT; ++i) {
        memory->blocks[i].handle = storage[i]; memory->blocks[i].virtual_address = storage[i];
        memory->blocks[i].address = 0x10000u + (uint64_t)i * 0x10000u;
        memory->blocks[i].bytes = sizes[i];
    }
    *reg(f, AW_MCU_RING_REGS) = 0x10000; *reg(f, AW_MCU_RING_REGS + 4) = 256;
    *reg(f, AW_FWDL_RING_REGS) = 0x20000; *reg(f, AW_FWDL_RING_REGS + 4) = 128;
    *reg(f, AW_EVENT_RING_REGS) = 0x30000; *reg(f, AW_EVENT_RING_REGS + 4) = 8;
    *reg(f, AW_EVENT_RING_REGS + 8) = 7;
}
int main(void) {
    fake f = {0}; aw_mcu_memory memory; aw_ring_program_state rings = {1,1,1};
    aw_dma_enable_state state = {0}; aw_ring_program_io io = {&f, read_reg, write_reg, barrier};
    prepare(&f, &memory);
    *reg(&f, AW_WFDMA_CONFIG) = AW_DMA_ADDR_EXT | AW_DMA_OMIT_RX_INFO;
    assert(aw_enable_firmware_dma(&io, &memory, &rings, &state));
    assert(state.attempted && state.enable_attempted && state.ready && f.barriers == 1);
    assert((*reg(&f, AW_WFDMA_CONFIG) & (AW_DMA_SETUP_BITS | AW_DMA_ENABLE_BITS)) == (AW_DMA_SETUP_BITS | AW_DMA_ENABLE_BITS));
    assert(!(*reg(&f, AW_WFDMA_CONFIG) & (AW_DMA_ADDR_EXT | AW_DMA_OMIT_RX_INFO)));
    assert(f.mcu_dummy & 2);
    assert(!aw_enable_firmware_dma(&io, &memory, &rings, &state));
    f = (fake){0}; state = (aw_dma_enable_state){0}; prepare(&f, &memory);
    f.pcie_mask = 1;
    assert(!aw_enable_firmware_dma(&io, &memory, &rings, &state) && !f.writes && !state.attempted);
    f = (fake){.fail_write = 18}; state = (aw_dma_enable_state){0}; prepare(&f, &memory);
    assert(!aw_enable_firmware_dma(&io, &memory, &rings, &state));
    assert(state.attempted && state.enable_attempted && !state.ready);
    assert((*reg(&f, AW_WFDMA_CONFIG) & AW_DMA_ENABLE_BITS) == AW_DMA_ENABLE_BITS && memory.published);
    return 0;
}
