#include "../mcu_ownership.h"
#include <assert.h>
typedef struct fake { uint32_t selector; unsigned writes, polls, waits; int never_ready, fail_restore; } fake;
static int read_reg(void *ctx, uint32_t offset, uint32_t *value) {
    fake *f = ctx;
    if (offset == AW_L1_SELECTOR) { *value = f->selector; return 1; }
    assert(offset == AW_L1_MCU_LPCR && (f->selector & 0xffffu) == AW_L1_MCU_SELECTOR);
    ++f->polls; *value = f->never_ready ? AW_MCU_FW_OWN : 0; return 1;
}
static int write_reg(void *ctx, uint32_t offset, uint32_t value) {
    fake *f = ctx;
    ++f->writes;
    if (offset == AW_L1_SELECTOR) {
        if (f->fail_restore && f->writes == 3) return 0;
        f->selector = value;
    } else { assert(offset == AW_L1_MCU_LPCR && value == AW_MCU_DRV_OWN); }
    return 1;
}
static int delay(void *ctx, uint32_t ms) { fake *f = ctx; assert(ms == 1); ++f->waits; return 1; }
static aw_reset_result run(fake *f) { aw_reset_io io = {f, read_reg, write_reg, delay}; return aw_mcu_driver_own(&io); }
int main(void) {
    fake f = {.selector = 0x12340055};
    assert(run(&f) == AW_RESET_OK && f.selector == 0x12340055 && f.polls == 1);
    f = (fake){.selector = 0x12340055, .never_ready = 1};
    assert(run(&f) == AW_RESET_UNCERTAIN && f.selector == 0x12340055 && f.waits == 500);
    f = (fake){.selector = 0x12340055, .fail_restore = 1};
    assert(run(&f) == AW_RESET_UNCERTAIN && f.selector != 0x12340055);
    assert(aw_mcu_driver_own(0) == AW_RESET_UNAVAILABLE);
    return 0;
}
