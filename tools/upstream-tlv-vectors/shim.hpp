/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Minimal declarations for the TLV senders extracted at run time from
 * rdkcentral/unified-wifi-mesh @ ba4180ba36748629f10c20babc4320dd69c9db4f.
 * inc/em_base.h is not included whole because it needs unavailable OneWifi
 * headers. Every struct comes from structs.inc, which `regen` extracts from
 * the pinned header by line range; this file holds no struct layouts and no
 * upstream function bodies, only the standard headers and the MAC typedefs.
 */
#pragma once

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

// OneWifi/lib/datapipeline/dpp_types.h:37: typedef uint8_t mac_address_t[MAC_ADDRESS_LEN];
// OneWifi/include/wifi_base.h:239: typedef unsigned char mac_addr_t[MAC_ADDR_LEN];
// OneWifi/include/wifi_base.h:205: #define MAC_ADDR_LEN 6
typedef unsigned char mac_address_t[6];
typedef unsigned char mac_addr_t[6];

// Extracted verbatim from inc/em_base.h by regen.
#include "structs.inc"

// Reader functions (later batches) use em_msg_t's m_buff and m_len members. When a batch adds
// them, declare a minimal `struct em_msg_t { unsigned char *m_buff; unsigned int m_len; };`
// here; see README.md. No reader is extracted yet.
