// SPDX-License-Identifier: Apache-2.0

//! Records of the `sta` group (see the module docs of `records`).

use crate::bounded::{BoundedVec, CapacityError};
use crate::float::Float32;
use crate::limits::{EM_MAX_FRAME_BODY_LEN, LONG_STRING_CAP, STRING_CAP};
#[cfg(test)]
use crate::mapping::{Map, NO_WIRE};
use crate::text::CStrBytes;
use em_types::MacAddr;

record! {
    /// One alarm sample of a link report.
    ///
    /// C `em_alarm_samples_t` (`inc/em_base.h:1404`).
    pub struct AlarmSamples = "em_alarm_samples_t", copy {
        /// C `link_quality_score`: `float`.
        pub link_quality_score: Float32 = Map::field("link_quality_score"),
        /// C `reporting_time`: `em_string_t`.
        /// Wire: the reader copies the whole 32-byte array with no NUL check.
        pub reporting_time: CStrBytes<STRING_CAP> = Map::field("reporting_time").cstr().wire("32 @ src/em/metrics/em_metrics.cpp:1001"),
        /// C `snr`: `float`.
        pub snr: Float32 = Map::field("snr"),
        /// C `per`: `float`.
        pub per: Float32 = Map::field("per"),
        /// C `phy`: `float`.
        pub phy: Float32 = Map::field("phy"),
    }
}

record! {
    /// Link statistics report of a STA.
    ///
    /// C `em_link_report_t` (`inc/em_base.h:1412`).
    #[derive(Default)]
    pub struct LinkReport = "em_link_report_t", owned {
        /// C `sta_mac`: `mac_addr_t`.
        pub sta_mac: MacAddr = Map::field("sta_mac"),
        /// C `reporting_timestamp`: `em_string_t`.
        /// Wire: the reader copies 31 bytes and writes the NUL itself.
        pub reporting_timestamp: CStrBytes<STRING_CAP> = Map::field("reporting_timestamp").cstr().wire("31 @ src/em/metrics/em_metrics.cpp:979"),
        /// C `link_quality_threshold`: `float`.
        pub link_quality_threshold: Float32 = Map::field("link_quality_threshold"),
        /// C `alarm_triggered`: `bool`.
        pub alarm_triggered: bool = Map::field("alarm_triggered"),
        /// C `alarm_sample`: `em_alarm_samples_t[EM_MAX_SAMPLES_PER_LINK_REPORT]` with count `sample_count`.
        pub alarm_sample: BoundedVec<AlarmSamples, 10> = Map::counted("sample_count", "alarm_sample"),
    }
}

record! {
    /// Wide-bandwidth channel of a beacon report.
    ///
    /// C `wifi_WideBWChannel_t` (`include/wifi_hal_extender.h:535 in rdkb-halif-wifi @ 1deddfc`).
    pub struct WideBwChannel = "wifi_WideBWChannel_t", copy {
        /// C `bandwidth`: `UCHAR`.
        pub bandwidth: u8 = Map::field("bandwidth"),
        /// C `centerSeg0`: `UCHAR`.
        pub center_seg0: u8 = Map::field("centerSeg0"),
        /// C `centerSeg1`: `UCHAR`.
        pub center_seg1: u8 = Map::field("centerSeg1"),
    }
}

record! {
    /// One beacon measurement report (HAL type).
    ///
    /// C `wifi_BeaconReport_t` (`include/wifi_hal_extender.h:864 in rdkb-halif-wifi @ 1deddfc`).
    pub struct BeaconReport = "wifi_BeaconReport_t", copy {
        /// C `opClass`: `UCHAR`.
        pub op_class: u8 = Map::field("opClass"),
        /// C `channel`: `UCHAR`.
        pub channel: u8 = Map::field("channel"),
        /// C `startTime`: `ULLONG`.
        pub start_time: u64 = Map::field("startTime"),
        /// C `duration`: `USHORT`.
        pub duration: u16 = Map::field("duration"),
        /// C `frameInfo`: `UCHAR`.
        pub frame_info: u8 = Map::field("frameInfo"),
        /// C `rcpi`: `UCHAR`.
        pub rcpi: u8 = Map::field("rcpi"),
        /// C `rsni`: `UCHAR`.
        pub rsni: u8 = Map::field("rsni"),
        /// C `bssid`: `bssid_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `antenna`: `UCHAR`.
        pub antenna: u8 = Map::field("antenna"),
        /// C `tsf`: `UINT`.
        pub tsf: u32 = Map::field("tsf"),
        /// C `wideBandWidthChannelPresent`: `BOOL`.
        pub wide_bandwidth_channel_present: bool = Map::field("wideBandWidthChannelPresent"),
        /// C `wideBandwidthChannel`: `wifi_WideBWChannel_t`.
        pub wide_bandwidth_channel: WideBwChannel = Map::field("wideBandwidthChannel"),
        /// C `numRepetitions`: `USHORT`.
        pub num_repetitions: u16 = Map::field("numRepetitions"),
    }
}

