#include "../wfsys_reset.h"
#include <assert.h>

typedef struct fake {
    uint32_t selector, wfsys;
    unsigned selector_writes, reset_writes, waited, poll_reads;
    int fail_initial_read, fail_remap, fail_restore, never_ready, fail_initial_delay;
} fake;
static int read_reg(void *context, uint32_t offset, uint32_t *value) {
    fake *f = context;
    if (offset == AW_L1_SELECTOR) {
        if (f->fail_initial_read) return 0;
        *value = f->selector;
        return 1;
    }
    assert(offset == AW_L1_RESET_WINDOW && (f->selector & 0xffffu) == AW_L1_RESET_SELECTOR);
    ++f->poll_reads;
    *value = f->wfsys;
    return 1;
}
static int write_reg(void *context, uint32_t offset, uint32_t value) {
    fake *f = context;
    if (offset == AW_L1_SELECTOR) {
        ++f->selector_writes;
        if ((f->fail_remap && f->selector_writes == 1) || (f->fail_restore && f->selector_writes == 2)) return 0;
        f->selector = value;
        return 1;
    }
    assert(offset == AW_L1_RESET_WINDOW && (f->selector & 0xffffu) == AW_L1_RESET_SELECTOR);
    ++f->reset_writes;
    f->wfsys = value;
    if ((value & 1) && !f->never_ready) f->wfsys |= AW_WFSYS_READY_BIT;
    return 1;
}
static int delay(void *context, uint32_t ms) {
    fake *f = context;
    assert(ms == 1 || ms == 50);
    f->waited += ms;
    return !(ms == 50 && f->fail_initial_delay);
}
static aw_reset_result run(fake *f) {
    aw_reset_io io = {f, read_reg, write_reg, delay};
    return aw_wfsys_reset(&io);
}
int main(void) {
    fake f = {.selector = 0x12340055u, .wfsys = 1};
    assert(run(&f) == AW_RESET_OK);
    assert(f.selector == 0x12340055u && f.selector_writes == 2 && f.reset_writes == 2 && f.waited == 50);
    f = (fake){.selector = 0x12340055u, .fail_initial_read = 1};
    assert(run(&f) == AW_RESET_UNAVAILABLE && f.selector_writes == 0);
    f = (fake){.selector = 0x12340055u, .fail_remap = 1};
    assert(run(&f) == AW_RESET_UNCERTAIN && f.selector == 0x12340055u && f.selector_writes == 2);
    f = (fake){.selector = 0x12340055u, .wfsys = 1, .never_ready = 1};
    assert(run(&f) == AW_RESET_UNCERTAIN && f.selector == 0x12340055u && f.waited == 550);
    f = (fake){.selector = 0x12340055u, .wfsys = 1, .fail_initial_delay = 1};
    assert(run(&f) == AW_RESET_UNCERTAIN && f.selector == 0x12340055u && f.reset_writes == 2 && (f.wfsys & 1));
    f = (fake){.selector = 0x12340055u, .wfsys = 1, .fail_restore = 1};
    assert(run(&f) == AW_RESET_UNCERTAIN && f.selector != 0x12340055u);
    return 0;
}
