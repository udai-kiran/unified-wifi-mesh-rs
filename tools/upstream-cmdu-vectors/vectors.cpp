/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Reference bytes from rdkcentral/unified-wifi-mesh @
 * ba4180ba36748629f10c20babc4320dd69c9db4f.
 *
 * inc/em_base.h is not included whole: it pulls in unavailable OneWifi /
 * rdk-wifi-hal headers. The definitions below are extracted verbatim from
 * that pinned header, except for the two explicitly substituted MAC typedefs.
 * The three functions below are extracted verbatim from src/em/em_msg.cpp;
 * their em_msg_t:: qualifiers are removed and static is added for this tool.
 */

#include <arpa/inet.h>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <utility>
#include <vector>

// OneWifi/lib/datapipeline/dpp_types.h:37: typedef uint8_t mac_address_t[MAC_ADDRESS_LEN];
// OneWifi/include/wifi_base.h:239: typedef unsigned char mac_addr_t[MAC_ADDR_LEN];
// OneWifi/include/wifi_base.h:205: #define MAC_ADDR_LEN 6
typedef unsigned char mac_address_t[6];
typedef unsigned char mac_addr_t[6];

// inc/em_base.h:45
#define ETH_P_1905      0x893a

// inc/em_base.h:438-442
typedef struct {
    mac_address_t   dst;
    mac_address_t   src;
    unsigned short  type;
} __attribute__((__packed__)) em_raw_hdr_t;

// inc/em_base.h:458-462
typedef struct {
    unsigned char   type;
    unsigned short  len;
    unsigned char   value[0];
} __attribute__((__packed__)) em_tlv_t;

// inc/em_base.h:464-473
typedef struct {
    unsigned char ver;
    unsigned char   reserved;
    unsigned short  type;
    unsigned short  id;
    unsigned char   frag_id;
    unsigned char   reserved_field:6;
    unsigned char   relay_ind:1;
    unsigned char   last_frag_ind:1;
} __attribute__((__packed__)) em_cmdu_t;

// inc/em_base.h:698-771
typedef enum {
    em_msg_type_topo_disc = 0x0000,
    em_msg_type_topo_notif,
    em_msg_type_topo_query,
    em_msg_type_topo_resp,
    em_msg_type_topo_vendor,
    em_msg_type_link_metric_query,
    em_msg_type_link_metric_resp,
    em_msg_type_autoconf_search,
    em_msg_type_autoconf_resp,
    em_msg_type_autoconf_wsc,
    em_msg_type_autoconf_renew,
    em_msg_type_1905_ack = 0x8000,
    em_msg_type_ap_cap_query,
    em_msg_type_ap_cap_rprt,
    em_msg_type_map_policy_config_req,
    em_msg_type_channel_pref_query,
    em_msg_type_channel_pref_rprt,
    em_msg_type_channel_sel_req,
    em_msg_type_channel_sel_rsp,
    em_msg_type_op_channel_rprt,
    em_msg_type_client_cap_query,
    em_msg_type_client_cap_rprt,
    em_msg_type_ap_metrics_query,
    em_msg_type_ap_metrics_rsp,
    em_msg_type_assoc_sta_link_metrics_query,
    em_msg_type_assoc_sta_link_metrics_rsp,
    em_msg_type_unassoc_sta_link_metrics_query,
    em_msg_type_unassoc_sta_link_metrics_rsp,
    em_msg_type_beacon_metrics_query,
    em_msg_type_beacon_metrics_rsp,
    em_msg_type_combined_infra_metrics,
    em_msg_type_client_steering_req,
    em_msg_type_client_steering_btm_rprt,
    em_msg_type_client_assoc_ctrl_req,
    em_msg_type_steering_complete,
    em_msg_type_higher_layer_data,
    em_msg_type_bh_steering_req,
    em_msg_type_bh_steering_rsp,
    em_msg_type_channel_scan_req,
    em_msg_type_channel_scan_rprt,
    em_msg_type_dpp_cce_ind,
    em_msg_type_1905_rekey_req,
    em_msg_type_1905_decrypt_fail,
    em_msg_type_cac_req,
    em_msg_type_cac_term,
    em_msg_type_client_disassoc_stats,
    em_msg_type_svc_prio_req,
    em_msg_type_err_rsp,
    em_msg_type_assoc_status_notif,
    em_msg_type_tunneled,
    em_msg_type_bh_sta_cap_query,
    em_msg_type_bh_sta_cap_rprt,
    em_msg_type_proxied_encap_dpp,
    em_msg_type_direct_encap_dpp,
    em_msg_type_reconfig_trigger,
    em_msg_type_bss_config_req,
    em_msg_type_bss_config_rsp,
    em_msg_type_bss_config_res,
    em_msg_type_chirp_notif,
    em_msg_type_1905_encap_eapol,
    em_msg_type_dpp_bootstrap_uri_notif,
    em_msg_type_anticipated_channel_pref,
    em_msg_type_failed_conn,
    em_msg_type_agent_list = 0x8035,
    em_msg_type_anticipated_channel_usage_rprt,
    em_msg_type_qos_mgmt_notif,
    em_msg_type_early_ap_cap_rprt = 0x8043,
    em_msg_type_ap_mld_config_req,
    em_msg_type_ap_mld_config_resp,
    em_msg_type_bsta_mld_config_req,
    em_msg_type_bsta_mld_config_resp,
    em_msg_type_avail_spectrum_inquiry = 0x8049,
} em_msg_type_t;

