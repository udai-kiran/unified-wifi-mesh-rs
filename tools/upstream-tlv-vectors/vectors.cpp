// SPDX-License-Identifier: Apache-2.0
//
// Fixed inputs for the pinned upstream TLV senders. `regen` compiles this file together with
// shim.hpp, structs.inc and senders.inc (the last two are extracted from the pinned sources), runs
// it with the output path as its only argument, and validates what it wrote.
//
// Output: `# Probe:` lines (sizeof/offsetof of every extracted struct), the column line, then one
// row per case with the columns case, function, inputs, return, hex. `-` is an empty value.

#include "shim.hpp"
#include "senders.inc"

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

void write_probe(std::FILE *out)
{
    std::fprintf(out,
        "# Probe: em_profile_2_ap_cap_t sizeof=%zu offsetof(max_prior_rule)=%zu "
        "offsetof(reserved1)=%zu offsetof(max_vid_count)=%zu\n",
        sizeof(em_profile_2_ap_cap_t), offsetof(em_profile_2_ap_cap_t, max_prior_rule),
        offsetof(em_profile_2_ap_cap_t, reserved1), offsetof(em_profile_2_ap_cap_t, max_vid_count));
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

} // namespace

int main(int argc, char **argv)
{
    if (argc != 2) {
        std::fprintf(stderr, "usage: vectors OUTPUT\n");
        return 2;
    }
    std::FILE *out = std::fopen(argv[1], "wb");
    if (out == NULL) {
        std::perror("fopen");
        return 2;
    }
    write_probe(out);
    std::fprintf(out, "case\tfunction\tinputs\treturn\thex\n");
    write_prof_2(out);
    write_error_code(out);
    if (std::fclose(out) != 0) {
        std::perror("fclose");
        return 2;
    }
    return 0;
}
