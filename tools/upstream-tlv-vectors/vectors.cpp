// SPDX-License-Identifier: Apache-2.0
//
// Fixed inputs for the pinned upstream TLV senders and em_msg_t readers. `regen` compiles this
// file together with shim.hpp and the extracted defines.inc, structs.inc, senders.inc and
// readers.inc, runs it with two output paths, and validates what it wrote.
//
// First output (senders): `# Probe:` lines (sizeof/offsetof of every extracted struct), the
// column line, then one row per case with the columns case, function, inputs, return, hex.
// Second output (readers): the column line, then one row per case with the columns case,
// function, inputs, area, return, output. `-` is an empty value.

#include "shim.hpp"
#include "senders.inc"
#include "readers.inc"

#include <array>
#include <string>
#include <vector>

namespace {

constexpr size_t kBufferSize = 64;
// The buffer is pre-filled so that bytes a sender forgets to write show up in the output.
constexpr unsigned char kFill = 0xff;

std::string to_hex(const unsigned char *data, size_t length)
{
    static const char digits[] = "0123456789abcdef";
    if (length == 0) {
        return "-";
    }
    std::string text;
    for (size_t i = 0; i < length; i++) {
        text += digits[data[i] >> 4];
        text += digits[data[i] & 0x0f];
    }
    return text;
}

void write_row(std::FILE *out, const std::string &name, const char *function,
    const std::string &inputs, long returned, const unsigned char *buffer)
{
    std::fprintf(out, "%s\t%s\t%s\t%ld\t%s\n", name.c_str(), function, inputs.c_str(), returned,
        to_hex(buffer, static_cast<size_t>(returned)).c_str());
}

// One `# Probe:` line: the size of TYPE and the offset of each whole-byte member.
#define PROBE_BEGIN(type) std::fprintf(out, "# Probe: " #type " sizeof=%zu", sizeof(type))
#define PROBE_MEMBER(type, member) \
    std::fprintf(out, " offsetof(" #member ")=%zu", offsetof(type, member))
#define PROBE_END() std::fprintf(out, "\n")

void write_probe(std::FILE *out)
{
    PROBE_BEGIN(em_tlv_t);
    PROBE_MEMBER(em_tlv_t, type);
    PROBE_MEMBER(em_tlv_t, len);
    PROBE_MEMBER(em_tlv_t, value);
    PROBE_END();
    PROBE_BEGIN(em_channels_list_t);
    PROBE_MEMBER(em_channels_list_t, channel);
    PROBE_END();
    PROBE_BEGIN(em_op_class_t);
    PROBE_MEMBER(em_op_class_t, op_class);
    PROBE_MEMBER(em_op_class_t, max_tx_eirp);
    PROBE_MEMBER(em_op_class_t, num);
    PROBE_MEMBER(em_op_class_t, channels);
    PROBE_END();
    PROBE_BEGIN(em_ap_radio_basic_cap_t);
    PROBE_MEMBER(em_ap_radio_basic_cap_t, ruid);
    PROBE_MEMBER(em_ap_radio_basic_cap_t, num_bss);
    PROBE_MEMBER(em_ap_radio_basic_cap_t, op_class_num);
    PROBE_MEMBER(em_ap_radio_basic_cap_t, op_classes);
    PROBE_END();
    PROBE_BEGIN(em_unassoc_query_channel_t);
    PROBE_MEMBER(em_unassoc_query_channel_t, channel);
    PROBE_MEMBER(em_unassoc_query_channel_t, num_sta);
    PROBE_MEMBER(em_unassoc_query_channel_t, sta_list);
    PROBE_END();
    PROBE_BEGIN(em_unassoc_query_opclass_t);
    PROBE_MEMBER(em_unassoc_query_opclass_t, op_class);
    PROBE_MEMBER(em_unassoc_query_opclass_t, num_channels);
    PROBE_MEMBER(em_unassoc_query_opclass_t, channel_list);
    PROBE_END();
    PROBE_BEGIN(em_unassoc_sta_metric_entry_t);
    PROBE_MEMBER(em_unassoc_sta_metric_entry_t, sta_mac);
    PROBE_MEMBER(em_unassoc_sta_metric_entry_t, channel);
    PROBE_MEMBER(em_unassoc_sta_metric_entry_t, op_class);
    PROBE_MEMBER(em_unassoc_sta_metric_entry_t, rcpi);
    PROBE_MEMBER(em_unassoc_sta_metric_entry_t, time_delta);
    PROBE_END();
    PROBE_BEGIN(em_unassoc_sta_metrics_rsp_t);
    PROBE_MEMBER(em_unassoc_sta_metrics_rsp_t, num_entries);
    PROBE_MEMBER(em_unassoc_sta_metrics_rsp_t, entry);
    PROBE_END();
    PROBE_BEGIN(em_unassoc_sta_metric_t);
    PROBE_MEMBER(em_unassoc_sta_metric_t, sta_mac);
    PROBE_MEMBER(em_unassoc_sta_metric_t, channel);
    PROBE_MEMBER(em_unassoc_sta_metric_t, time_delta);
    PROBE_MEMBER(em_unassoc_sta_metric_t, rcpi);
    PROBE_END();
    PROBE_BEGIN(em_unassoc_sta_link_metrics_rsp_t);
    PROBE_MEMBER(em_unassoc_sta_link_metrics_rsp_t, op_class);
    PROBE_MEMBER(em_unassoc_sta_link_metrics_rsp_t, num_sta);
    PROBE_MEMBER(em_unassoc_sta_link_metrics_rsp_t, sta_metric);
    PROBE_END();
    PROBE_BEGIN(em_channel_scan_req_op_class_t);
    PROBE_MEMBER(em_channel_scan_req_op_class_t, op_class);
    PROBE_MEMBER(em_channel_scan_req_op_class_t, num_channels);
    PROBE_MEMBER(em_channel_scan_req_op_class_t, channel_list);
    PROBE_END();
    PROBE_BEGIN(em_channel_scan_req_t);
    PROBE_MEMBER(em_channel_scan_req_t, num_radios);
    PROBE_MEMBER(em_channel_scan_req_t, ruid);
    PROBE_MEMBER(em_channel_scan_req_t, num_op_classes);
    PROBE_MEMBER(em_channel_scan_req_t, op_class);
    PROBE_END();
    PROBE_BEGIN(em_client_info_t);
    PROBE_MEMBER(em_client_info_t, bssid);
    PROBE_MEMBER(em_client_info_t, client_mac_addr);
    PROBE_END();
    PROBE_BEGIN(em_ap_operational_bss_t);
    PROBE_MEMBER(em_ap_operational_bss_t, bssid);
    PROBE_MEMBER(em_ap_operational_bss_t, ssid_len);
    PROBE_MEMBER(em_ap_operational_bss_t, ssid);
    PROBE_END();
    PROBE_BEGIN(em_ap_op_bss_radio_t);
    PROBE_MEMBER(em_ap_op_bss_radio_t, ruid);
    PROBE_MEMBER(em_ap_op_bss_radio_t, bss_num);
    PROBE_MEMBER(em_ap_op_bss_radio_t, bss);
    PROBE_END();
    PROBE_BEGIN(em_ap_op_bss_t);
    PROBE_MEMBER(em_ap_op_bss_t, radios_num);
    PROBE_MEMBER(em_ap_op_bss_t, radios);
    PROBE_END();
    PROBE_BEGIN(em_profile_2_ap_cap_t);
    PROBE_MEMBER(em_profile_2_ap_cap_t, max_prior_rule);
    PROBE_MEMBER(em_profile_2_ap_cap_t, reserved1);
    PROBE_MEMBER(em_profile_2_ap_cap_t, max_vid_count);
    PROBE_END();
    PROBE_BEGIN(em_ap_ht_cap_t);
    PROBE_MEMBER(em_ap_ht_cap_t, ruid);
    PROBE_END();
    PROBE_BEGIN(em_ap_vht_cap_t);
    PROBE_MEMBER(em_ap_vht_cap_t, ruid);
    PROBE_MEMBER(em_ap_vht_cap_t, sprt_tx_mcs);
    PROBE_MEMBER(em_ap_vht_cap_t, sprt_rx_mcs);
    PROBE_END();
    PROBE_BEGIN(em_ap_he_mcs_maps_t);
    PROBE_MEMBER(em_ap_he_mcs_maps_t, tx_he_mcs);
    PROBE_MEMBER(em_ap_he_mcs_maps_t, rx_he_mcs);
    PROBE_END();
    PROBE_BEGIN(em_ap_he_cap_t);
    PROBE_MEMBER(em_ap_he_cap_t, ruid);
    PROBE_MEMBER(em_ap_he_cap_t, sprt_mcs_len);
    PROBE_MEMBER(em_ap_he_cap_t, sprt_tx_rx_mcs);
    PROBE_END();
    PROBE_BEGIN(em_bh_bss_config_t);
    PROBE_MEMBER(em_bh_bss_config_t, bssid);
    PROBE_END();
    PROBE_BEGIN(em_supported_service_t);
    PROBE_MEMBER(em_supported_service_t, num);
    PROBE_MEMBER(em_supported_service_t, service);
    PROBE_END();
    PROBE_BEGIN(em_ap_radio_advanced_cap_t);
    PROBE_MEMBER(em_ap_radio_advanced_cap_t, ruid);
    PROBE_END();
    PROBE_BEGIN(em_backhaul_bss_config_policy_t);
    PROBE_MEMBER(em_backhaul_bss_config_policy_t, bssid);
    PROBE_MEMBER(em_backhaul_bss_config_policy_t, b_profile_1_sta_disallowed);
    PROBE_MEMBER(em_backhaul_bss_config_policy_t, b_profile_2_sta_disallowed);
    PROBE_END();
}

void write_prof_2(std::FILE *out)
{
    unsigned char buffer[kBufferSize];
    std::memset(buffer, kFill, sizeof(buffer));
    short returned = create_prof_2_tlv(buffer);
    write_row(out, "default", "em_t::create_prof_2_tlv", "fill=ff", returned, buffer);
}

void write_error_code(std::FILE *out)
{
    const int values[] = { 0, 1, 255, 256 };
    mac_addr_t station = { 0x02, 0x11, 0x22, 0x33, 0x44, 0x55 };

    for (int value : values) {
        for (bool with_mac : { false, true }) {
            unsigned char buffer[kBufferSize];
            std::memset(buffer, kFill, sizeof(buffer));
            unsigned short returned =
                create_error_code_tlv(buffer, value, with_mac ? station : NULL);
            std::string name = "val-" + std::to_string(value) + (with_mac ? "-mac" : "-null");
            std::string inputs = "fill=ff;val=" + std::to_string(value) + ";sta_mac=" +
                (with_mac ? to_hex(station, sizeof(mac_address_t)) : std::string("null"));
            write_row(out, name, "em_configuration_t::create_error_code_tlv", inputs, returned,
                buffer);
        }
    }
}

void write_error_code_metrics(std::FILE *out)
{
    mac_address_t station = { 0x02, 0x11, 0x22, 0x33, 0x44, 0x55 };

    for (bool found : { false, true }) {
        for (bool associated : { false, true }) {
            unsigned char buffer[kBufferSize];
            std::memset(buffer, kFill, sizeof(buffer));
            short returned = create_error_code_tlv(buffer, station, found, associated);
            std::string name = std::string("found-") + (found ? "1" : "0") + "-assoc-" +
                (associated ? "1" : "0");
            std::string inputs = "fill=ff;sta=" + to_hex(station, sizeof(mac_address_t)) +
                ";sta_found=" + (found ? "1" : "0") + ";is_associated=" + (associated ? "1" : "0");
            write_row(out, name, "em_metrics_t::create_error_code_tlv", inputs, returned, buffer);
        }
    }
}

void write_client_info_row(std::FILE *out, const char *name, mac_address_t station, bssid_t bssid)
{
    unsigned char buffer[kBufferSize];
    std::memset(buffer, kFill, sizeof(buffer));
    short returned = create_client_info_tlv(buffer, station, bssid);
    std::string inputs = "fill=ff;sta=" + to_hex(station, sizeof(mac_address_t)) + ";bssid=" +
        to_hex(bssid, sizeof(mac_address_t));
    write_row(out, name, "em_capability_t::create_client_info_tlv", inputs, returned, buffer);
}

void write_client_info(std::FILE *out)
{
    mac_address_t station = { 0x02, 0x11, 0x22, 0x33, 0x44, 0x55 };
    bssid_t bssid = { 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff };
    write_client_info_row(out, "default", station, bssid);
    write_client_info_row(out, "swapped", bssid, station);
}

void write_backhaul_policy_row(std::FILE *out, const std::string &name, const mac_address_t bssid,
    bool profile_one, bool profile_two)
{
    unsigned char buffer[kBufferSize];
    std::memset(buffer, kFill, sizeof(buffer));
    em_backhaul_bss_config_policy_t entry;
    std::memset(&entry, 0, sizeof(entry));
    std::memcpy(entry.bssid, bssid, sizeof(mac_address_t));
    entry.b_profile_1_sta_disallowed = profile_one;
    entry.b_profile_2_sta_disallowed = profile_two;
    short returned = create_backhaul_bss_conf_policy_tlv(buffer, &entry);
    std::string inputs = "fill=ff;bssid=" + to_hex(bssid, sizeof(mac_address_t)) + ";p_one=" +
        (profile_one ? "1" : "0") + ";p_two=" + (profile_two ? "1" : "0");
    write_row(out, name, "em_policy_cfg_t::create_backhaul_bss_conf_policy_tlv", inputs, returned,
        buffer);
}

void write_backhaul_policy(std::FILE *out)
{
    const mac_address_t null_bssid = { 0, 0, 0, 0, 0, 0 };
    const mac_address_t bssid = { 0x02, 0x11, 0x22, 0x33, 0x44, 0x55 };

    // An all-zero BSSID writes nothing, even with both flags set.
    write_backhaul_policy_row(out, "null-bssid", null_bssid, true, true);
    for (bool profile_one : { false, true }) {
        for (bool profile_two : { false, true }) {
            write_backhaul_policy_row(out,
                std::string("p-one-") + (profile_one ? "1" : "0") + "-p-two-" +
                    (profile_two ? "1" : "0"),
                bssid, profile_one, profile_two);
        }
    }
}

struct QueryChannel {
    unsigned char number;
    std::vector<std::array<unsigned char, 6>> stations;
};

void write_query_case(std::FILE *out, const char *name, unsigned char op_class,
    const std::vector<QueryChannel> &channels)
{
    unsigned char buffer[kBufferSize];
    std::memset(buffer, kFill, sizeof(buffer));
    em_unassoc_query_opclass_t op;
    std::memset(&op, 0, sizeof(op));
    op.op_class = op_class;
    op.num_channels = static_cast<uint8_t>(channels.size());
    std::string channels_text;
    for (size_t i = 0; i < channels.size(); i++) {
        op.channel_list[i].channel = channels[i].number;
        op.channel_list[i].num_sta = static_cast<uint8_t>(channels[i].stations.size());
        std::string stations_text;
        for (size_t j = 0; j < channels[i].stations.size(); j++) {
            std::memcpy(op.channel_list[i].sta_list[j], channels[i].stations[j].data(),
                sizeof(mac_address_t));
            stations_text += (j ? "+" : "") + to_hex(channels[i].stations[j].data(), 6);
        }
        channels_text += (i ? "," : "") + std::to_string(channels[i].number) + ":" +
            (stations_text.empty() ? std::string("-") : stations_text);
    }
    unsigned short returned = create_unassoc_sta_link_metrics_query_tlv(buffer, &op);
    std::string inputs = "fill=ff;op_class=" + std::to_string(op_class) + ";channels=" +
        (channels_text.empty() ? std::string("none") : channels_text);
    write_row(out, name, "em_metrics_t::create_unassoc_sta_link_metrics_query_tlv", inputs,
        returned, buffer);
}

void write_unassoc_query(std::FILE *out)
{
    const std::array<unsigned char, 6> station = { 0x02, 0x11, 0x22, 0x33, 0x44, 0x55 };
    const std::array<unsigned char, 6> other = { 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f };
    const std::array<unsigned char, 6> third = { 0x02, 0x11, 0x22, 0x33, 0x44, 0xaa };

    unsigned char buffer[kBufferSize];
    std::memset(buffer, kFill, sizeof(buffer));
    unsigned short returned = create_unassoc_sta_link_metrics_query_tlv(buffer, NULL);
    write_row(out, "null-op", "em_metrics_t::create_unassoc_sta_link_metrics_query_tlv",
        "fill=ff;op=null", returned, buffer);

    write_query_case(out, "no-channels", 115, {});
    write_query_case(out, "one-channel-one-sta", 115, { { 36, { station } } });
    write_query_case(out, "one-channel-two-sta", 121, { { 100, { station, other } } });
    write_query_case(out, "two-channels", 115, { { 36, { station, other } }, { 40, { third } } });
    write_query_case(out, "channel-without-sta", 115, { { 36, {} }, { 40, { station } } });
}

struct ResponseEntry {
    std::array<unsigned char, 6> station;
    unsigned char channel;
    unsigned char op_class;
    unsigned char rcpi;
    unsigned int time_delta;
};

void write_response_case(std::FILE *out, const char *name, unsigned char op_class,
    const std::vector<ResponseEntry> &entries)
{
    unsigned char buffer[kBufferSize];
    std::memset(buffer, kFill, sizeof(buffer));
    em_unassoc_sta_metrics_rsp_t rsp;
    std::memset(&rsp, 0, sizeof(rsp));
    rsp.num_entries = static_cast<unsigned int>(entries.size());
    std::string entries_text;
    for (size_t i = 0; i < entries.size(); i++) {
        std::memcpy(rsp.entry[i].sta_mac, entries[i].station.data(), sizeof(mac_address_t));
        rsp.entry[i].channel = entries[i].channel;
        rsp.entry[i].op_class = entries[i].op_class;
        rsp.entry[i].rcpi = entries[i].rcpi;
        rsp.entry[i].time_delta = entries[i].time_delta;
        entries_text += (i ? "," : "") + to_hex(entries[i].station.data(), 6) + "/" +
            std::to_string(entries[i].channel) + "/" + std::to_string(entries[i].op_class) + "/" +
            std::to_string(entries[i].rcpi) + "/" + std::to_string(entries[i].time_delta);
    }
    unsigned short returned = create_unassoc_sta_link_metrics_resp_tlv(buffer, op_class, &rsp);
    std::string inputs = "fill=ff;op_class=" + std::to_string(op_class) + ";entries=" +
        (entries_text.empty() ? std::string("none") : entries_text);
    write_row(out, name, "em_metrics_t::create_unassoc_sta_link_metrics_resp_tlv", inputs,
        returned, buffer);
}

void write_unassoc_response(std::FILE *out)
{
    const std::array<unsigned char, 6> station = { 0x02, 0x11, 0x22, 0x33, 0x44, 0x55 };
    const std::array<unsigned char, 6> other = { 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f };
    const std::array<unsigned char, 6> third = { 0x02, 0x11, 0x22, 0x33, 0x44, 0xaa };
    const std::vector<ResponseEntry> mixed = {
        { station, 36, 115, 90, 1 },
        { other, 100, 121, 70, 16909060 },
        { third, 40, 115, 60, 4294967295U },
    };

    write_response_case(out, "empty", 115, {});
    write_response_case(out, "one-entry", 115, { { station, 36, 115, 90, 1 } });
    write_response_case(out, "op-115-of-mixed", 115, mixed);
    write_response_case(out, "op-121-of-mixed", 121, mixed);
    write_response_case(out, "op-125-of-mixed", 125, mixed);
}

// ---------------------------------------------------------------------------------------------
// Readers. Every area is TLVs plus an End of Message TLV, copied into a buffer of exactly its
// length so that a read past the end shows up under a sanitizer.
// ---------------------------------------------------------------------------------------------

struct TlvSpec {
    em_tlv_type_t type;
    const char *value_hex;
};

struct AreaSpec {
    const char *name;
    std::vector<TlvSpec> tlvs;
};

// The corpus. tools/upstream-tlv-vectors/regen holds the same table, written from the
// specifications, and compares every area byte for byte. The pinned test names that some areas
// follow are cited there.
const std::vector<AreaSpec> kAreas = {
    {"eom-only", {}},
    {"al-mac", {{em_tlv_type_al_mac_address, "aabbccddeeff"}}},
    {"device-info-only", {{em_tlv_type_device_info, "112233445566"}}},
    {"device-info-then-al-mac", {{em_tlv_type_device_info, "112233"}, {em_tlv_type_al_mac_address, "aabbccddeeff"}}},
    {"al-mac-twice", {{em_tlv_type_al_mac_address, "aabbccddeeff"}, {em_tlv_type_al_mac_address, "112233445566"}}},
    {"unknown-type", {{static_cast<em_tlv_type_t>(0xff), "000000000000"}}},
    {"zero-length-only", {{static_cast<em_tlv_type_t>(0xfe), ""}}},
    {"zero-length-then-al-mac", {{static_cast<em_tlv_type_t>(0xfe), ""}, {em_tlv_type_al_mac_address, "aabbccddeeff"}}},
    {"client-info", {{em_tlv_type_client_info, "aabbccddeeff112233445566"}}},
    {"client-assoc-event", {{em_tlv_type_client_assoc_event, "00000000000011223344556601"}}},
    {"client-assoc-event-sta-and-bssid", {{em_tlv_type_client_assoc_event, "c1c2c3c4c5c6b1b2b3b4b5b680"}}},
    {"ap-metrics", {{em_tlv_type_ap_metrics, "deadbeef000120000300"}}},
    {"bssid-tlv", {{em_tlv_type_bssid, "0a0b0c0d0e0f"}}},
    {"bcon-metric-query", {{em_tlv_type_bcon_metric_query, "c1c2c3c4c5c67324b1b2b3b4b5b600000000"}}},
    {"bcon-metric-rsp", {{em_tlv_type_bcon_metric_rsp, "c1c2c3c4c5c60000"}}},
    {"assoc-sta-link-metric", {{em_tlv_type_assoc_sta_link_metric, "c1c2c3c4c5c600"}}},
    {"mixed-sta", {{em_tlv_type_client_info, "aabbccddeeff112233445566"}, {em_tlv_type_client_assoc_event, "0a0b0c0d0e0fdeadbeef000180"}, {em_tlv_type_bcon_metric_query, "c1c2c3c4c5c67324b1b2b3b4b5b600000000"}, {em_tlv_type_bcon_metric_rsp, "0102030405060000"}, {em_tlv_type_assoc_sta_link_metric, "06050403020100"}}},
    {"mixed-sta-reversed", {{em_tlv_type_assoc_sta_link_metric, "06050403020100"}, {em_tlv_type_bcon_metric_rsp, "0102030405060000"}, {em_tlv_type_bcon_metric_query, "c1c2c3c4c5c67324b1b2b3b4b5b600000000"}, {em_tlv_type_client_assoc_event, "0a0b0c0d0e0fdeadbeef000180"}, {em_tlv_type_client_info, "aabbccddeeff112233445566"}}},
    {"mixed-bss", {{em_tlv_type_ap_metrics, "deadbeef000120000300"}, {em_tlv_type_bssid, "0a0b0c0d0e0f"}, {em_tlv_type_client_assoc_event, "c1c2c3c4c5c6b1b2b3b4b5b680"}, {em_tlv_type_client_info, "aabbccddeeff112233445566"}}},
    {"mixed-bss-reversed", {{em_tlv_type_client_info, "aabbccddeeff112233445566"}, {em_tlv_type_client_assoc_event, "c1c2c3c4c5c6b1b2b3b4b5b680"}, {em_tlv_type_bssid, "0a0b0c0d0e0f"}, {em_tlv_type_ap_metrics, "deadbeef000120000300"}}},
    {"profile-1-octet-reserved", {{em_tlv_type_profile, "00"}}},
    {"profile-1-octet-p1", {{em_tlv_type_profile, "01"}}},
    {"profile-1-octet-p2", {{em_tlv_type_profile, "02"}}},
    {"profile-1-octet-p3", {{em_tlv_type_profile, "03"}}},
    {"profile-4-octet-p1", {{em_tlv_type_profile, "01000000"}}},
    {"profile-4-octet-p2", {{em_tlv_type_profile, "02000000"}}},
    {"profile-4-octet-p3", {{em_tlv_type_profile, "03000000"}}},
    {"profile-value-out-of-range", {{em_tlv_type_profile, "04"}}},
    {"profile-value-ff", {{em_tlv_type_profile, "ff"}}},
    {"profile-length-2", {{em_tlv_type_profile, "0200"}}},
    {"profile-length-0", {{em_tlv_type_profile, ""}}},
    {"profile-twice", {{em_tlv_type_profile, "01"}, {em_tlv_type_profile, "03"}}},
    {"profile-after-other", {{em_tlv_type_device_info, "112233445566"}, {em_tlv_type_profile, "02"}}},
    {"supported-service-0", {{em_tlv_type_supported_service, "00"}}},
    {"supported-service-1", {{em_tlv_type_supported_service, "0100"}}},
    {"supported-service-3", {{em_tlv_type_supported_service, "030001a1"}}},
    {"supported-service-8", {{em_tlv_type_supported_service, "08000102a0a1030405"}}},
    {"supported-service-9", {{em_tlv_type_supported_service, "09000102a0a103040506"}}},
    {"supported-service-twice", {{em_tlv_type_supported_service, "0100"}, {em_tlv_type_supported_service, "0101"}}},
    {"supported-service-after-other", {{em_tlv_type_device_info, "112233445566"}, {em_tlv_type_supported_service, "0201a1"}}},
    {"supported-freq-band-5ghz", {{em_tlv_type_supported_freq_band, "01"}}},
    {"supported-freq-band-6ghz", {{em_tlv_type_supported_freq_band, "03"}}},
    {"autoconf-freq-band-24ghz", {{em_tlv_type_autoconf_freq_band, "00"}}},
    {"autoconf-freq-band-60ghz", {{em_tlv_type_autoconf_freq_band, "02"}}},
    {"freq-band-supported-then-autoconf", {{em_tlv_type_supported_freq_band, "01"}, {em_tlv_type_autoconf_freq_band, "00"}}},
    {"freq-band-autoconf-then-supported", {{em_tlv_type_autoconf_freq_band, "03"}, {em_tlv_type_supported_freq_band, "02"}}},
    {"radio-id", {{em_tlv_type_radio_id, "a0a1a2a3a4a5"}}},
    {"ap-radio-basic-cap", {{em_tlv_type_ap_radio_basic_cap, "b0b1b2b3b4b51000"}}},
    {"ap-radio-advanced-cap", {{em_tlv_type_ap_radio_advanced_cap, "c0c1c2c3c4c500"}}},
    {"ht-cap", {{em_tlv_type_ht_cap, "d0d1d2d3d4d500"}}},
    {"vht-cap", {{em_tlv_type_vht_cap, "e0e1e2e3e4e5000000000000"}}},
    {"he-cap", {{em_tlv_type_he_cap, "f0f1f2f3f4f504ffffffff0000"}}},
    {"channel-pref", {{em_tlv_type_channel_pref, "10111213141500"}}},
    {"channel-sel-resp", {{em_tlv_type_channel_sel_resp, "20212223242500"}}},
    {"op-channel-report", {{em_tlv_type_op_channel_report, "3031323334350000"}}},
    {"channel-scan-req-1-radio", {{em_tlv_type_channel_scan_req, "800140414243444500"}}},
    {"channel-scan-rslt", {{em_tlv_type_channel_scan_rslt, "50515253545573240000201000000000000000"}}},
    {"radio-metric", {{em_tlv_type_radio_metric, "60616263646510203040"}}},
    {"bh-sta-radio-cap", {{em_tlv_type_bh_sta_radio_cap, "70717273747500"}}},
    {"operational-bss-0-radios", {{em_tlv_type_operational_bss, "00"}}},
    {"operational-bss-0-radios-then-radio-id", {{em_tlv_type_operational_bss, "00"}, {em_tlv_type_radio_id, "b0b1b2b3b4b5"}}},
    {"operational-bss-1-radio", {{em_tlv_type_operational_bss, "01a0a1a2a3a4a501112233445566046d657368"}}},
    {"operational-bss-2-radios", {{em_tlv_type_operational_bss, "02a0a1a2a3a4a500b0b1b2b3b4b500"}}},
    {"channel-scan-req-0-radios", {{em_tlv_type_channel_scan_req, "8000"}}},
    {"channel-scan-req-0-radios-full-size", {{em_tlv_type_channel_scan_req, "800000000000000000"}}},
    {"channel-scan-req-0-radios-then-radio-id", {{em_tlv_type_channel_scan_req, "800000000000000000"}, {em_tlv_type_radio_id, "b0b1b2b3b4b5"}}},
    {"radio-id-after-unknown", {{static_cast<em_tlv_type_t>(0xff), "00"}, {em_tlv_type_radio_id, "a0a1a2a3a4a5"}}},
    {"radio-id-twice", {{em_tlv_type_radio_id, "a0a1a2a3a4a5"}, {em_tlv_type_radio_id, "b0b1b2b3b4b5"}}},
    {"ht-cap-then-radio-id", {{em_tlv_type_ht_cap, "d0d1d2d3d4d500"}, {em_tlv_type_radio_id, "a0a1a2a3a4a5"}}},
    {"mixed-radio", {{em_tlv_type_channel_pref, "10111213141500"}, {em_tlv_type_channel_sel_resp, "20212223242500"}, {em_tlv_type_op_channel_report, "3031323334350000"}, {em_tlv_type_channel_scan_rslt, "50515253545573240000201000000000000000"}, {em_tlv_type_radio_metric, "60616263646510203040"}, {em_tlv_type_bh_sta_radio_cap, "70717273747500"}, {em_tlv_type_ap_radio_basic_cap, "b0b1b2b3b4b51000"}, {em_tlv_type_ap_radio_advanced_cap, "c0c1c2c3c4c500"}, {em_tlv_type_ht_cap, "d0d1d2d3d4d500"}, {em_tlv_type_vht_cap, "e0e1e2e3e4e5000000000000"}, {em_tlv_type_he_cap, "f0f1f2f3f4f504ffffffff0000"}, {em_tlv_type_operational_bss, "01a0a1a2a3a4a500"}, {em_tlv_type_channel_scan_req, "800140414243444500"}, {em_tlv_type_radio_id, "a0a1a2a3a4a5"}}},
    {"mixed-radio-reversed", {{em_tlv_type_radio_id, "a0a1a2a3a4a5"}, {em_tlv_type_channel_scan_req, "800140414243444500"}, {em_tlv_type_operational_bss, "01a0a1a2a3a4a500"}, {em_tlv_type_he_cap, "f0f1f2f3f4f504ffffffff0000"}, {em_tlv_type_vht_cap, "e0e1e2e3e4e5000000000000"}, {em_tlv_type_ht_cap, "d0d1d2d3d4d500"}, {em_tlv_type_ap_radio_advanced_cap, "c0c1c2c3c4c500"}, {em_tlv_type_ap_radio_basic_cap, "b0b1b2b3b4b51000"}, {em_tlv_type_bh_sta_radio_cap, "70717273747500"}, {em_tlv_type_radio_metric, "60616263646510203040"}, {em_tlv_type_channel_scan_rslt, "50515253545573240000201000000000000000"}, {em_tlv_type_op_channel_report, "3031323334350000"}, {em_tlv_type_channel_sel_resp, "20212223242500"}, {em_tlv_type_channel_pref, "10111213141500"}}},
    {"autoconf-search-like", {{em_tlv_type_al_mac_address, "aabbccddeeff"}, {em_tlv_type_supported_freq_band, "02"}, {em_tlv_type_profile, "02"}, {em_tlv_type_supported_service, "0100"}}},
    {"client-capability-like", {{em_tlv_type_client_info, "aabbccddeeff112233445566"}, {em_tlv_type_device_info, "00"}, {em_tlv_type_radio_id, "a0a1a2a3a4a5"}}},
};

// Type that no area holds; get_tlv is also asked for it.
const em_tlv_type_t kAbsentType = static_cast<em_tlv_type_t>(0xfd);

std::vector<unsigned char> from_hex(const char *text)
{
    std::vector<unsigned char> bytes;
    for (size_t i = 0; text[i] != '\0'; i += 2) {
        char pair[3] = { text[i], text[i + 1], '\0' };
        bytes.push_back(static_cast<unsigned char>(std::strtoul(pair, NULL, 16)));
    }
    return bytes;
}

void append_tlv(std::vector<unsigned char> &area, unsigned char type,
    const std::vector<unsigned char> &value)
{
    area.push_back(type);
    area.push_back(static_cast<unsigned char>(value.size() >> 8));
    area.push_back(static_cast<unsigned char>(value.size() & 0xff));
    area.insert(area.end(), value.begin(), value.end());
}

struct BuiltArea {
    std::string name;
    std::vector<unsigned char> bytes;
    std::vector<size_t> offsets;           // of every TLV, End of Message included
    std::vector<em_tlv_type_t> types;      // each type of the area once, in order
};

BuiltArea build_area(const AreaSpec &spec)
{
    BuiltArea area;
    area.name = spec.name;
    for (const TlvSpec &tlv : spec.tlvs) {
        area.offsets.push_back(area.bytes.size());
        append_tlv(area.bytes, tlv.type, from_hex(tlv.value_hex));
        bool seen = false;
        for (em_tlv_type_t type : area.types) {
            seen = seen || type == tlv.type;
        }
        if (!seen) {
            area.types.push_back(tlv.type);
        }
    }
    area.offsets.push_back(area.bytes.size());
    append_tlv(area.bytes, em_tlv_type_eom, {});
    return area;
}

std::string tlv_offset_text(const em_tlv_t *found, const unsigned char *base)
{
    if (found == NULL) {
        return "-";
    }
    return std::to_string(reinterpret_cast<const unsigned char *>(found) - base);
}

void write_reader_row(std::FILE *out, const std::string &name, const char *function,
    const std::string &inputs, const std::vector<unsigned char> &area, const std::string &returned,
    const std::string &output)
{
    std::fprintf(out, "%s\t%s\t%s\t%s\t%s\t%s\n", name.c_str(), function, inputs.c_str(),
        to_hex(area.data(), area.size()).c_str(), returned.c_str(), output.c_str());
}

const char *bool_text(bool value)
{
    return value ? "true" : "false";
}

// Runs a reader that writes into an output object of type T, pre-filled with 0xff, on every area.
template <typename T, typename Call>
void write_out_parameter_rows(std::FILE *out, const std::vector<BuiltArea> &areas,
    const char *function, Call call)
{
    for (const BuiltArea &area : areas) {
        std::vector<unsigned char> buffer(area.bytes);
        em_msg_t msg;
        msg.m_buff = buffer.data();
        msg.m_len = static_cast<unsigned int>(buffer.size());
        // A real T object, so the reader writes through a pointer to an object of its own type.
        T output;
        std::memset(&output, kFill, sizeof(output));
        bool result = call(msg, &output);
        write_reader_row(out, area.name, function,
            "len=" + std::to_string(buffer.size()) + ";fill=ff", area.bytes, bool_text(result),
            to_hex(reinterpret_cast<const unsigned char *>(&output), sizeof(output)));
    }
}

void write_whole_area_readers(std::FILE *out, const std::vector<BuiltArea> &areas)
{
    write_out_parameter_rows<unsigned char[6]>(out, areas, "em_msg_t::get_al_mac_address",
        [](em_msg_t &msg, unsigned char (*mac)[6]) { return msg.get_al_mac_address(*mac); });
    write_out_parameter_rows<em_supported_service_t>(out, areas, "em_msg_t::get_supported_service",
        [](em_msg_t &msg, em_supported_service_t *svc) { return msg.get_supported_service(svc); });
    write_out_parameter_rows<em_profile_type_t>(out, areas, "em_msg_t::get_profile",
        [](em_msg_t &msg, em_profile_type_t *profile) { return msg.get_profile(profile); });
    write_out_parameter_rows<mac_address_t>(out, areas, "em_msg_t::get_sta_mac",
        [](em_msg_t &msg, mac_address_t *mac) { return msg.get_sta_mac(mac); });
    write_out_parameter_rows<mac_address_t>(out, areas, "em_msg_t::get_bss_id",
        [](em_msg_t &msg, mac_address_t *mac) { return msg.get_bss_id(mac); });
    write_out_parameter_rows<mac_address_t>(out, areas, "em_msg_t::get_radio_id",
        [](em_msg_t &msg, mac_address_t *mac) { return msg.get_radio_id(mac); });
    write_out_parameter_rows<em_freq_band_t>(out, areas, "em_msg_t::get_freq_band",
        [](em_msg_t &msg, em_freq_band_t *band) { return msg.get_freq_band(band); });
    write_out_parameter_rows<em_profile_type_t>(out, areas, "em_msg_t::get_profile_type",
        [](em_msg_t &msg, em_profile_type_t *profile) { return msg.get_profile_type(profile); });
    for (const BuiltArea &area : areas) {
        std::vector<unsigned char> buffer(area.bytes);
        em_tlv_t *found = em_msg_t::get_first_tlv(reinterpret_cast<em_tlv_t *>(buffer.data()),
            static_cast<unsigned int>(buffer.size()));
        write_reader_row(out, area.name, "em_msg_t::get_first_tlv",
            "len=" + std::to_string(buffer.size()), area.bytes,
            tlv_offset_text(found, buffer.data()), "-");
    }
}

std::string hex_byte(unsigned char value)
{
    return to_hex(&value, 1);
}

void write_type_readers(std::FILE *out, const std::vector<BuiltArea> &areas)
{
    for (const BuiltArea &area : areas) {
        std::vector<em_tlv_type_t> types = area.types;
        types.push_back(em_tlv_type_eom);
        types.push_back(kAbsentType);
        for (em_tlv_type_t type : types) {
            std::vector<unsigned char> buffer(area.bytes);
            em_msg_t msg;
            msg.m_buff = buffer.data();
            msg.m_len = static_cast<unsigned int>(buffer.size());
            std::string inputs = "len=" + std::to_string(buffer.size()) + ";type=" +
                hex_byte(static_cast<unsigned char>(type));
            std::string name = area.name + "@type-" + hex_byte(static_cast<unsigned char>(type));
            write_reader_row(out, name, "em_msg_t::get_tlv(type)", inputs, area.bytes,
                tlv_offset_text(msg.get_tlv(type), buffer.data()), "-");
        }
    }
    for (const BuiltArea &area : areas) {
        std::vector<em_tlv_type_t> types = area.types;
        types.push_back(em_tlv_type_eom);
        types.push_back(kAbsentType);
        for (em_tlv_type_t type : types) {
            std::vector<unsigned char> buffer(area.bytes);
            std::string inputs = "len=" + std::to_string(buffer.size()) + ";type=" +
                hex_byte(static_cast<unsigned char>(type));
            std::string name = area.name + "@type-" + hex_byte(static_cast<unsigned char>(type));
            em_tlv_t *found = em_msg_t::get_tlv(reinterpret_cast<em_tlv_t *>(buffer.data()),
                static_cast<unsigned int>(buffer.size()), type);
            write_reader_row(out, name, "em_msg_t::get_tlv(buff,len,type)", inputs, area.bytes,
                tlv_offset_text(found, buffer.data()), "-");
        }
    }
}

void write_next_tlv_readers(std::FILE *out, const std::vector<BuiltArea> &areas)
{
    for (const BuiltArea &area : areas) {
        for (size_t offset : area.offsets) {
            std::vector<unsigned char> buffer(area.bytes);
            em_tlv_t *current = reinterpret_cast<em_tlv_t *>(buffer.data() + offset);
            em_tlv_t *found = em_msg_t::get_next_tlv(current,
                reinterpret_cast<em_tlv_t *>(buffer.data()), static_cast<unsigned int>(buffer.size()));
            std::string inputs = "len=" + std::to_string(buffer.size()) + ";from=" +
                std::to_string(offset);
            write_reader_row(out, area.name + "@from-" + std::to_string(offset),
                "em_msg_t::get_next_tlv", inputs, area.bytes, tlv_offset_text(found, buffer.data()),
                "-");
        }
    }
}

struct ProfileCase {
    const char *name;
    const char *value_hex;
    uint16_t value_len;
};

void write_parse_profile(std::FILE *out)
{
    const ProfileCase cases[] = {
        { "value-0-length-1", "00", 1 },
        { "value-1-length-1", "01", 1 },
        { "value-2-length-1", "02", 1 },
        { "value-3-length-1", "03", 1 },
        { "value-4-length-1", "04", 1 },
        { "value-ff-length-1", "ff", 1 },
        { "value-1-length-4", "01000000", 4 },
        { "value-2-length-4", "02000000", 4 },
        { "value-3-length-4", "03000000", 4 },
        { "value-4-length-4", "04000000", 4 },
        { "value-2-length-0", "02", 0 },
        { "value-2-length-2", "0200", 2 },
        { "value-2-length-3", "020000", 3 },
        { "value-2-length-5", "0200000000", 5 },
    };
    for (const ProfileCase &profile_case : cases) {
        std::vector<unsigned char> value = from_hex(profile_case.value_hex);
        em_profile_type_t output;
        std::memset(&output, kFill, sizeof(output));
        bool result = em_msg_t::parse_profile_tlv(value.data(), profile_case.value_len, &output);
        write_reader_row(out, profile_case.name, "em_msg_t::parse_profile_tlv",
            "value_len=" + std::to_string(profile_case.value_len) + ";fill=ff", value,
            bool_text(result),
            to_hex(reinterpret_cast<const unsigned char *>(&output), sizeof(output)));
    }
}

void write_readers(std::FILE *out)
{
    std::fprintf(out, "case\tfunction\tinputs\tarea\treturn\toutput\n");
    std::vector<BuiltArea> areas;
    for (const AreaSpec &spec : kAreas) {
        areas.push_back(build_area(spec));
    }
    write_whole_area_readers(out, areas);
    write_type_readers(out, areas);
    write_next_tlv_readers(out, areas);
    write_parse_profile(out);
}

} // namespace

int main(int argc, char **argv)
{
    if (argc != 3) {
        std::fprintf(stderr, "usage: vectors SENDER_OUTPUT READER_OUTPUT\n");
        return 2;
    }
    std::FILE *out = std::fopen(argv[1], "wb");
    std::FILE *reader_out = std::fopen(argv[2], "wb");
    if (out == NULL || reader_out == NULL) {
        std::perror("fopen");
        return 2;
    }
    write_probe(out);
    std::fprintf(out, "case\tfunction\tinputs\treturn\thex\n");
    write_prof_2(out);
    write_error_code(out);
    write_error_code_metrics(out);
    write_client_info(out);
    write_backhaul_policy(out);
    write_unassoc_query(out);
    write_unassoc_response(out);
    write_readers(reader_out);
    if (std::fclose(out) != 0 || std::fclose(reader_out) != 0) {
        std::perror("fclose");
        return 2;
    }
    return 0;
}
