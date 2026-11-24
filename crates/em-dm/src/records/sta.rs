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

/// IEEE 802.11 fixed fields before the IEs of an association request (`EM_ASSOC_FIXED_FIELDS_LEN`,
/// `inc/em_base.h:380`).
const ASSOC_FIXED_FIELDS_LEN: usize = 4;
/// Fixed fields of a reassociation request: the association fields plus the current AP address
/// (`EM_REASSOC_FIXED_FIELDS_LEN`, `inc/em_base.h:381`).
const REASSOC_FIXED_FIELDS_LEN: usize = ASSOC_FIXED_FIELDS_LEN + 6;
/// Element id and length (`EM_IE_HDR_LEN`, `inc/em_base.h:382`).
const IE_HEADER_LEN: usize = 2;
/// `EM_EID_SSID` and `EM_EID_SUPP_RATES` (`inc/em_base.h:383-384`).
const EID_SSID: u8 = 0;
const EID_SUPPORTED_RATES: u8 = 1;
/// Largest SSID element length the offset probe accepts: `EM_MAX_SSID_LEN - 1`
/// (`src/dm/dm_sta.cpp:429-430`, `inc/em_base.h:267`).
const MAX_PROBED_SSID_LEN: u8 = 32;
/// `dm_easy_mesh_t::hex` refuses an element whose hex text, plus a terminator, does not fit an
/// `em_long_string_t`: at most 63 bytes (`src/dm/dm_easy_mesh.cpp:1994-1998`).
const MAX_HEX_ELEMENT_LEN: usize = 63;

/// Element ids that `decode_sta_capability` reads (`tag_type_t`, `inc/em_base.h:3711-3727`).
mod tag {
    pub const SSID: u8 = 0;
    pub const SUPPORTED_RATES: u8 = 1;
    pub const POWER_CAPABILITY: u8 = 33;
    pub const SUPPORTED_CHANNELS: u8 = 36;
    pub const HT_CAPABILITIES: u8 = 45;
    pub const RSN_INFORMATION: u8 = 48;
    pub const EXTENDED_SUPPORTED_RATES: u8 = 50;
    pub const SUPPORTED_OPERATING_CLASSES: u8 = 59;
    pub const RM_ENABLED_CAPABILITY: u8 = 70;
    pub const EXTENDED_CAPABILITIES: u8 = 127;
    pub const VHT_CAPABILITY: u8 = 191;
    pub const VENDOR_SPECIFIC: u8 = 221;
    pub const EXTENDED_TAGS: u8 = 255;
    pub const EXT_HE_CAP: u8 = 35;
    pub const EXT_MULTI_LINK: u8 = 107;
    pub const EXT_EHT_CAP: u8 = 108;
}

/// Hex text of `bytes` as `dm_easy_mesh_t::hex` writes it (lowercase), or `None` when upstream
/// refuses the element (`dm_easy_mesh.cpp:1994-1998`).
fn hex_text(bytes: &[u8]) -> Option<CStrBytes<LONG_STRING_CAP>> {
    if bytes.len() > MAX_HEX_ELEMENT_LEN {
        return None;
    }
    let mut text = Vec::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(b"0123456789abcdef"[usize::from(byte >> 4)]);
        text.push(b"0123456789abcdef"[usize::from(byte & 0x0f)]);
    }
    CStrBytes::new(&text).ok()
}

/// True if the element at `off` is an SSID element followed by a Supported Rates element
/// (`starts_with_ssid_and_rates`, `src/dm/dm_sta.cpp:424-434`).
fn starts_with_ssid_and_rates(body: &[u8], off: usize) -> bool {
    if off + IE_HEADER_LEN > body.len()
        || body[off] != EID_SSID
        || body[off + 1] > MAX_PROBED_SSID_LEN
    {
        return false;
    }
    let next = off + IE_HEADER_LEN + usize::from(body[off + 1]);
    next + IE_HEADER_LEN <= body.len() && body[next] == EID_SUPPORTED_RATES
}

/// Offset of the first IE in an association frame body (`get_assoc_frame_ie_offset`,
/// `src/dm/dm_sta.cpp:435-462`): the fixed-field layouts first, accepted when the element walk
/// consumes the body exactly, then the same candidates without that requirement, then 0.
fn assoc_frame_ie_offset(body: &[u8]) -> usize {
    let offsets = [ASSOC_FIXED_FIELDS_LEN, REASSOC_FIXED_FIELDS_LEN, 0];
    for start in offsets {
        if !starts_with_ssid_and_rates(body, start) {
            continue;
        }
        let mut off = start;
        while off + IE_HEADER_LEN <= body.len() {
            off += IE_HEADER_LEN + usize::from(body[off + 1]);
        }
        if off == body.len() {
            return start;
        }
    }
    for start in offsets {
        if starts_with_ssid_and_rates(body, start) {
            return start;
        }
    }
    0
}

impl StaInfo {
    /// Replaces this record with `src`, as `dm_sta_t::copy_sta_info` does (`src/dm/dm_sta.cpp:704-733`):
    /// every field, the frame body included. Upstream copies the frame into new memory and
    /// clamps it at `EM_MAX_FRAME_BODY_LEN`; here `src` is moved in, and its frame body already
    /// respects the limit by type, so nothing is clamped.
    pub fn copy_sta_info(&mut self, src: Self) {
        *self = src;
    }

