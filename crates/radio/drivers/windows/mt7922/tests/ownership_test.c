#include "../ownership.h"
#include <assert.h>

typedef struct fake {
    uint32_t value;
    unsigned writes, claim_writes, release_writes, delays;
    int block_claim, block_release, unavailable, remove_on_claim, inspect_fail;
    uint32_t dma_flags;
    int dma_valid;
} fake;
static int fake_read(void *context, uint32_t *value) {
    fake *f = context;
    if (f->unavailable) return 0;
    *value = f->value;
    return 1;
}
static int fake_write(void *context, uint32_t value) {
    fake *f = context;
    if (f->unavailable) return 0;
    ++f->writes;
    if (value == AW_LPCTL_CLEAR_FW_OWN) {
        ++f->claim_writes;
        if (!f->block_claim) f->value &= ~AW_LPCTL_FW_OWN_SYNC;
        if (f->remove_on_claim) f->unavailable = 1;
    } else {
        assert(value == AW_LPCTL_SET_FW_OWN);
        ++f->release_writes;
        if (!f->block_release) f->value |= AW_LPCTL_FW_OWN_SYNC;
    }
    return 1;
}
static int fake_delay(void *context, uint32_t ms) {
    fake *f = context;
    assert(ms == 1 || ms == 2);
    ++f->delays;
    return !f->unavailable;
}
static int fake_inspect(void *context, uint32_t *value) {
    fake *f = context;
    if (f->unavailable || f->inspect_fail) return 0;
    *value = 0x12340005u;
    return 1;
}
static aw_own_result run(fake *f) {
    aw_own_io io = {f, fake_read, fake_write, fake_delay, fake_inspect};
    return aw_own_roundtrip(&io, &f->dma_flags, &f->dma_valid);
}
int main(void) {
    fake f = {.value = AW_LPCTL_FW_OWN_SYNC};
    assert(run(&f) == AW_OWN_OK);
    assert(f.claim_writes == 1 && f.release_writes == 1 && f.value == AW_LPCTL_FW_OWN_SYNC);
    assert(f.dma_valid && f.dma_flags == 5);
    f = (fake){0};
    assert(run(&f) == AW_OWN_BUSY && f.writes == 0);
    f = (fake){.value = UINT32_MAX};
    assert(run(&f) == AW_OWN_UNAVAILABLE && f.writes == 0);
    f = (fake){.value = AW_LPCTL_FW_OWN_SYNC, .block_claim = 1};
    assert(run(&f) == AW_OWN_CLAIM_FAILED_RESTORED);
    assert(f.claim_writes == AW_OWN_RETRIES && f.release_writes == 1);
    assert(f.delays <= AW_OWN_RETRIES * (AW_OWN_POLLS + 2));
    f = (fake){.value = AW_LPCTL_FW_OWN_SYNC, .block_release = 1};
    assert(run(&f) == AW_OWN_RECOVERY_FAILED);
    assert(f.claim_writes == 1 && f.release_writes == AW_OWN_RETRIES);
    f = (fake){.value = AW_LPCTL_FW_OWN_SYNC, .remove_on_claim = 1};
    assert(run(&f) == AW_OWN_RECOVERY_FAILED && f.claim_writes == 1 && f.release_writes == 0);
    f = (fake){.value = AW_LPCTL_FW_OWN_SYNC, .inspect_fail = 1};
    assert(run(&f) == AW_OWN_INSPECTION_FAILED_RESTORED && f.release_writes == 1 && !f.dma_valid);
    assert(aw_own_roundtrip(0, &f.dma_flags, &f.dma_valid) == AW_OWN_UNAVAILABLE);
    return 0;
}
