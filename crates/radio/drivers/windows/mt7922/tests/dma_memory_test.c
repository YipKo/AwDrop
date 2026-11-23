#include "../dma_memory.h"
#include <assert.h>

_Alignas(16) static uint8_t storage[AW_MCU_BLOCK_COUNT][16384];
typedef struct fake { unsigned attempts, released; int fail_at, overlap_at; } fake;
static int allocate(void *context, uint32_t bytes, aw_mcu_block *out) {
    fake *f = context;
    unsigned i = f->attempts++;
    assert(i < AW_MCU_BLOCK_COUNT && bytes <= sizeof(storage[0]));
    out->handle = storage[i];
    out->virtual_address = storage[i];
    out->address = 0x10000u + (uint64_t)i * 0x10000u;
    if (f->overlap_at == (int)i) out->address = 0x10000u;
    out->bytes = bytes;
    return f->fail_at != (int)i;
}
static void release(void *context, void *handle) {
    fake *f = context;
    assert(handle);
    ++f->released;
}
int main(void) {
    fake f = {.fail_at = -1, .overlap_at = -1};
    aw_mcu_allocator allocator = {&f, allocate, release};
    aw_mcu_memory memory = {0};
    assert(AW_MCU_MEMORY_BYTES == 28800);
    assert(aw_mcu_memory_prepare(&memory, &allocator));
    assert(memory.prepared && f.attempts == AW_MCU_BLOCK_COUNT);
    assert(aw_mcu_memory_mark_published(&memory));
    assert(!aw_mcu_memory_mark_published(&memory));
    assert(!aw_mcu_memory_release(&memory, &allocator, 0));
    assert(f.released == 0 && memory.published);
    assert(aw_mcu_memory_release(&memory, &allocator, 1));
    assert(f.released == AW_MCU_BLOCK_COUNT && !memory.prepared && !memory.published);
    assert(aw_mcu_memory_release(&memory, &allocator, 0));
    assert(f.released == AW_MCU_BLOCK_COUNT);
    f = (fake){.fail_at = 3, .overlap_at = -1};
    assert(!aw_mcu_memory_prepare(&memory, &allocator));
    assert(f.attempts == 4 && f.released == 4 && !memory.attempted);
    f = (fake){.fail_at = -1, .overlap_at = 2};
    assert(!aw_mcu_memory_prepare(&memory, &allocator));
    assert(f.attempts == 3 && f.released == 3 && !memory.attempted);
    return 0;
}