    /// Derives the IE text fields from the stored frame body
    /// (`dm_sta_t::decode_sta_capability`, `src/dm/dm_sta.cpp:464-607`).
    ///
    /// The vendor list is cleared first (`:473`), then every element of the walk is read:
    /// SSID, the hex text fields, the multi-band bit of Extended Capabilities, vendor elements
    /// (at most 5), and the extension elements Multi-Link, HE and EHT. An empty frame body
    /// changes nothing else (`:475-477`). The walk stops at a truncated element (`:483-492`).
    ///
    /// Differences, each from upstream's unchecked copies (defect 2):
    ///
    /// - An SSID element longer than 127 bytes leaves `ssid` empty; upstream overflows its
    ///   128-byte field (`:508-509`). The SSID is cut at the first NUL, as a C string read would.
    /// - An element too long for the hex text (above 63 bytes) leaves its field unchanged, as
    ///   upstream's `hex` refuses it (`dm_easy_mesh.cpp:1994-1998`). A vendor element of that
    ///   size still takes a vendor slot, as an empty text (`:562-566`; upstream's slot keeps
    ///   stale bytes).
    pub fn decode_sta_capability(&mut self) {
        self.vendor_info.clear();
        if self.frame_body.is_empty() {
            return;
        }
        // Moved out for the walk (which writes other fields of `self`) and put back: no copy.
        let body = std::mem::take(&mut self.frame_body);
        let frame = body.as_slice();
        let mut offset = assoc_frame_ie_offset(frame);
        while offset < frame.len() {
            if offset + IE_HEADER_LEN > frame.len() {
                break;
            }
            let tag_id = frame[offset];
            let length = usize::from(frame[offset + 1]);
            if offset + IE_HEADER_LEN + length > frame.len() {
                break;
            }
            let value = &frame[offset + IE_HEADER_LEN..offset + IE_HEADER_LEN + length];
            self.decode_element(tag_id, value);
            offset += IE_HEADER_LEN + length;
        }
        self.frame_body = body;
    }

    fn decode_element(&mut self, tag_id: u8, value: &[u8]) {
        match tag_id {
            tag::SSID => {
                self.ssid = CStrBytes::default();
                let until_nul = value.iter().position(|&b| b == 0).unwrap_or(value.len());
                if let Ok(ssid) = CStrBytes::new(&value[..until_nul]) {
                    self.ssid = ssid;
                }
            }
            tag::SUPPORTED_RATES => set_hex(&mut self.supp_rates, value),
            tag::EXTENDED_SUPPORTED_RATES => set_hex(&mut self.ext_supp_rates, value),
            tag::POWER_CAPABILITY => set_hex(&mut self.power_cap, value),
            tag::SUPPORTED_CHANNELS => set_hex(&mut self.supp_channels, value),
            tag::RSN_INFORMATION => set_hex(&mut self.rsn_info, value),
            tag::SUPPORTED_OPERATING_CLASSES => set_hex(&mut self.supp_op_classes, value),
            tag::RM_ENABLED_CAPABILITY => set_hex(&mut self.rm_cap, value),
            tag::HT_CAPABILITIES => set_hex(&mut self.ht_cap, value),
            tag::EXTENDED_CAPABILITIES => {
                set_hex(&mut self.ext_cap, value);
                self.multi_band_cap = value.len() >= 3 && value[2] & (1 << 3) != 0;
            }
            tag::VHT_CAPABILITY => set_hex(&mut self.vht_cap, value),
            tag::VENDOR_SPECIFIC => {
                let text = hex_text(value).unwrap_or_default();
                // At capacity (5) upstream skips the element; `push` reports it and so does nothing.
                let _ = self.vendor_info.push(text);
            }
            tag::EXTENDED_TAGS => self.decode_extension_element(value),
            _ => {}
        }
    }

    fn decode_extension_element(&mut self, value: &[u8]) {
        let Some((&ext_id, rest)) = value.split_first() else {
            return;
        };
        match ext_id {
            tag::EXT_MULTI_LINK => {
                // 2 bytes multi-link control, 1 byte common info length, 6 bytes MLD MAC.
                if rest.len() >= 9 {
                    let common_info_len = usize::from(rest[2]);
                    if common_info_len >= 6 {
                        let mac =
                            MacAddr::new([rest[3], rest[4], rest[5], rest[6], rest[7], rest[8]]);
                        if let Ok(text) = CStrBytes::new(mac.to_string().as_bytes()) {
                            self.multi_link = text;
                        }
                    }
                }
            }
            tag::EXT_HE_CAP if !rest.is_empty() => set_hex(&mut self.he_cap, rest),
            tag::EXT_EHT_CAP if !rest.is_empty() => set_hex(&mut self.cap, rest),
            _ => {}
        }
    }
}

/// Writes the hex text of `value` into `field`, or leaves `field` unchanged when upstream's
/// `hex` refuses the element.
fn set_hex(field: &mut CStrBytes<LONG_STRING_CAP>, value: &[u8]) {
    if let Some(text) = hex_text(value) {
        *field = text;
    }
}
