/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Minimal declarations for the TLV senders extracted at run time from
 * rdkcentral/unified-wifi-mesh @ ba4180ba36748629f10c20babc4320dd69c9db4f.
 * inc/em_base.h is not included whole because it needs unavailable OneWifi
 * headers. Every type and macro comes from defines.inc and structs.inc, which
 * `regen` extracts from the pinned headers by line range; this file holds no
 * struct layouts and no upstream function bodies. It holds the standard
 * headers, the MAC typedefs, a no-op logging macro and the declaration of
 * struct em_msg_t that the extracted readers (readers.inc) are members of;
 * `regen` checks that declaration against inc/em_msg.h at the pin.
 */
#pragma once

#include <arpa/inet.h>

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
// rdkb-halif-wifi/include/wifi_hal_generic.h:261: typedef mac_address_t bssid_t;
typedef mac_address_t bssid_t;

// The extracted functions log through util::em_util_print, which logs and has no effect on the
// bytes under test. inc/util.h:396 defines em_printfout with it; here it does nothing.
#define em_printfout(...) ((void)0)

// Extracted verbatim from inc/em_base.h and inc/util.h by regen.
#include "defines.inc"
#include "structs.inc"

// inc/em_msg.h:80-81 (the members) and :115, :150, :164, :179, :242, :255, :268, :281, :282,
// :298, :326, :339, :352 (the declarations). Only the two members the readers use are declared;
// regen rejects any other member and any declaration that is not in the pinned header. The
// definitions are in readers.inc, unchanged.
struct em_msg_t {
    unsigned char *m_buff;
    unsigned int m_len;

    static bool parse_profile_tlv(const unsigned char *value, uint16_t value_len, em_profile_type_t *profile);
    static em_tlv_t *get_tlv(em_tlv_t *tlvs_buff, unsigned int buff_len, em_tlv_type_t type);
    static em_tlv_t *get_first_tlv(em_tlv_t* tlvs_buff, unsigned int buff_len);
    static em_tlv_t *get_next_tlv(em_tlv_t* tlv, em_tlv_t* tlvs_buff, unsigned int buff_len);
    bool get_radio_id(mac_address_t *mac);
    bool get_bss_id(mac_address_t *mac);
    bool get_sta_mac(mac_address_t *mac);
    bool get_profile(em_profile_type_t *profile);
    bool get_supported_service(em_supported_service_t *svc);
    bool get_freq_band(em_freq_band_t *band);
    bool get_profile_type(em_profile_type_t *profile);
    bool get_al_mac_address(unsigned char *mac);
    em_tlv_t *get_tlv(em_tlv_type_t type);
};