// inc/em_base.h:811-935
typedef enum {
    em_tlv_type_eom = 0,
    em_tlv_type_al_mac_address = 1, 
    em_tlv_type_mac_address = 2,
    em_tlv_type_device_info = 3,
    em_tlv_type_device_bridging_cap = 4,
    em_tlv_type_non1905_neigh_list = 6,
    em_tlv_type_1905_neigh_list = 7,
    em_tlv_type_link_metric = 8,
    em_tlv_type_transmitter_link_metric = 9,
    em_tlv_type_receiver_link_metric = 0x0a,
    em_tlv_type_vendor_specific = 0x0b,
    em_tlv_type_link_metric_result_code = 0x0c,
    em_tlv_type_searched_role = 0x0d,
    em_tlv_type_autoconf_freq_band = 0x0e,
    em_tlv_type_supported_role = 0x0f,
    em_tlv_type_supported_freq_band = 0x10,
    em_tlv_type_wsc = 0x11,
    em_tlv_type_supported_service = 0x80,
    em_tlv_type_searched_service = 0x81,
    em_tlv_type_radio_id = 0x82,
    em_tlv_type_operational_bss = 0x83,
    em_tlv_type_associated_clients = 0x84,
    em_tlv_type_ap_radio_basic_cap = 0x85,
    em_tlv_type_ht_cap = 0x86,
    em_tlv_type_vht_cap = 0x87,
    em_tlv_type_he_cap = 0x88,
    em_tlv_type_steering_policy = 0x89,
    em_tlv_type_metric_reporting_policy = 0x8a,
    em_tlv_type_channel_pref = 0x8b,
    em_tlv_type_radio_op_restriction = 0x8c,
    em_tlv_type_tx_power = 0x8d,
    em_tlv_type_channel_sel_resp = 0x8e,
    em_tlv_type_op_channel_report = 0x8f,
    em_tlv_type_client_info = 0x90,
    em_tlv_type_client_cap_report = 0x91,
    em_tlv_type_client_assoc_event = 0x92,
    em_tlv_type_ap_metrics_query = 0x93,
    em_tlv_type_ap_metrics = 0x94,
    em_tlv_type_sta_mac_addr = 0x95,
    em_tlv_type_assoc_sta_link_metric = 0x96,
    em_tlv_type_unassoc_sta_link_metric_query = 0x97,
    em_tlv_type_unassoc_sta_link_metric_rsp = 0x98,
    em_tlv_type_bcon_metric_query = 0x99,
    em_tlv_type_bcon_metric_rsp = 0x9a,
    em_tlv_type_steering_request = 0x9b,
    em_tlv_type_steering_btm_rprt = 0x9c,
    em_tlv_type_client_assoc_ctrl_req = 0x9d,
    em_tlv_type_bh_steering_req = 0x9e,
    em_tlv_type_bh_steering_rsp = 0x9f,
    em_tlv_type_higher_layer_data = 0xa0,
    em_tlv_type_ap_cap = 0xa1,
    em_tlv_type_assoc_sta_traffic_sts = 0xa2,
    em_tlv_type_error_code = 0xa3,
    em_tlv_type_channel_scan_rprt_policy = 0xa4,
    em_tlv_type_channel_scan_cap = 0xa5,
    em_tlv_type_channel_scan_req = 0xa6,
    em_tlv_type_channel_scan_rslt = 0xa7,
    em_tlv_type_timestamp = 0xa8,
    em_tlv_type_1905_layer_security_cap = 0xa9,
    em_tlv_type_ap_wifi6_cap = 0xaa,
    em_tlv_type_mic = 0xab,
    em_tlv_type_encrypt_payload = 0xac,
    em_tlv_type_cac_req = 0xad,
    em_tlv_type_cac_term = 0xae,
    em_tlv_type_cac_cmpltn_rprt = 0xaf,
    em_tlv_type_assoc_wifi6_sta_rprt = 0xb0,
    em_tlv_type_cac_sts_rprt = 0xb1,
    em_tlv_type_cac_cap = 0xb2,
    em_tlv_type_profile = 0xb3,
    em_tlv_type_profile_2_ap_cap = 0xb4,
    em_tlv_type_dflt_8021q_settings = 0xb5,
    em_tlv_type_traffic_separation_policy = 0xb6,
    em_tlv_type_bss_conf_rep = 0xb7,
    em_tlv_type_bssid = 0xb8,
    em_tlv_type_svc_prio_rule = 0xb9,
    em_tlv_type_dscp_map_tbl = 0xba,
    em_tlv_type_bss_conf_req = 0xbb,
    em_tlv_type_profile_2_error_code = 0xbc,
    em_tlv_type_bss_conf_rsp = 0xbd,
    em_tlv_type_ap_radio_advanced_cap = 0xbe,
    em_tlv_type_assoc_sts_notif = 0xbf,
    em_tlv_type_src_info = 0xc0,
    em_tlv_type_tunneled_msg_type = 0xc1,
    em_tlv_type_tunneled = 0xc2,
    em_tlv_type_profile2_steering_request = 0xc3,
    em_tlv_type_unsucc_assoc_policy = 0xc4,
    em_tlv_type_metric_cltn_interval = 0xc5,
    em_tlv_type_radio_metric = 0xc6,
    em_tlv_type_ap_ext_metric = 0xc7,
    em_tlv_type_assoc_sta_ext_link_metric = 0xc8,
    em_tlv_type_status_code = 0xc9,
    em_tlv_type_reason_code = 0xca,
    em_tlv_type_bh_sta_radio_cap = 0xcb,
    em_tlv_type_akm_suite = 0xcc,
    em_tlv_type_1905_encap_dpp = 0xcd,
    em_tlv_type_1905_encap_eapol = 0xce,
    em_tlv_type_dpp_bootstrap_uri_notification = 0xcf,
    em_tlv_type_backhaul_bss_conf = 0xd0,
    em_tlv_type_dpp_msg = 0xd1,
    em_tlv_type_dpp_cce_indication = 0xd2,
    em_tlv_type_dpp_chirp_value = 0xd3,
    em_tlv_type_device_inventory = 0xd4,
    em_tlv_type_agent_list = 0xd5,
    em_tlv_type_anticipated_channel_pref = 0xd6,
    em_tlv_type_channel_usage = 0xd7,
    em_tlv_type_spatial_reuse_req = 0xd8,
    em_tlv_type_spatial_reuse_rep = 0xd9,
    em_tlv_type_spatial_reuse_cfg_rsp = 0xda,
    em_tlv_type_qos_mgmt_policy = 0xdb,
    em_tlv_type_qos_mgmt_desc = 0xdc,
    em_tlv_type_ctrl_cap = 0xdd,
    em_tlv_type_wifi7_agent_cap = 0xdf,
    em_tlv_type_ap_mld_config = 0xe0,
    em_tlv_type_bsta_mld_config = 0xe1,
    em_tlv_type_assoc_sta_mld_conf_rep = 0xe2,
    em_tlv_type_affiliated_sta_metrics = 0xe4,
    em_tlv_type_tid_to_link_map_policy = 0xe6,
    em_tlv_eht_operations = 0xe7,
    em_tlv_type_avail_spectrum_inquiry_reg = 0xe8,
    em_tlv_type_avail_spectrum_inquiry_rsp = 0xe9,
    em_tlv_type_vendor_operational_bss = 0xf2,

    em_tlv_type_max
} em_tlv_type_t;

