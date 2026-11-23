#include "../dma_stop.h"
#include <assert.h>

typedef struct fake {
    uint32_t value, ext, scheduler, reset;
    unsigned reads, writes, waits, busy_reads;
    int fail_read, fail_write, keep_enabled, fail_write_at;
} fake;
static int read_reg(void *context, uint32_t offset, uint32_t *value) {
    fake *f = context;
    ++f->reads;
    if (f->fail_read) return 0;
    if (offset == 0xd42b0u) { *value = f->ext; return 1; }
    if (offset == 0xd6004u) { *value = f->scheduler; return 1; }
    if (offset == 0xd4100u) { *value = f->reset; return 1; }
    assert(offset == AW_WFDMA0_CFG);
    if (f->writes && f->busy_reads && --f->busy_reads == 0) f->value &= ~10u;
    *value = f->value;
    return 1;
}
static int write_reg(void *context, uint32_t offset, uint32_t value) {
    fake *f = context;
    ++f->writes;
    if (offset == 0xd42b0u) f->ext = value;
    else if (offset == 0xd6004u) f->scheduler = value;
    else if (offset == 0xd4100u) f->reset = value;
    else { assert(offset == AW_WFDMA0_CFG); if (!f->keep_enabled) f->value = value; }
    return !f->fail_write && f->writes != (unsigned)f->fail_write_at;
}
static int delay(void *context, uint32_t ms) {
    fake *f = context;
    assert(ms == 1);
    ++f->waits;
    return 1;
}
static aw_dma_stop_result stop(fake *f) {
    aw_dma_stop_io io = {f, read_reg, write_reg, delay};
    return aw_dma_stop_primary(&io);
}
int main(void) {
    fake f = {.value = 0x4000000fu, .busy_reads = 3};
    assert(stop(&f) == AW_DMA_STOP_IDLE);
    assert(f.value == 0x40000000u && f.writes == 1 && f.waits == 2);
    f = (fake){.fail_read = 1};
    assert(stop(&f) == AW_DMA_STOP_UNAVAILABLE && f.writes == 0);
    f = (fake){.value = UINT32_MAX};
    assert(stop(&f) == AW_DMA_STOP_UNAVAILABLE && f.writes == 0);
    f = (fake){.value = 5, .fail_write = 1};
    assert(stop(&f) == AW_DMA_STOP_UNCERTAIN && f.writes == 1);
    f = (fake){.value = 5, .keep_enabled = 1};
    assert(stop(&f) == AW_DMA_STOP_UNCERTAIN && f.waits == AW_WFDMA_STOP_POLLS);
    assert(aw_dma_stop_primary(0) == AW_DMA_STOP_UNAVAILABLE);
    {
        aw_dma_stop_io io = {&f, read_reg, write_reg, delay};
        f = (fake){.value = 15, .busy_reads = 2, .ext = 0x40, .reset = 0x30};
        assert(aw_dma_stop_force(&io) == AW_DMA_STOP_IDLE);
        assert(!(f.ext & 0x40) && (f.scheduler & 0x10000000u) && (f.reset & 0x30) == 0x30);
        f = (fake){.value = 5, .fail_write_at = 4};
        assert(aw_dma_stop_force(&io) == AW_DMA_STOP_UNCERTAIN && f.writes == 4);
    }
    return 0;
}
