/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * DMA span checks follow the MT7925 reference driver; see
 * ../../../LICENSE-mt7925.txt. This function does not touch hardware.
 */
#pragma once
#include <stdint.h>

static int aw_dma_span32(uint64_t address, uint32_t size, uint32_t alignment) {
    if (!size || !alignment || (alignment & (alignment - 1u)) || (address & (alignment - 1u))) return 0;
    if (address > UINT32_MAX) return 0;
    return (uint64_t)(size - 1u) <= (uint64_t)UINT32_MAX - address;
}