// src/em/em_msg.cpp:482-487
static unsigned char* add_buff_element(unsigned char *buff, unsigned int *len, unsigned char *element, unsigned int element_len)
{
    memcpy(buff, element, element_len);
    *len += element_len;
    return buff + element_len;
}

// src/em/em_msg.cpp:489-502
static unsigned char* add_tlv(unsigned char *buff, unsigned int *len, em_tlv_type_t tlv_type, 
                                            unsigned char *value, unsigned int value_len)
{
    em_tlv_t* tlv = reinterpret_cast<em_tlv_t *> (buff);
    tlv->type = tlv_type;
    tlv->len = (htons(static_cast<short unsigned int>(value_len)));
    if (value_len > 0) {
        memcpy(tlv->value, value, value_len);
    }

    *len += static_cast<unsigned int> (sizeof(em_tlv_t) + value_len);
    return buff + (sizeof(em_tlv_t) + value_len);
    
}

// src/em/em_msg.cpp:503-538
static unsigned char* add_1905_header(unsigned char *buff, unsigned int *len, mac_addr_t dst, mac_addr_t src, em_msg_type_t msg_type, unsigned short msg_id)
{

    uint16_t type = htons(ETH_P_1905);

    unsigned char* tmp = buff;
    tmp = add_buff_element(tmp, len, reinterpret_cast<uint8_t *>(dst), sizeof(mac_address_t));
    tmp = add_buff_element(tmp, len, reinterpret_cast<uint8_t *>(src), sizeof(mac_address_t));
    tmp = add_buff_element(tmp, len, reinterpret_cast<uint8_t *> (&type), sizeof(uint16_t));

    // Set relay_ind based on message type
    uint8_t relay_ind = 0;
    switch (msg_type) {
        case em_msg_type_autoconf_search:
        case em_msg_type_autoconf_renew:
        case em_msg_type_topo_notif:
            relay_ind = 1;
            break;
        default:
            relay_ind = 0;
            break;
    }

    em_cmdu_t cmdu = {
        .ver = 0,
        .reserved = 0,
        .type = htons(msg_type),
        .id = htons(msg_id),
        .frag_id = 0,
        .reserved_field = 0,
        .relay_ind = relay_ind,
        .last_frag_ind = 1
    };

    return add_buff_element(tmp, len, reinterpret_cast<uint8_t *> (&cmdu), sizeof(em_cmdu_t));
}