record! {
    /// A STA. Large: store it as `Box<StaInfo>` in collections (inline size in the crate README).
    /// `Default` is the all-zero C record: every number zero, every string and list empty.
    ///
    /// C `em_sta_info_t` (`inc/em_base.h:2708`).
    #[derive(Default)]
    pub struct StaInfo = "em_sta_info_t", owned {
        /// C `id`: `mac_address_t`.
        pub id: MacAddr = Map::field("id"),
        /// C `bssid`: `mac_address_t`.
        pub bssid: MacAddr = Map::field("bssid"),
        /// C `radiomac`: `mac_address_t`.
        pub radiomac: MacAddr = Map::field("radiomac"),
        /// C `associated`: `bool`.
        pub associated: bool = Map::field("associated"),
        /// C `sta_client_type`: `em_string_t`.
        /// Wire: the reader cuts the 32-byte array at its NUL, or at 31 bytes.
        pub sta_client_type: CStrBytes<STRING_CAP> = Map::field("sta_client_type").cstr().wire("31 @ src/em/metrics/em_metrics.cpp:182"),
        /// C `timestamp`: `em_long_string_t`.
        pub timestamp: CStrBytes<LONG_STRING_CAP> = Map::field("timestamp").cstr().wire(NO_WIRE),
        /// C `last_ul_rate`: `unsigned int`.
        pub last_ul_rate: u32 = Map::field("last_ul_rate"),
        /// C `last_dl_rate`: `unsigned int`.
        pub last_dl_rate: u32 = Map::field("last_dl_rate"),
        /// C `est_ul_rate`: `unsigned int`.
        pub est_ul_rate: u32 = Map::field("est_ul_rate"),
        /// C `est_dl_rate`: `unsigned int`.
        pub est_dl_rate: u32 = Map::field("est_dl_rate"),
        /// C `last_conn_time`: `unsigned int`.
        pub last_conn_time: u32 = Map::field("last_conn_time"),
        /// C `retrans_count`: `unsigned int`.
        pub retrans_count: u32 = Map::field("retrans_count"),
        /// C `signal_strength`: `signed int`.
        pub signal_strength: i32 = Map::field("signal_strength"),
        /// C `rcpi`: `unsigned char`.
        pub rcpi: u8 = Map::field("rcpi"),
        /// C `util_tx`: `unsigned int`.
        pub util_tx: u32 = Map::field("util_tx"),
        /// C `util_rx`: `unsigned int`.
        pub util_rx: u32 = Map::field("util_rx"),
        /// C `pkts_tx`: `unsigned int`.
        pub pkts_tx: u32 = Map::field("pkts_tx"),
        /// C `pkts_rx`: `unsigned int`.
        pub pkts_rx: u32 = Map::field("pkts_rx"),
        /// C `bytes_tx`: `unsigned int`.
        pub bytes_tx: u32 = Map::field("bytes_tx"),
        /// C `bytes_rx`: `unsigned int`.
        pub bytes_rx: u32 = Map::field("bytes_rx"),
        /// C `errors_tx`: `unsigned int`.
        pub errors_tx: u32 = Map::field("errors_tx"),
        /// C `errors_rx`: `unsigned int`.
        pub errors_rx: u32 = Map::field("errors_rx"),
        /// C `frame_body`: `unsigned char *` with length `frame_body_len`, bounded by `EM_MAX_FRAME_BODY_LEN` (11 426, `inc/em_base.h:74`). Set it with `StaInfo::set_frame_body`; a rejected update keeps the old frame.
        pub frame_body: BoundedVec<u8, EM_MAX_FRAME_BODY_LEN> = Map::counted("frame_body_len", "frame_body").bound_cite("inc/em_base.h:74"),
        /// C `multi_band_cap`: `bool`.
        pub multi_band_cap: bool = Map::field("multi_band_cap"),
        /// C `beacon_query_sent_time`: `time_t`.
        pub beacon_query_sent_time: i64 = Map::field("beacon_query_sent_time"),
        /// C `beacon_report_elem`: `unsigned char[EM_MAX_BEACON_MEASUREMENT_LEN]` with count `beacon_report_len`.
        pub beacon_report_elem: BoundedVec<u8, 400> = Map::counted("beacon_report_len", "beacon_report_elem"),
        /// C `delta_ms`: `unsigned int`.
        pub delta_ms: u32 = Map::field("delta_ms"),
        /// C `cap`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub cap: CStrBytes<LONG_STRING_CAP> = Map::field("cap").cstr().wire("126 @ src/dm/dm_sta.cpp:592"),
        /// C `ht_cap`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub ht_cap: CStrBytes<LONG_STRING_CAP> = Map::field("ht_cap").cstr().wire("126 @ src/dm/dm_sta.cpp:539"),
        /// C `vht_cap`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub vht_cap: CStrBytes<LONG_STRING_CAP> = Map::field("vht_cap").cstr().wire("126 @ src/dm/dm_sta.cpp:556"),
        /// C `he_cap`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub he_cap: CStrBytes<LONG_STRING_CAP> = Map::field("he_cap").cstr().wire("126 @ src/dm/dm_sta.cpp:586"),
        /// C `wifi6_cap`: `em_long_string_t`.
        pub wifi6_cap: CStrBytes<LONG_STRING_CAP> = Map::field("wifi6_cap").cstr().wire(NO_WIRE),
        /// C `wifi7_cap`: `em_long_string_t`.
        pub wifi7_cap: CStrBytes<LONG_STRING_CAP> = Map::field("wifi7_cap").cstr().wire(NO_WIRE),
        /// C `cellular_data_pref`: `em_long_string_t`.
        pub cellular_data_pref: CStrBytes<LONG_STRING_CAP> = Map::field("cellular_data_pref").cstr().wire(NO_WIRE),
        /// C `listen_interval`: `em_long_string_t`.
        pub listen_interval: CStrBytes<LONG_STRING_CAP> = Map::field("listen_interval").cstr().wire(NO_WIRE),
        /// C `ssid`: `em_long_string_t`.
        /// Wire: an SSID IE of up to 255 bytes is copied unchecked into 128 bytes (upstream defect 2); the native type rejects more than 127.
        pub ssid: CStrBytes<LONG_STRING_CAP> = Map::field("ssid").cstr().wire("255 @ src/dm/dm_sta.cpp:507"),
        /// C `supp_rates`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub supp_rates: CStrBytes<LONG_STRING_CAP> = Map::field("supp_rates").cstr().wire("126 @ src/dm/dm_sta.cpp:511"),
        /// C `power_cap`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub power_cap: CStrBytes<LONG_STRING_CAP> = Map::field("power_cap").cstr().wire("126 @ src/dm/dm_sta.cpp:519"),
        /// C `supp_channels`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub supp_channels: CStrBytes<LONG_STRING_CAP> = Map::field("supp_channels").cstr().wire("126 @ src/dm/dm_sta.cpp:523"),
        /// C `rsn_info`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub rsn_info: CStrBytes<LONG_STRING_CAP> = Map::field("rsn_info").cstr().wire("126 @ src/dm/dm_sta.cpp:527"),
        /// C `ext_supp_rates`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub ext_supp_rates: CStrBytes<LONG_STRING_CAP> = Map::field("ext_supp_rates").cstr().wire("126 @ src/dm/dm_sta.cpp:515"),
        /// C `supp_op_classes`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub supp_op_classes: CStrBytes<LONG_STRING_CAP> = Map::field("supp_op_classes").cstr().wire("126 @ src/dm/dm_sta.cpp:531"),
        /// C `ext_cap`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub ext_cap: CStrBytes<LONG_STRING_CAP> = Map::field("ext_cap").cstr().wire("126 @ src/dm/dm_sta.cpp:543"),
        /// C `rm_cap`: `em_long_string_t`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value.
        pub rm_cap: CStrBytes<LONG_STRING_CAP> = Map::field("rm_cap").cstr().wire("126 @ src/dm/dm_sta.cpp:535"),
        /// C `multi_link`: `em_long_string_t`.
        /// Wire: the MLD MAC address of the multi-link element, written as a 17-character string.
        pub multi_link: CStrBytes<LONG_STRING_CAP> = Map::field("multi_link").cstr().wire("17 @ src/dm/dm_sta.cpp:580"),
        /// C `vendor_info`: `em_long_string_t[MAX_VENDOR_INFO]` with count `num_vendor_infos`.
        /// Wire: hex text of one IE of at most 63 bytes (126 characters); `dm_easy_mesh_t::hex` (`src/dm/dm_easy_mesh.cpp:1994`) refuses a longer IE and the field keeps its old value. At most `MAX_VENDOR_INFO` entries are kept.
        pub vendor_info: BoundedVec<CStrBytes<LONG_STRING_CAP>, 5> = Map::counted("num_vendor_infos", "vendor_info").cstr().wire("126 @ src/dm/dm_sta.cpp:561"),
        /// C `beacon_reports`: `wifi_BeaconReport_t[EM_MAX_BEACON_REPORTS_PER_SCAN]` with count `num_beacon_meas_report`.
        pub beacon_reports: BoundedVec<BeaconReport, 10> = Map::counted("num_beacon_meas_report", "beacon_reports"),
        /// C `link_stats_report`: `em_link_report_t`.
        pub link_stats_report: LinkReport = Map::field("link_stats_report"),
        /// C `reason_code`: `unsigned short`.
        pub reason_code: u16 = Map::field("reason_code"),
    }
}

impl StaInfo {
    /// Replaces the captured association frame body.
    ///
    /// # Errors
    ///
    /// [`CapacityError`] if `frame` is longer than `EM_MAX_FRAME_BODY_LEN` (11 426) bytes; the
    /// current frame body is kept.
    pub fn set_frame_body(&mut self, frame: Vec<u8>) -> Result<(), CapacityError> {
        self.frame_body.replace(frame)
    }
}
