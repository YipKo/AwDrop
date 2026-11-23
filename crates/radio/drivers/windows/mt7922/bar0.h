/* SPDX-License-Identifier: BSD-3-Clause-Clear
 * BAR0 matching follows the PCI resource checks in the MT7925 reference driver.
 * See ../../../LICENSE-mt7925.txt. This code only decodes values, not I/O.
 */
#pragma once
#include <stdint.h>

static int aw_bar0_address(uint32_t low, uint32_t high, uint64_t *address) {
    uint32_t type = low & 6u;
    uint64_t result;
    if (!address || (low & 1u) || type == 2u || type == 6u) return 0;
    result = (uint64_t)(low & 0xfffffff0u);
    if (type == 4u) result |= (uint64_t)high << 32;
    if (!result || result == UINT64_MAX || (low == UINT32_MAX)) return 0;
    *address = result;
    return 1;
}

static int aw_bar0_resource_matches(uint64_t bar0, uint64_t raw_start,
                                    uint64_t raw_length, uint64_t translated_length) {
    if (!bar0 || !raw_length || raw_length != translated_length) return 0;
    if (raw_start > UINT64_MAX - (raw_length - 1)) return 0;
    return bar0 == raw_start;
}