static_assert(sizeof(em_raw_hdr_t) == 14, "unexpected Ethernet header size");
static_assert(sizeof(em_cmdu_t) == 8, "unexpected CMDU header size");
static_assert(sizeof(em_tlv_t) == 3, "unexpected TLV header size");

static unsigned int frames_checked = 0;

static void print_hex(const unsigned char *data, std::size_t len)
{
    const char digits[] = "0123456789abcdef";
    for (std::size_t i = 0; i < len; ++i) {
        std::cout << digits[data[i] >> 4] << digits[data[i] & 0x0f];
    }
}

static void emit_case(
    const char *case_name, const char *msg_type_name, em_msg_type_t msg_type,
    unsigned short msg_id,
    std::vector<std::pair<const char *,
        std::pair<em_tlv_type_t, std::vector<unsigned char>>>> tlvs)
{
    unsigned char buff[1024] = {};
    unsigned int len = 0;
    mac_addr_t dst = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x13};
    mac_addr_t src = {0x02, 0x00, 0x00, 0x00, 0x00, 0x01};
    const unsigned char expected_ethernet[] = {
        0x01, 0x80, 0xc2, 0x00, 0x00, 0x13,
        0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x89, 0x3a
    };

    unsigned char *next = add_1905_header(buff, &len, dst, src, msg_type, msg_id);
    for (auto &tlv : tlvs) {
        auto &value = tlv.second.second;
        assert(value.size() <= 0xffff);
        assert(len + sizeof(em_tlv_t) + value.size() + sizeof(em_tlv_t) <= sizeof(buff));
        next = add_tlv(next, &len, tlv.second.first,
                       value.empty() ? NULL : value.data(),
                       static_cast<unsigned int>(value.size()));
    }
    assert(len + sizeof(em_tlv_t) <= sizeof(buff));
    next = add_tlv(next, &len, em_tlv_type_eom, NULL, 0);

    assert(sizeof(expected_ethernet) == sizeof(em_raw_hdr_t));
    assert(std::memcmp(buff, expected_ethernet, sizeof(expected_ethernet)) == 0);
    assert(static_cast<std::size_t>(next - buff) == len);
    ++frames_checked;

    const unsigned int relay = (buff[sizeof(em_raw_hdr_t) + sizeof(em_cmdu_t) - 1] & 0x40) != 0;
    std::cout << case_name << '\t' << msg_type_name << '\t' << msg_id << '\t' << relay << '\t';
    if (tlvs.empty()) {
        std::cout << '-';
    } else {
        bool first = true;
        for (const auto &tlv : tlvs) {
            if (!first) {
                std::cout << ';';
            }
            first = false;
            std::cout << tlv.first << '=';
            const auto &value = tlv.second.second;
            print_hex(value.data(), value.size());
        }
    }
    std::cout << '\t';
    print_hex(buff + sizeof(em_raw_hdr_t), len - sizeof(em_raw_hdr_t));
    std::cout << '\n';
}

