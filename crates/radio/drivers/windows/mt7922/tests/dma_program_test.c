#include "../dma_program.h"
#include <assert.h>

typedef struct fake { uint32_t regs[256]; unsigned writes, barriers, fail_write; int bad_readback; } fake;
_Alignas(16) static uint8_t storage[AW_MCU_BLOCK_COUNT][16384];
static uint32_t *reg(fake *f, uint32_t offset) {
    assert(offset >= 0xd4200u && offset < 0xd4600u && !(offset & 3));
    return &f->regs[(offset - 0xd4200u) / 4];
}
static int read_reg(void *context, uint32_t offset, uint32_t *value) {
    fake *f = context;
    *value = *reg(f, offset);
    if (f->bad_readback && offset == AW_MCU_RING_REGS) *value ^= 16;
    return 1;
}
static int write_reg(void *context, uint32_t offset, uint32_t value) {
    fake *f = context;
    ++f->writes;
    *reg(f, offset) = value; /* Even a reported failure might have taken effect. */
    return f->writes != f->fail_write;
}
static void barrier(void *context) { ++((fake *)context)->barriers; }
static int program(fake *f, aw_ring_program_state *state, uint64_t command, uint64_t download, uint64_t event) {
    aw_ring_program_io io = {f, read_reg, write_reg, barrier};
    return aw_program_firmware_rings_unchecked(&io, state, command, download, event);
}
int main(void) {
    fake f = {0};
    aw_ring_program_state state = {0};
    assert(program(&f, &state, 0x10000, 0x12000, 0x14000));
    assert(state.modified && state.published && state.ready && f.barriers == 1);
    assert(*reg(&f, AW_MCU_RING_REGS) == 0x10000 && *reg(&f, AW_MCU_RING_REGS + 4) == 256);
    assert(*reg(&f, AW_FWDL_RING_REGS) == 0x12000 && *reg(&f, AW_FWDL_RING_REGS + 4) == 128);
    assert(*reg(&f, AW_EVENT_RING_REGS) == 0x14000 && *reg(&f, AW_EVENT_RING_REGS + 8) == 7);
    assert(!program(&f, &state, 0x10000, 0x12000, 0x14000));
    assert(state.published && state.ready);
    f = (fake){0}; state = (aw_ring_program_state){0};
    assert(!program(&f, &state, 0x10000, 0x10800, 0x14000) && f.writes == 0);
    *reg(&f, AW_WFDMA_CONFIG) = 5;
    assert(!program(&f, &state, 0x10000, 0x12000, 0x14000) && f.writes == 0);
    f = (fake){.fail_write = 4}; state = (aw_ring_program_state){0};
    assert(!program(&f, &state, 0x10000, 0x12000, 0x14000));
    assert(state.modified && state.published && !state.ready && *reg(&f, AW_MCU_RING_REGS) == 0x10000);
    assert(!program(&f, &state, 0x10000, 0x12000, 0x14000) && state.published);
    f = (fake){.fail_write = 2}; state = (aw_ring_program_state){0};
    assert(!program(&f, &state, 0x10000, 0x12000, 0x14000));
    assert(state.modified && !state.published && !state.ready);
    f = (fake){.bad_readback = 1}; state = (aw_ring_program_state){0};
    assert(!program(&f, &state, 0x10000, 0x12000, 0x14000));
    assert(state.published && !state.ready);
    {
        aw_mcu_memory memory = {0};
        aw_ring_program_io io = {&f, read_reg, write_reg, barrier};
        const uint32_t sizes[AW_MCU_BLOCK_COUNT] = {4096, 2048, 128, 14336, 4096, 4096};
        uint32_t i;
        f = (fake){.fail_write = 1}; state = (aw_ring_program_state){0};
        memory.prepared = memory.attempted = 1;
        for (i = 0; i < AW_MCU_BLOCK_COUNT; ++i) {
            memory.blocks[i].handle = storage[i];
            memory.blocks[i].virtual_address = storage[i];
            memory.blocks[i].address = 0x10000u + (uint64_t)i * 0x10000u;
            memory.blocks[i].bytes = sizes[i];
        }
        assert(!aw_program_mcu_memory(&io, &state, &memory));
        assert(memory.published && state.modified && !state.published);
    }
    return 0;
}
