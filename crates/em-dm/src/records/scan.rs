// SPDX-License-Identifier: Apache-2.0

//! Records of the `scan` group (see the module docs of `records`).

use crate::bounded::BoundedVec;
use crate::enums::{ChannelBandwidth, ScannerType};
use crate::limits::{LONG_STRING_CAP, SSID_CAP};
#[cfg(test)]
use crate::mapping::{Map, NO_WIRE};
use crate::text::{CStrBytes, Text};
use em_types::MacAddr;

record! {
    /// Identity of a scan result.
    ///
    /// C `em_scan_result_id_t` (`inc/em_base.h:1064`).
    pub struct ScanResultId = "em_scan_result_id_t", copy {
        /// C `net_id`: `em_long_string_t`.
        pub net_id: Text<LONG_STRING_CAP> = Map::field("net_id").text("copy of the network id text (src/em/channel/em_channel.cpp:2429)").wire(NO_WIRE),
        /// C `dev_mac`: `mac_address_t`.
        pub dev_mac: MacAddr = Map::field("dev_mac"),
        /// C `scanner_mac`: `mac_address_t`.
        pub scanner_mac: MacAddr = Map::field("scanner_mac"),
        /// C `op_class`: `unsigned char`.
        pub op_class: u8 = Map::field("op_class"),
        /// C `channel`: `unsigned char`.
        pub channel: u8 = Map::field("channel"),
        /// C `scanner_type`: `em_scanner_type_t`.
        pub scanner_type: ScannerType = Map::field("scanner_type"),
    }
}

record! {
    /// One neighbor BSS seen by a scan.
    ///
    /// C `em_neighbor_t` (`inc/em_base.h:1073`).
    pub struct Neighbor = "em_neighbor_t", copy {
        /// C `bssid`: `bssid_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `ssid`: `ssid_t`.
        /// Wire: the reader clamps the SSID length to 31 bytes.
        pub ssid: CStrBytes<SSID_CAP> = Map::field("ssid").cstr().wire("31 @ src/em/channel/em_channel.cpp:2341").tag("0007-A1"),
        /// C `signal_strength`: `signed char`.
        pub signal_strength: i8 = Map::field("signal_strength"),
        /// C `bandwidth`: `wifi_channelBandwidth_t`.
        pub bandwidth: ChannelBandwidth = Map::field("bandwidth"),
        /// C `bss_color`: `unsigned char`.
        pub bss_color: u8 = Map::field("bss_color"),
        /// C `channel_util`: `unsigned char`.
        pub channel_util: u8 = Map::field("channel_util"),
        /// C `sta_count`: `unsigned short`.
        pub sta_count: u16 = Map::field("sta_count"),
        /// C `bss_load_element_present`: `unsigned char`.
        pub bss_load_element_present: u8 = Map::field("bss_load_element_present"),
    }
}

record! {
    /// A scan result. Large: store it as `Box<ScanResult>` in collections (inline size in the crate README).
    ///
    /// C `em_scan_result_t` (`inc/em_base.h:1084`).
    pub struct ScanResult = "em_scan_result_t", owned {
        /// C `id`: `em_scan_result_id_t`.
        pub id: ScanResultId = Map::field("id"),
        /// C `scan_status`: `unsigned char`.
        pub scan_status: u8 = Map::field("scan_status"),
        /// C `timestamp`: `em_long_string_t`.
        /// Wire: `timestamp_len` is one byte and the reader copies it unchecked into 128 bytes (upstream defect 2); the native type rejects more than 127.
        pub timestamp: CStrBytes<LONG_STRING_CAP> = Map::field("timestamp").cstr().wire("255 @ src/em/channel/em_channel.cpp:2312"),
        /// C `util`: `unsigned char`.
        pub util: u8 = Map::field("util"),
        /// C `noise`: `unsigned char`.
        pub noise: u8 = Map::field("noise"),
        /// C `neighbor`: `em_neighbor_t[EM_MAX_NEIGHBORS]` with count `num_neighbors`.
        pub neighbor: BoundedVec<Neighbor, 16> = Map::counted("num_neighbors", "neighbor"),
        /// C `aggr_scan_duration`: `unsigned int`.
        pub aggr_scan_duration: u32 = Map::field("aggr_scan_duration"),
        /// C `scan_type`: `unsigned char`.
        pub scan_type: u8 = Map::field("scan_type"),
    }
}