#define HEADER_CASE(name, id) emit_case(#name + sizeof("em_msg_type_") - 1, #name, name, id, {})
#define TLV(name, bytes) {#name, {name, bytes}}

int main()
{
    std::cout
        << "# Generated by tools/upstream-cmdu-vectors/vectors.cpp\n"
        << "# Source: upstream src/em/em_msg.cpp and inc/em_base.h @ ba4180ba36748629f10c20babc4320dd69c9db4f\n"
        << "# Regenerate from a fresh scratch directory after setting UPSTREAM and REPO:\n"
        << "# mkdir -p up\n"
        << "# git --no-replace-objects -C \"$UPSTREAM\" archive ba4180ba36748629f10c20babc4320dd69c9db4f inc src/em/em_msg.cpp | tar -x -C up\n"
        << "# c++ -std=c++17 -Wall -Wextra \"$REPO/tools/upstream-cmdu-vectors/vectors.cpp\" -o vectors\n"
        << "# ./vectors > \"$REPO/crates/em-msg/tests/data/upstream_cmdu_vectors.tsv\"\n"
        << "# SPDX-License-Identifier: Apache-2.0\n"
        << "case\tmsg_type\tmsg_id\trelay\ttlvs\tcmdu_hex\n";

    HEADER_CASE(em_msg_type_topo_disc, 0);
    HEADER_CASE(em_msg_type_topo_notif, 1);
    HEADER_CASE(em_msg_type_topo_query, 0);
    HEADER_CASE(em_msg_type_topo_resp, 0);
    HEADER_CASE(em_msg_type_topo_vendor, 0);
    HEADER_CASE(em_msg_type_link_metric_query, 0);
    HEADER_CASE(em_msg_type_link_metric_resp, 0);
    HEADER_CASE(em_msg_type_autoconf_search, 0x1234);
    HEADER_CASE(em_msg_type_autoconf_resp, 7);
    HEADER_CASE(em_msg_type_autoconf_wsc, 0);
    HEADER_CASE(em_msg_type_autoconf_renew, 0xffff);
    HEADER_CASE(em_msg_type_1905_ack, 3);
    HEADER_CASE(em_msg_type_ap_cap_query, 2);
    HEADER_CASE(em_msg_type_ap_cap_rprt, 0);
    HEADER_CASE(em_msg_type_map_policy_config_req, 0);
    HEADER_CASE(em_msg_type_channel_pref_query, 0);
    HEADER_CASE(em_msg_type_channel_pref_rprt, 0);
    HEADER_CASE(em_msg_type_channel_sel_req, 0);
    HEADER_CASE(em_msg_type_channel_sel_rsp, 0);
    HEADER_CASE(em_msg_type_op_channel_rprt, 0);
    HEADER_CASE(em_msg_type_client_cap_query, 0);
    HEADER_CASE(em_msg_type_client_cap_rprt, 0);
    HEADER_CASE(em_msg_type_ap_metrics_query, 0);
    HEADER_CASE(em_msg_type_ap_metrics_rsp, 0);
    HEADER_CASE(em_msg_type_assoc_sta_link_metrics_query, 0);
    HEADER_CASE(em_msg_type_assoc_sta_link_metrics_rsp, 0);
    HEADER_CASE(em_msg_type_unassoc_sta_link_metrics_query, 0);
    HEADER_CASE(em_msg_type_unassoc_sta_link_metrics_rsp, 0);
    HEADER_CASE(em_msg_type_beacon_metrics_query, 0);
    HEADER_CASE(em_msg_type_beacon_metrics_rsp, 0);
    HEADER_CASE(em_msg_type_combined_infra_metrics, 0);
    HEADER_CASE(em_msg_type_client_steering_req, 0);
    HEADER_CASE(em_msg_type_client_steering_btm_rprt, 0);
    HEADER_CASE(em_msg_type_client_assoc_ctrl_req, 0);
    HEADER_CASE(em_msg_type_steering_complete, 0);
    HEADER_CASE(em_msg_type_higher_layer_data, 0);
    HEADER_CASE(em_msg_type_bh_steering_req, 0);
    HEADER_CASE(em_msg_type_bh_steering_rsp, 0);
    HEADER_CASE(em_msg_type_channel_scan_req, 0);
    HEADER_CASE(em_msg_type_channel_scan_rprt, 0);
    HEADER_CASE(em_msg_type_dpp_cce_ind, 0);
    HEADER_CASE(em_msg_type_1905_rekey_req, 0);
    HEADER_CASE(em_msg_type_1905_decrypt_fail, 0);
    HEADER_CASE(em_msg_type_cac_req, 0);
    HEADER_CASE(em_msg_type_cac_term, 0);
    HEADER_CASE(em_msg_type_client_disassoc_stats, 0);
    HEADER_CASE(em_msg_type_svc_prio_req, 0);
    HEADER_CASE(em_msg_type_err_rsp, 0);
    HEADER_CASE(em_msg_type_assoc_status_notif, 0);
    HEADER_CASE(em_msg_type_tunneled, 0);
    HEADER_CASE(em_msg_type_bh_sta_cap_query, 0);
    HEADER_CASE(em_msg_type_bh_sta_cap_rprt, 0);
    HEADER_CASE(em_msg_type_proxied_encap_dpp, 0);
    HEADER_CASE(em_msg_type_direct_encap_dpp, 0);
    HEADER_CASE(em_msg_type_reconfig_trigger, 0);
    HEADER_CASE(em_msg_type_bss_config_req, 0);
    HEADER_CASE(em_msg_type_bss_config_rsp, 0);
    HEADER_CASE(em_msg_type_bss_config_res, 0);
    HEADER_CASE(em_msg_type_chirp_notif, 0);
    HEADER_CASE(em_msg_type_1905_encap_eapol, 0);
    HEADER_CASE(em_msg_type_dpp_bootstrap_uri_notif, 0);
    HEADER_CASE(em_msg_type_anticipated_channel_pref, 0);
    HEADER_CASE(em_msg_type_failed_conn, 0);
    HEADER_CASE(em_msg_type_agent_list, 0);
    HEADER_CASE(em_msg_type_anticipated_channel_usage_rprt, 0);
    HEADER_CASE(em_msg_type_qos_mgmt_notif, 0);
    HEADER_CASE(em_msg_type_early_ap_cap_rprt, 0);
    HEADER_CASE(em_msg_type_ap_mld_config_req, 0);
    HEADER_CASE(em_msg_type_ap_mld_config_resp, 0);
    HEADER_CASE(em_msg_type_bsta_mld_config_req, 0);
    HEADER_CASE(em_msg_type_bsta_mld_config_resp, 0);
    HEADER_CASE(em_msg_type_avail_spectrum_inquiry, 0);

    const std::vector<unsigned char> al_mac = {0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f};
    const std::vector<unsigned char> interface_mac = {0x01, 0x02, 0x03, 0x04, 0x05, 0x06};
    const std::vector<unsigned char> zero = {0x00};
    const std::vector<unsigned char> empty;
    std::vector<unsigned char> long_value(300);
    for (std::size_t i = 0; i < long_value.size(); ++i) {
        long_value[i] = static_cast<unsigned char>(i % 256);
    }

    emit_case("topo_disc_with_tlvs", "em_msg_type_topo_disc", em_msg_type_topo_disc, 0,
              {TLV(em_tlv_type_al_mac_address, al_mac),
               TLV(em_tlv_type_mac_address, interface_mac)});
    emit_case("autoconf_search_with_tlvs", "em_msg_type_autoconf_search", em_msg_type_autoconf_search, 0x1234,
              {TLV(em_tlv_type_al_mac_address, al_mac),
               TLV(em_tlv_type_searched_role, zero),
               TLV(em_tlv_type_autoconf_freq_band, zero)});
    emit_case("zero_length_tlv", "em_msg_type_ap_metrics_rsp", em_msg_type_ap_metrics_rsp, 4,
              {TLV(em_tlv_type_ap_metrics, empty)});
    emit_case("long_tlv", "em_msg_type_autoconf_wsc", em_msg_type_autoconf_wsc, 5,
              {TLV(em_tlv_type_wsc, long_value)});

    assert(std::cout.good());
    std::cerr << "PASS: Ethernet header and length assertions for " << frames_checked << " frames\n";
    return 0;
}
