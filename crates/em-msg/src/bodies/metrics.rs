// SPDX-License-Identifier: Apache-2.0

//! `EasyMesh` metrics TLV bodies: AP, station, radio and beacon metrics and their queries.
//!
//! Counters, rates, RCPI values, reporting details and TIDs are kept as reported, including
//! reserved values (note 0009, D2). Measurement Report elements are raw octets (D5).

use em_types::{MacAddr, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// `IncludeBE`, `IncludeBK`, `IncludeVO` and `IncludeVI` in the AP Metrics flags octet.
const INCLUDE_BE: u8 = 0x80;
const INCLUDE_BK: u8 = 0x40;
const INCLUDE_VO: u8 = 0x20;
const INCLUDE_VI: u8 = 0x10;

/// The nine zero octets upstream sends for Associated Wi-Fi 6 STA Status Report.
const UPSTREAM_WIFI6_STATUS_REPORT: [u8; 9] = [0; 9];

/// AP Metrics Query, `EasyMesh` §17.2.21 Table 43; audit rows 83–84.
///
/// Upstream `em_ap_metrics_query_t` (`em_base.h:1304-1307`) holds one BSSID and has no
/// production sender or receiver (test stub `validation_test.cpp:1620-1634`). This body keeps
/// every listed AP MAC. A count of zero is spec-valid, so it is accepted
/// (upstream's validator wants at least one, `em_msg.cpp:953`, audit row 84).
#[derive(Debug, PartialEq, Eq)]
pub struct ApMetricsQuery {
    /// AP MAC addresses (BSSIDs, or affiliated AP addresses of an MLD) in wire order.
    pub ap_macs: Vec<MacAddr>,
}

impl TlvBody<'_> for ApMetricsQuery {
    const TLV_TYPE: TlvType = TlvType::AP_METRICS_QUERY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut ap_macs = bounded_vec(count, reader.remaining(), 6);
        for _ in 0..count {
            ap_macs.push(reader.mac()?);
        }
        reader.finish()?;
        Ok(Self { ap_macs })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.ap_macs.len())?;
        for mac in &self.ap_macs {
            writer.mac(*mac);
        }
        Ok(writer.into_bytes())
    }
}

/// Reads an estimated service parameters field if its include flag is set.
fn read_optional_esp(
    reader: &mut Reader<'_>,
    flags: u8,
    include_flag: u8,
) -> Result<Option<[u8; 3]>, DecodeError> {
    if flags & include_flag == 0 {
        return Ok(None);
    }
    Ok(Some(reader.array()?))
}

/// AP Metrics, `EasyMesh` §17.2.22 Table 44; audit rows 85–89.
///
/// Upstream `em_ap_metric_t`, sender `create_ap_metrics_tlv`
/// (`em_metrics.cpp:1933-1990`), receiver `handle_ap_metrics_tlv` (`em_metrics.cpp:796-830`,
/// reads the fixed 13 octets only).
/// The spec says `IncludeBE` shall be 1, but the BE field is always present, so a clear
/// `IncludeBE` sizes nothing and is kept in [`include_be`](Self::include_be) (audit row 87:
/// upstream checks neither). The four estimated service parameter fields are raw octets
/// (note 0009, D5). The reserved flag bits are ignored on decode and written as zero.
///
/// Lenient form: an empty value is upstream's AP Metrics TLV for a BSS whose radio was not
/// requested (`em_metrics.cpp:1946`, audit row 85); it decodes to `Ok(None)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApMetrics {
    /// BSSID of the reported BSS.
    pub bssid: MacAddr,
    /// Channel utilization.
    pub channel_utilization: u8,
    /// Number of associated stations.
    pub station_count: u16,
    /// `IncludeBE` flag as read; the spec says it shall be 1.
    pub include_be: bool,
    /// Estimated Service Parameters Information for access category BE (always on the wire).
    pub esp_be: [u8; 3],
    /// Estimated Service Parameters for BK, present when `IncludeBK` is set.
    pub esp_bk: Option<[u8; 3]>,
    /// Estimated Service Parameters for VO, present when `IncludeVO` is set.
    pub esp_vo: Option<[u8; 3]>,
    /// Estimated Service Parameters for VI, present when `IncludeVI` is set.
    pub esp_vi: Option<[u8; 3]>,
}

impl TlvBody<'_> for ApMetrics {
    const TLV_TYPE: TlvType = TlvType::AP_METRICS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let bssid = reader.mac()?;
        let channel_utilization = reader.u8()?;
        let station_count = reader.be_u16()?;
        let flags = reader.u8()?;
        let esp_be = reader.array()?;
        // The optional fields follow in the order BK, VO, VI, not in bit order.
        let metrics = Self {
            bssid,
            channel_utilization,
            station_count,
            include_be: flags & INCLUDE_BE != 0,
            esp_be,
            esp_bk: read_optional_esp(&mut reader, flags, INCLUDE_BK)?,
            esp_vo: read_optional_esp(&mut reader, flags, INCLUDE_VO)?,
            esp_vi: read_optional_esp(&mut reader, flags, INCLUDE_VI)?,
        };
        reader.finish()?;
        Ok(metrics)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut flags = 0;
        if self.include_be {
            flags |= INCLUDE_BE;
        }
        if self.esp_bk.is_some() {
            flags |= INCLUDE_BK;
        }
        if self.esp_vo.is_some() {
            flags |= INCLUDE_VO;
        }
        if self.esp_vi.is_some() {
            flags |= INCLUDE_VI;
        }
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.u8(self.channel_utilization);
        writer.be_u16(self.station_count);
        writer.u8(flags);
        writer.bytes(&self.esp_be);
        for esp in [self.esp_bk, self.esp_vo, self.esp_vi]
            .into_iter()
            .flatten()
        {
            writer.bytes(&esp);
        }
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for ApMetrics {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// STA MAC Address Type, `EasyMesh` §17.2.23 Table 45; audit rows 90–91.
///
/// Upstream senders `em_metrics.cpp:1172`, `em_configuration.cpp:512-517,619-624`; receivers
/// read the six octets without a length check (audit row 91). This body requires exactly six.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaMacAddress {
    /// MAC address of the station.
    pub sta_mac: MacAddr,
}

impl TlvBody<'_> for StaMacAddress {
    const TLV_TYPE: TlvType = TlvType::STA_MAC_ADDRESS_TYPE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mac = reader.mac()?;
        reader.finish()?;
        Ok(Self { sta_mac })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        Ok(writer.into_bytes())
    }
}

/// One BSS in Associated STA Link Metrics, `EasyMesh` §17.2.24 Table 46; audit rows 92–95.
///
/// Upstream `em_assoc_link_metrics_t`. The RCPI is kept raw: 221-255 are reserved in the spec
/// and upstream stores them unchecked (audit row 93).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssociatedStaBssMetrics {
    /// BSSID (or affiliated AP address) the station is associated with.
    pub bssid: MacAddr,
    /// Time delta since the measurement.
    pub time_delta: u32,
    /// MAC data rate, downlink.
    pub mac_rate_down: u32,
    /// MAC data rate, uplink.
    pub mac_rate_up: u32,
    /// Uplink RCPI, raw.
    pub rcpi: u8,
}

impl AssociatedStaBssMetrics {
    fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            bssid: reader.mac()?,
            time_delta: reader.be_u32()?,
            mac_rate_down: reader.be_u32()?,
            mac_rate_up: reader.be_u32()?,
            rcpi: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.mac(self.bssid);
        writer.be_u32(self.time_delta);
        writer.be_u32(self.mac_rate_down);
        writer.be_u32(self.mac_rate_up);
        writer.u8(self.rcpi);
    }
}

/// Associated STA Link Metrics, `EasyMesh` §17.2.24 Table 46; audit rows 92–95.
///
/// Upstream `em_assoc_sta_link_metrics_t`, sender `create_assoc_sta_link_metrics_tlv`
/// (`em_metrics.cpp:1599-1639`, one BSS when the station is known, none otherwise). A BSS count
/// of zero is spec-valid and is accepted although upstream's validator wants one (audit row 94).
#[derive(Debug, PartialEq, Eq)]
pub struct AssociatedStaLinkMetrics {
    /// MAC address of the station.
    pub sta_mac: MacAddr,
    /// Link metrics per BSS, in wire order.
    pub bss_metrics: Vec<AssociatedStaBssMetrics>,
}

impl TlvBody<'_> for AssociatedStaLinkMetrics {
    const TLV_TYPE: TlvType = TlvType::ASSOCIATED_STA_LINK_METRICS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mac = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut bss_metrics = bounded_vec(count, reader.remaining(), 19);
        for _ in 0..count {
            bss_metrics.push(AssociatedStaBssMetrics::read(&mut reader)?);
        }
        reader.finish()?;
        Ok(Self {
            sta_mac,
            bss_metrics,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.count_u8(self.bss_metrics.len())?;
        for metrics in &self.bss_metrics {
            metrics.write(&mut writer);
        }
        Ok(writer.into_bytes())
    }
}

/// One channel in Unassociated STA Link Metrics Query, `EasyMesh` §17.2.25 Table 47.
#[derive(Debug, PartialEq, Eq)]
pub struct UnassociatedQueryChannel {
    /// Channel number.
    pub channel: u8,
    /// Stations to measure on this channel; the count may be zero.
    pub sta_macs: Vec<MacAddr>,
}

/// Unassociated STA Link Metrics Query, `EasyMesh` §17.2.25 Table 47; audit rows 96–98.
///
/// Upstream sender `create_unassoc_sta_link_metrics_query_tlv` (`em_metrics.cpp:2702-2734`,
/// one TLV per operating class). Counts use their full wire range: upstream's receiver rejects the
/// whole query above 16 channels or 64 stations per channel (`em_metrics.cpp:2581-2584`,
/// `:2607-2610`; limits at `em_base.h:237-238`; audit row 96). There is one operating class
/// per TLV: octets after the last channel are `TrailingBytes`, where upstream's receiver parses
/// further blocks (audit row 97).
#[derive(Debug, PartialEq, Eq)]
pub struct UnassociatedStaLinkMetricsQuery {
    /// Operating class of all the channels.
    pub operating_class: u8,
    /// Channels in wire order; the count may be zero.
    pub channels: Vec<UnassociatedQueryChannel>,
}

impl TlvBody<'_> for UnassociatedStaLinkMetricsQuery {
    const TLV_TYPE: TlvType = TlvType::UNASSOCIATED_STA_LINK_METRICS_QUERY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let operating_class = reader.u8()?;
        let channel_count = usize::from(reader.u8()?);
        let mut channels = bounded_vec(channel_count, reader.remaining(), 2);
        for _ in 0..channel_count {
            let channel = reader.u8()?;
            let sta_count = usize::from(reader.u8()?);
            let mut sta_macs = bounded_vec(sta_count, reader.remaining(), 6);
            for _ in 0..sta_count {
                sta_macs.push(reader.mac()?);
            }
            channels.push(UnassociatedQueryChannel { channel, sta_macs });
        }
        reader.finish()?;
        Ok(Self {
            operating_class,
            channels,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.operating_class);
        writer.count_u8(self.channels.len())?;
        for channel in &self.channels {
            writer.u8(channel.channel);
            writer.count_u8(channel.sta_macs.len())?;
            for mac in &channel.sta_macs {
                writer.mac(*mac);
            }
        }
        Ok(writer.into_bytes())
    }
}

/// One station in Unassociated STA Link Metrics Response, `EasyMesh` §17.2.26 Table 48;
/// audit rows 99–100. The RCPI is kept raw (221-255 are reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnassociatedStaMetrics {
    /// MAC address of the station.
    pub sta_mac: MacAddr,
    /// Channel the station was measured on.
    pub channel: u8,
    /// Time delta since the measurement.
    pub time_delta: u32,
    /// Uplink RCPI, raw.
    pub rcpi: u8,
}

/// Unassociated STA Link Metrics Response, `EasyMesh` §17.2.26 Table 48; audit rows 99–100.
///
/// Upstream `em_unassoc_sta_link_metrics_rsp_t` (`em_base.h:680-690`), sender
/// `create_unassoc_sta_link_metrics_resp_tlv` (`em_metrics.cpp:2807-2866`). Upstream's receiver
/// drops entries after 64 (`em_metrics.cpp:503-509`, `EM_MAX_UNASSOC_STA` at `em_base.h:650`;
/// audit row 100); this body keeps all of them.
#[derive(Debug, PartialEq, Eq)]
pub struct UnassociatedStaLinkMetricsResponse {
    /// Operating class of the measured channels.
    pub operating_class: u8,
    /// Measured stations in wire order; the count may be zero.
    pub stations: Vec<UnassociatedStaMetrics>,
}

impl TlvBody<'_> for UnassociatedStaLinkMetricsResponse {
    const TLV_TYPE: TlvType = TlvType::UNASSOCIATED_STA_LINK_METRICS_RESPONSE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let operating_class = reader.u8()?;
        let count = usize::from(reader.u8()?);
        let mut stations = bounded_vec(count, reader.remaining(), 12);
        for _ in 0..count {
            stations.push(UnassociatedStaMetrics {
                sta_mac: reader.mac()?,
                channel: reader.u8()?,
                time_delta: reader.be_u32()?,
                rcpi: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            operating_class,
            stations,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.operating_class);
        writer.count_u8(self.stations.len())?;
        for station in &self.stations {
            writer.mac(station.sta_mac);
            writer.u8(station.channel);
            writer.be_u32(station.time_delta);
            writer.u8(station.rcpi);
        }
        Ok(writer.into_bytes())
    }
}

/// An AP Channel Report in Beacon Metrics Query, `EasyMesh` §17.2.27 Table 49.
///
/// On the wire the Report Length octet counts the operating class and the channel list.
/// A channel list entry of 255 is kept as a raw channel (upstream's fallback, audit row 103).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApChannelReport<'a> {
    /// Operating class of the channels.
    pub operating_class: u8,
    /// Channel list, borrowed from the value; it can be empty.
    pub channels: &'a [u8],
}

/// Beacon Metrics Query, `EasyMesh` §17.2.27 Table 49; audit rows 101–105.
///
/// Upstream sender `create_beacon_metrics_query_tlv` (`em_metrics.cpp:1722-1904`), receiver
/// `handle_beacon_metrics_query` (`em_metrics.cpp:308`), which accepts a value without `Num_Ids`
/// (`em_metrics.cpp:416-418`, audit row 105). The value rules (a Channel Number other than 255
/// with reports, `Num_Ids` with a Reporting Detail other than 1, the 802.11 detail codes, an SSID
/// over 32 octets) are for the consumer: this body keeps the bytes as given. A Report Length of
/// zero cannot describe its own operating class and is `InvalidLength`, as upstream's receiver
/// also rejects it (`em_metrics.cpp:389-393`).
///
/// Lenient form (note 0009, D4/A7): upstream's default query ends after the AP Channel Report
/// group with no `Num_Ids` octet (audit row 101), because its sender writes `Num_Ids` only when
/// Reporting Detail is 1 and there is at least one element ID (`em_metrics.cpp:1885-1892`). The
/// lenient decoder accepts a value that ends exactly there and returns the body with no element
/// IDs; any other shortfall is `Truncated`. The encoder always writes `Num_Ids`, even when the
/// list is empty, so it never writes upstream's shorter form. This is an intentional difference.
#[derive(Debug, PartialEq, Eq)]
pub struct BeaconMetricsQuery<'a> {
    /// MAC address of the associated station.
    pub sta_mac: MacAddr,
    /// Operating class for the Beacon request.
    pub operating_class: u8,
    /// Channel number for the Beacon request (255: use the AP channel reports).
    pub channel: u8,
    /// BSSID for the Beacon request.
    pub bssid: MacAddr,
    /// Reporting Detail, raw.
    pub reporting_detail: u8,
    /// SSID octets, borrowed from the value; empty for the wildcard SSID.
    pub ssid: &'a [u8],
    /// AP Channel Reports in wire order.
    pub ap_channel_reports: Vec<ApChannelReport<'a>>,
    /// Element IDs for the Request element, borrowed from the value.
    pub element_ids: &'a [u8],
}

impl<'a> BeaconMetricsQuery<'a> {
    /// Decodes the value. With `num_ids_may_be_absent`, a value that ends exactly after the AP
    /// Channel Report group decodes with no element IDs.
    fn decode(value: &'a [u8], num_ids_may_be_absent: bool) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mac = reader.mac()?;
        let operating_class = reader.u8()?;
        let channel = reader.u8()?;
        let bssid = reader.mac()?;
        let reporting_detail = reader.u8()?;
        let ssid_len = usize::from(reader.u8()?);
        let ssid = reader.bytes(ssid_len)?;
        let report_count = usize::from(reader.u8()?);
        let mut ap_channel_reports = bounded_vec(report_count, reader.remaining(), 2);
        for _ in 0..report_count {
            let report_len = usize::from(reader.u8()?);
            if report_len == 0 {
                return Err(DecodeError::InvalidLength { len: 0 });
            }
            let operating_class = reader.u8()?;
            let channels = reader.bytes(report_len - 1)?;
            ap_channel_reports.push(ApChannelReport {
                operating_class,
                channels,
            });
        }
        let element_ids = if num_ids_may_be_absent && reader.remaining() == 0 {
            &[]
        } else {
            let num_ids = usize::from(reader.u8()?);
            reader.bytes(num_ids)?
        };
        reader.finish()?;
        Ok(Self {
            sta_mac,
            operating_class,
            channel,
            bssid,
            reporting_detail,
            ssid,
            ap_channel_reports,
            element_ids,
        })
    }
}

impl<'a> TlvBody<'a> for BeaconMetricsQuery<'a> {
    const TLV_TYPE: TlvType = TlvType::BEACON_METRICS_QUERY;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        Self::decode(value, false)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.u8(self.operating_class);
        writer.u8(self.channel);
        writer.mac(self.bssid);
        writer.u8(self.reporting_detail);
        writer.count_u8(self.ssid.len())?;
        writer.bytes(self.ssid);
        writer.count_u8(self.ap_channel_reports.len())?;
        for report in &self.ap_channel_reports {
            // Report Length counts the operating class octet too.
            let Ok(report_len) = u8::try_from(report.channels.len() + 1) else {
                return Err(EncodeError::CountTooLarge {
                    len: report.channels.len(),
                    max: usize::from(u8::MAX) - 1,
                });
            };
            writer.u8(report_len);
            writer.u8(report.operating_class);
            writer.bytes(report.channels);
        }
        writer.count_u8(self.element_ids.len())?;
        writer.bytes(self.element_ids);
        Ok(writer.into_bytes())
    }
}

impl<'a> TlvBodyLenient<'a> for BeaconMetricsQuery<'a> {
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        Self::decode(value, true).map(Some)
    }
}

/// A Measurement Report element in Beacon Metrics Response, `EasyMesh` §17.2.28 Table 50.
///
/// The element is kept raw (note 0009, D5): its ID is not checked against 39 and its body is not
/// parsed. The Length octet is the body length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasurementReportElement<'a> {
    /// Element ID, normally 39 (Measurement Report).
    pub element_id: u8,
    /// Element body, borrowed from the value; at most 255 octets.
    pub body: &'a [u8],
}

/// Beacon Metrics Response, `EasyMesh` §17.2.28 Table 50; audit rows 106–107.
///
/// Upstream sender `create_beacon_metrics_response_tlv` (`em_metrics.cpp:1907-1930`), receiver
/// `decode_beacon_report`, which does not check the element lengths (audit row 106); this body
/// does, and a length above the remaining octets is `Truncated`. The reserved octet is ignored on
/// decode and written as zero.
#[derive(Debug, PartialEq, Eq)]
pub struct BeaconMetricsResponse<'a> {
    /// MAC address of the station the report is for.
    pub sta_mac: MacAddr,
    /// Measurement Report elements in wire order.
    pub reports: Vec<MeasurementReportElement<'a>>,
}

impl<'a> TlvBody<'a> for BeaconMetricsResponse<'a> {
    const TLV_TYPE: TlvType = TlvType::BEACON_METRICS_RESPONSE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mac = reader.mac()?;
        let _reserved = reader.u8()?;
        let count = usize::from(reader.u8()?);
        let mut reports = bounded_vec(count, reader.remaining(), 2);
        for _ in 0..count {
            let element_id = reader.u8()?;
            let body_len = usize::from(reader.u8()?);
            let body = reader.bytes(body_len)?;
            reports.push(MeasurementReportElement { element_id, body });
        }
        reader.finish()?;
        Ok(Self { sta_mac, reports })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.u8(0);
        writer.count_u8(self.reports.len())?;
        for report in &self.reports {
            writer.u8(report.element_id);
            writer.count_u8(report.body.len())?;
            writer.bytes(report.body);
        }
        Ok(writer.into_bytes())
    }
}

/// Associated STA Traffic Stats, `EasyMesh` §17.2.35 Table 57; audit rows 119–121.
///
/// Upstream `em_assoc_sta_traffic_stats_t`, sender `create_assoc_sta_traffic_stats_tlv`
/// (`em_metrics.cpp:2265-2297`). The byte counters are kept as sent, in the units of the
/// agent's Profile-2 AP Capability (audit row 120); this body does not scale them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssociatedStaTrafficStats {
    /// MAC address of the station.
    pub sta_mac: MacAddr,
    /// Bytes sent.
    pub bytes_sent: u32,
    /// Bytes received.
    pub bytes_received: u32,
    /// Packets sent.
    pub packets_sent: u32,
    /// Packets received.
    pub packets_received: u32,
    /// Transmit packet errors.
    pub tx_packet_errors: u32,
    /// Receive packet errors.
    pub rx_packet_errors: u32,
    /// Retransmission count.
    pub retransmission_count: u32,
}

impl TlvBody<'_> for AssociatedStaTrafficStats {
    const TLV_TYPE: TlvType = TlvType::ASSOCIATED_STA_TRAFFIC_STATS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let stats = Self {
            sta_mac: reader.mac()?,
            bytes_sent: reader.be_u32()?,
            bytes_received: reader.be_u32()?,
            packets_sent: reader.be_u32()?,
            packets_received: reader.be_u32()?,
            tx_packet_errors: reader.be_u32()?,
            rx_packet_errors: reader.be_u32()?,
            retransmission_count: reader.be_u32()?,
        };
        reader.finish()?;
        Ok(stats)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.be_u32(self.bytes_sent);
        writer.be_u32(self.bytes_received);
        writer.be_u32(self.packets_sent);
        writer.be_u32(self.packets_received);
        writer.be_u32(self.tx_packet_errors);
        writer.be_u32(self.rx_packet_errors);
        writer.be_u32(self.retransmission_count);
        Ok(writer.into_bytes())
    }
}

/// Metric Collection Interval, `EasyMesh` §17.2.59 Table 81; audit row 195.
///
/// Upstream `em_metric_cltn_interval_t`, sender `create_metric_col_int_tlv`
/// (`em.cpp:1879-1902`). Upstream copies the value in host order, so it is little-endian on the
/// wire (audit row 195, rule UPSTREAM), unlike every other multi-octet field. This is the
/// upstream byte order, not the spec's silent default.
///
/// Lenient form: an empty value is upstream's absent TLV (`em.cpp:1886-1896` returns no value
/// when there is no radio capability information) and decodes to `Ok(None)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricCollectionInterval {
    /// Collection interval (`Device.CollectionInterval`).
    pub interval: u32,
}

impl TlvBody<'_> for MetricCollectionInterval {
    const TLV_TYPE: TlvType = TlvType::METRIC_COLLECTION_INTERVAL;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let interval = reader.le_u32()?;
        reader.finish()?;
        Ok(Self { interval })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.le_u32(self.interval);
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for MetricCollectionInterval {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// Radio Metrics, `EasyMesh` §17.2.60 Table 82; audit rows 196–197.
///
/// Upstream `em_radio_metric_t`, sender `create_radio_metrics_tlv` (`em_metrics.cpp:2034-2056`).
/// Upstream's helper reads the RUID without a length check (audit row 197); this body requires
/// all ten octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioMetrics {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Noise, raw.
    pub noise: u8,
    /// Transmit utilization, raw.
    pub transmit: u8,
    /// Receive utilization from the radio itself, raw.
    pub receive_self: u8,
    /// Receive utilization from other sources, raw.
    pub receive_other: u8,
}

impl TlvBody<'_> for RadioMetrics {
    const TLV_TYPE: TlvType = TlvType::RADIO_METRICS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let metrics = Self {
            ruid: reader.mac()?,
            noise: reader.u8()?,
            transmit: reader.u8()?,
            receive_self: reader.u8()?,
            receive_other: reader.u8()?,
        };
        reader.finish()?;
        Ok(metrics)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(self.noise);
        writer.u8(self.transmit);
        writer.u8(self.receive_self);
        writer.u8(self.receive_other);
        Ok(writer.into_bytes())
    }
}

/// AP Extended Metrics, `EasyMesh` §17.2.61 Table 83; audit rows 198–200.
///
/// Upstream `em_ap_ext_metric_t`, sender `create_ap_ext_metrics_tlv`
/// (`em_metrics.cpp:1993-2032`). For a BSS outside the requested radios upstream commits 30
/// octets of unset data (audit row 200); an all-zero value of that form is a valid body here,
/// and the byte counters are kept in the sender's units (audit row 199).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApExtendedMetrics {
    /// BSSID of the reported BSS.
    pub bssid: MacAddr,
    /// Unicast bytes sent.
    pub unicast_bytes_sent: u32,
    /// Unicast bytes received.
    pub unicast_bytes_received: u32,
    /// Multicast bytes sent.
    pub multicast_bytes_sent: u32,
    /// Multicast bytes received.
    pub multicast_bytes_received: u32,
    /// Broadcast bytes sent.
    pub broadcast_bytes_sent: u32,
    /// Broadcast bytes received.
    pub broadcast_bytes_received: u32,
}

impl TlvBody<'_> for ApExtendedMetrics {
    const TLV_TYPE: TlvType = TlvType::AP_EXTENDED_METRICS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let metrics = Self {
            bssid: reader.mac()?,
            unicast_bytes_sent: reader.be_u32()?,
            unicast_bytes_received: reader.be_u32()?,
            multicast_bytes_sent: reader.be_u32()?,
            multicast_bytes_received: reader.be_u32()?,
            broadcast_bytes_sent: reader.be_u32()?,
            broadcast_bytes_received: reader.be_u32()?,
        };
        reader.finish()?;
        Ok(metrics)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.be_u32(self.unicast_bytes_sent);
        writer.be_u32(self.unicast_bytes_received);
        writer.be_u32(self.multicast_bytes_sent);
        writer.be_u32(self.multicast_bytes_received);
        writer.be_u32(self.broadcast_bytes_sent);
        writer.be_u32(self.broadcast_bytes_received);
        Ok(writer.into_bytes())
    }
}

/// One BSS in Associated STA Extended Link Metrics, `EasyMesh` §17.2.62 Table 84;
/// audit rows 201–203. The four values are kept as reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedLinkMetricsBss {
    /// BSSID the station is associated with.
    pub bssid: MacAddr,
    /// Last data downlink rate.
    pub last_data_downlink_rate: u32,
    /// Last data uplink rate.
    pub last_data_uplink_rate: u32,
    /// Utilization, receive.
    pub utilization_receive: u32,
    /// Utilization, transmit.
    pub utilization_transmit: u32,
}

/// Associated STA Extended Link Metrics, `EasyMesh` §17.2.62 Table 84; audit rows 201–203.
///
/// Upstream `em_assoc_sta_ext_link_metrics_t`, sender `create_assoc_ext_sta_link_metrics_tlv`
/// (`em_metrics.cpp:1641-1681`, one BSS when the station is known, none otherwise). A BSS count
/// of zero is spec-valid and is accepted although upstream's validator wants one (audit row 202).
#[derive(Debug, PartialEq, Eq)]
pub struct AssociatedStaExtendedLinkMetrics {
    /// MAC address of the station.
    pub sta_mac: MacAddr,
    /// Extended link metrics per BSS, in wire order.
    pub bss_metrics: Vec<ExtendedLinkMetricsBss>,
}

impl TlvBody<'_> for AssociatedStaExtendedLinkMetrics {
    const TLV_TYPE: TlvType = TlvType::ASSOCIATED_STA_EXTENDED_LINK_METRICS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mac = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut bss_metrics = bounded_vec(count, reader.remaining(), 22);
        for _ in 0..count {
            bss_metrics.push(ExtendedLinkMetricsBss {
                bssid: reader.mac()?,
                last_data_downlink_rate: reader.be_u32()?,
                last_data_uplink_rate: reader.be_u32()?,
                utilization_receive: reader.be_u32()?,
                utilization_transmit: reader.be_u32()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            sta_mac,
            bss_metrics,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.count_u8(self.bss_metrics.len())?;
        for bss in &self.bss_metrics {
            writer.mac(bss.bssid);
            writer.be_u32(bss.last_data_downlink_rate);
            writer.be_u32(bss.last_data_uplink_rate);
            writer.be_u32(bss.utilization_receive);
            writer.be_u32(bss.utilization_transmit);
        }
        Ok(writer.into_bytes())
    }
}

/// One TID and its queue size in Associated Wi-Fi 6 STA Status Report,
/// `EasyMesh` §17.2.73 Table 95. Both values are raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wifi6TidQueueSize {
    /// Traffic identifier, raw (the spec range is 0-7).
    pub tid: u8,
    /// Queue size, raw.
    pub queue_size: u8,
}

/// Associated Wi-Fi 6 STA Status Report, `EasyMesh` §17.2.73 Table 95; audit rows 224–225.
///
/// Upstream sender `create_assoc_wifi6_sta_sta_report_tlv` (`em_metrics.cpp:2299-2306`)
/// writes nothing and returns 9. The caller's TLV buffer is zeroed (`em_msg.cpp:607`), so the TLV
/// is nine zero octets; the receiver is a no-op (audit row 225). That value is a station MAC of
/// zeros, zero TIDs and two stray octets, so the strict decoder rejects it with `TrailingBytes`.
///
/// Lenient form (note 0009, A7 exception): a value of exactly nine zero octets decodes to
/// `Ok(None)`, meaning "no report"; any other value is decoded as the spec form.
#[derive(Debug, PartialEq, Eq)]
pub struct AssociatedWifi6StaStatusReport {
    /// MAC address of the station.
    pub sta_mac: MacAddr,
    /// TID and queue size pairs in wire order; the count may be zero.
    pub tids: Vec<Wifi6TidQueueSize>,
}

impl TlvBody<'_> for AssociatedWifi6StaStatusReport {
    const TLV_TYPE: TlvType = TlvType::ASSOCIATED_WIFI_6_STA_STATUS_REPORT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mac = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut tids = bounded_vec(count, reader.remaining(), 2);
        for _ in 0..count {
            tids.push(Wifi6TidQueueSize {
                tid: reader.u8()?,
                queue_size: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self { sta_mac, tids })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.count_u8(self.tids.len())?;
        for pair in &self.tids {
            writer.u8(pair.tid);
            writer.u8(pair.queue_size);
        }
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for AssociatedWifi6StaStatusReport {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value == UPSTREAM_WIFI6_STATUS_REPORT {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Kind, Row};
    use crate::body_testkit::{Field, sweep_row, sweep_row_lenient, truncated_in};
    use core::mem::size_of;

    const GROUP: &str = "metrics";
    const STA: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4]);
    const BSSID1: MacAddr = MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]);
    const BSSID2: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]);
    const RUID: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]);
    const ZERO_MAC: MacAddr = MacAddr::new([0; 6]);
    const TOO_MANY: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    const ESP_BE: [u8; 3] = [0xa1, 0xb2, 0xc3];
    const ESP_BK: [u8; 3] = [0xb1, 0xc2, 0xd3];
    const ESP_VO: [u8; 3] = [0xc1, 0xd2, 0xe3];
    const ESP_VI: [u8; 3] = [0xd1, 0xe2, 0xf3];
    const SSID: &[u8] = b"HomeNet";
    /// The first measurement report element body of the 0x9a golden row.
    const BEACON_REPORT_BODY: [u8; 29] = [
        0x01, 0x00, 0x05, 0x73, 0x24, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x00, 0x64,
        0x0a, 0x9c, 0x40, 0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51, 0x01, 0x0a, 0x0b, 0x0c, 0x0d,
    ];
    /// Rows a lenient decoder accepts although they are `invalid` for the strict decoder.
    const LENIENT_INPUTS: &[Key] = &[
        ("0x94", "empty"),
        ("0xc5", "empty"),
        ("0xb0", "upstream-nine-zero-octets"),
    ];
    const CANONICAL: &[(Key, Key)] = &[
        (
            ("0x94", "reserved-bits-set"),
            ("0x94", "reserved-bits-clear"),
        ),
        (("0x9a", "reserved-set"), ("0x9a", "reserved-clear")),
    ];

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0x93" => sweep_row::<ApMetricsQuery>(row, bytes, canonical),
            "0x94" => sweep_row_lenient::<ApMetrics>(row, bytes, canonical, LENIENT_INPUTS),
            "0x95" => sweep_row::<StaMacAddress>(row, bytes, canonical),
            "0x96" => sweep_row::<AssociatedStaLinkMetrics>(row, bytes, canonical),
            "0x97" => sweep_row::<UnassociatedStaLinkMetricsQuery>(row, bytes, canonical),
            "0x98" => sweep_row::<UnassociatedStaLinkMetricsResponse>(row, bytes, canonical),
            "0x99" => {
                sweep_row_lenient::<BeaconMetricsQuery<'_>>(row, bytes, canonical, LENIENT_INPUTS)
            }
            "0x9a" => sweep_row::<BeaconMetricsResponse<'_>>(row, bytes, canonical),
            "0xa2" => sweep_row::<AssociatedStaTrafficStats>(row, bytes, canonical),
            "0xc5" => {
                sweep_row_lenient::<MetricCollectionInterval>(row, bytes, canonical, LENIENT_INPUTS)
            }
            "0xc6" => sweep_row::<RadioMetrics>(row, bytes, canonical),
            "0xc7" => sweep_row::<ApExtendedMetrics>(row, bytes, canonical),
            "0xc8" => sweep_row::<AssociatedStaExtendedLinkMetrics>(row, bytes, canonical),
            "0xb0" => sweep_row_lenient::<AssociatedWifi6StaStatusReport>(
                row,
                bytes,
                canonical,
                LENIENT_INPUTS,
            ),
            other => Err(format!("no body for TLV {other}")),
        }
    }

    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, CANONICAL, dispatch_row);
    }

    const fn field(offset: usize, width: usize) -> Field {
        Field { offset, width }
    }

    fn ap_metrics_query() -> ApMetricsQuery {
        ApMetricsQuery {
            ap_macs: vec![STA, BSSID1],
        }
    }
    fn ap_metrics() -> ApMetrics {
        ApMetrics {
            bssid: STA,
            channel_utilization: 87,
            station_count: 0x0102,
            include_be: true,
            esp_be: ESP_BE,
            esp_bk: Some(ESP_BK),
            esp_vo: Some(ESP_VO),
            esp_vi: Some(ESP_VI),
        }
    }
    fn sta_mac_address() -> StaMacAddress {
        StaMacAddress { sta_mac: STA }
    }
    fn associated_link_metrics() -> AssociatedStaLinkMetrics {
        AssociatedStaLinkMetrics {
            sta_mac: STA,
            bss_metrics: vec![
                AssociatedStaBssMetrics {
                    bssid: BSSID1,
                    time_delta: 0x0102_0304,
                    mac_rate_down: 866,
                    mac_rate_up: 0x0001_0203,
                    rcpi: 180,
                },
                AssociatedStaBssMetrics {
                    bssid: BSSID2,
                    time_delta: 0x0a0b_0c0d,
                    mac_rate_down: 0x1122_3344,
                    mac_rate_up: 0xfffe_fdfc,
                    rcpi: 96,
                },
            ],
        }
    }
    fn unassociated_query() -> UnassociatedStaLinkMetricsQuery {
        UnassociatedStaLinkMetricsQuery {
            operating_class: 115,
            channels: vec![
                UnassociatedQueryChannel {
                    channel: 36,
                    sta_macs: vec![STA, BSSID1],
                },
                UnassociatedQueryChannel {
                    channel: 44,
                    sta_macs: vec![BSSID2],
                },
            ],
        }
    }
    fn unassociated_response() -> UnassociatedStaLinkMetricsResponse {
        UnassociatedStaLinkMetricsResponse {
            operating_class: 115,
            stations: vec![
                UnassociatedStaMetrics {
                    sta_mac: STA,
                    channel: 36,
                    time_delta: 0x0102_0304,
                    rcpi: 150,
                },
                UnassociatedStaMetrics {
                    sta_mac: BSSID1,
                    channel: 44,
                    time_delta: 0x0a0b_0c0d,
                    rcpi: 88,
                },
            ],
        }
    }
    fn beacon_query() -> BeaconMetricsQuery<'static> {
        BeaconMetricsQuery {
            sta_mac: STA,
            operating_class: 115,
            channel: 255,
            bssid: BSSID1,
            reporting_detail: 1,
            ssid: SSID,
            ap_channel_reports: vec![
                ApChannelReport {
                    operating_class: 115,
                    channels: &[0x24, 0x2c],
                },
                ApChannelReport {
                    operating_class: 81,
                    channels: &[0x06],
                },
            ],
            element_ids: &[0x00, 0x30, 0xdd],
        }
    }
    /// The body of the `upstream-default` rows: one report and no element IDs.
    fn upstream_default_query() -> BeaconMetricsQuery<'static> {
        BeaconMetricsQuery {
            sta_mac: STA,
            operating_class: 115,
            channel: 36,
            bssid: BSSID1,
            reporting_detail: 1,
            ssid: SSID,
            ap_channel_reports: vec![ApChannelReport {
                operating_class: 115,
                channels: &[0x24],
            }],
            element_ids: &[],
        }
    }
    fn beacon_response() -> BeaconMetricsResponse<'static> {
        BeaconMetricsResponse {
            sta_mac: STA,
            reports: vec![
                MeasurementReportElement {
                    element_id: 39,
                    body: &BEACON_REPORT_BODY,
                },
                MeasurementReportElement {
                    element_id: 39,
                    body: &[0x02, 0x04, 0x05],
                },
            ],
        }
    }
    fn traffic_stats() -> AssociatedStaTrafficStats {
        AssociatedStaTrafficStats {
            sta_mac: STA,
            bytes_sent: 0x0102_0304,
            bytes_received: 0x0506_0708,
            packets_sent: 0x090a_0b0c,
            packets_received: 0x0d0e_0f10,
            tx_packet_errors: 17,
            rx_packet_errors: 258,
            retransmission_count: 0x00ab_cdef,
        }
    }
    fn collection_interval() -> MetricCollectionInterval {
        MetricCollectionInterval {
            interval: 0x0a0b_0c0d,
        }
    }
    fn radio_metrics() -> RadioMetrics {
        RadioMetrics {
            ruid: RUID,
            noise: 161,
            transmit: 12,
            receive_self: 34,
            receive_other: 56,
        }
    }
    fn ap_extended_metrics() -> ApExtendedMetrics {
        ApExtendedMetrics {
            bssid: STA,
            unicast_bytes_sent: 0x0102_0304,
            unicast_bytes_received: 0x0506_0708,
            multicast_bytes_sent: 0x090a_0b0c,
            multicast_bytes_received: 0x0d0e_0f10,
            broadcast_bytes_sent: 258,
            broadcast_bytes_received: 0x00ab_cdef,
        }
    }
    fn extended_link_metrics() -> AssociatedStaExtendedLinkMetrics {
        AssociatedStaExtendedLinkMetrics {
            sta_mac: STA,
            bss_metrics: vec![
                ExtendedLinkMetricsBss {
                    bssid: BSSID1,
                    last_data_downlink_rate: 0x0102_0304,
                    last_data_uplink_rate: 0x0506_0708,
                    utilization_receive: 0x090a_0b0c,
                    utilization_transmit: 0x0d0e_0f10,
                },
                ExtendedLinkMetricsBss {
                    bssid: BSSID2,
                    last_data_downlink_rate: 866_700,
                    last_data_uplink_rate: 6500,
                    utilization_receive: 258,
                    utilization_transmit: 0xfedc_ba98,
                },
            ],
        }
    }
    fn wifi6_status_report() -> AssociatedWifi6StaStatusReport {
        AssociatedWifi6StaStatusReport {
            sta_mac: STA,
            tids: vec![
                Wifi6TidQueueSize {
                    tid: 0,
                    queue_size: 0x12,
                },
                Wifi6TidQueueSize {
                    tid: 5,
                    queue_size: 0xfe,
                },
                Wifi6TidQueueSize {
                    tid: 7,
                    queue_size: 0x80,
                },
            ],
        }
    }

    fn many_ap_macs() -> ApMetricsQuery {
        ApMetricsQuery {
            ap_macs: vec![STA; 256],
        }
    }
    fn many_link_metrics() -> AssociatedStaLinkMetrics {
        let mut body = associated_link_metrics();
        body.bss_metrics.resize(256, body.bss_metrics[0]);
        body
    }
    fn many_query_channels() -> UnassociatedStaLinkMetricsQuery {
        UnassociatedStaLinkMetricsQuery {
            operating_class: 81,
            channels: (0..256)
                .map(|channel| UnassociatedQueryChannel {
                    channel: u8::try_from(channel % 256).unwrap(),
                    sta_macs: vec![],
                })
                .collect(),
        }
    }
    fn many_stations() -> UnassociatedStaLinkMetricsResponse {
        let mut body = unassociated_response();
        body.stations.resize(256, body.stations[0]);
        body
    }
    fn many_beacon_reports() -> BeaconMetricsQuery<'static> {
        let mut body = beacon_query();
        body.ap_channel_reports
            .resize(256, body.ap_channel_reports[0]);
        body
    }
    fn many_measurement_reports() -> BeaconMetricsResponse<'static> {
        let mut body = beacon_response();
        body.reports.resize(256, body.reports[0]);
        body
    }
    fn many_extended_link_metrics() -> AssociatedStaExtendedLinkMetrics {
        let mut body = extended_link_metrics();
        body.bss_metrics.resize(256, body.bss_metrics[0]);
        body
    }
    fn many_tids() -> AssociatedWifi6StaStatusReport {
        let mut body = wifi6_status_report();
        body.tids.resize(256, body.tids[0]);
        body
    }

    body_checks! {
        mod ap_metrics_query_checks;
        body: ApMetricsQuery;
        golden: &fixture(("0x93", "golden")), ap_metrics_query();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,6), field(7,6)]);
        trailing_byte: &fixture(("0x93", "golden"));
        count_too_large: many_ap_macs(), TOO_MANY;
    }
    body_checks! {
        mod ap_metrics_checks;
        body: ApMetrics;
        golden: &fixture(("0x94", "golden")), ap_metrics();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,2), field(9,1), field(10,3), field(13,3), field(16,3), field(19,3)]);
        trailing_byte: &fixture(("0x94", "golden"));
        reserved_bits: [
            (&fixture(("0x94", "reserved-bits-set")), ApMetrics::decode_value(&fixture(("0x94", "reserved-bits-clear"))).unwrap(), &fixture(("0x94", "reserved-bits-clear"))),
            (&fixture(("0x94", "reserved-bits-set-zero-flags")), zero_flags_ap_metrics(), &with_flags_octet(&fixture(("0x94", "reserved-bits-set-zero-flags")), 0x00))
        ];
        lenient_absent: [&fixture(("0x94", "empty"))];
    }
    body_checks! {
        mod sta_mac_address_checks;
        body: StaMacAddress;
        golden: &fixture(("0x95", "golden")), sta_mac_address();
        prefixes: |_| false, truncated_in(&[field(0,6)]);
        trailing_byte: &fixture(("0x95", "golden"));
    }
    body_checks! {
        mod associated_link_metrics_checks;
        body: AssociatedStaLinkMetrics;
        golden: &fixture(("0x96", "golden")), associated_link_metrics();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,6), field(13,4), field(17,4), field(21,4), field(25,1), field(26,6), field(32,4), field(36,4), field(40,4), field(44,1)]);
        trailing_byte: &fixture(("0x96", "golden"));
        count_too_large: many_link_metrics(), TOO_MANY;
    }
    body_checks! {
        mod unassociated_query_checks;
        body: UnassociatedStaLinkMetricsQuery;
        golden: &fixture(("0x97", "golden")), unassociated_query();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,1), field(2,1), field(3,1), field(4,6), field(10,6), field(16,1), field(17,1), field(18,6)]);
        trailing_byte: &fixture(("0x97", "golden"));
        count_too_large: many_query_channels(), TOO_MANY;
    }
    body_checks! {
        mod unassociated_response_checks;
        body: UnassociatedStaLinkMetricsResponse;
        golden: &fixture(("0x98", "golden")), unassociated_response();
        prefixes: |_| false, truncated_in(&[field(0,1), field(1,1), field(2,6), field(8,1), field(9,4), field(13,1), field(14,6), field(20,1), field(21,4), field(25,1)]);
        trailing_byte: &fixture(("0x98", "golden"));
        count_too_large: many_stations(), TOO_MANY;
    }
    body_checks! {
        mod beacon_query_checks;
        body: BeaconMetricsQuery<'a>;
        golden: &fixture(("0x99", "golden")), beacon_query();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,1), field(8,6), field(14,1), field(15,1), field(16,7), field(23,1), field(24,1), field(25,1), field(26,2), field(28,1), field(29,1), field(30,1), field(31,1), field(32,3)]);
        trailing_byte: &fixture(("0x99", "golden"));
        count_too_large: many_beacon_reports(), TOO_MANY;
        lenient: [(&fixture(("0x99", "upstream-default")), upstream_default_query(), &fixture(("0x99", "upstream-default-spec")))];
        lenient_rejects: [
            // Ends inside the first channel list: a shortfall before the report group is complete.
            (&fixture(("0x99", "golden"))[..27], DecodeError::Truncated { needed: 2, remaining: 1 }),
            // Ends before the second report's channel list.
            (&fixture(("0x99", "golden"))[..30], DecodeError::Truncated { needed: 1, remaining: 0 }),
            // Num_Ids is present, so the element list must follow.
            (&fixture(("0x99", "golden"))[..32], DecodeError::Truncated { needed: 3, remaining: 0 }),
            // Ends after the SSID, before Num_Report.
            (&fixture(("0x99", "missing-num-report")), DecodeError::Truncated { needed: 1, remaining: 0 }),
            // Num_Report promises a second report.
            (&fixture(("0x99", "reports-overstated")), DecodeError::Truncated { needed: 1, remaining: 0 }),
            (&fixture(("0x99", "report-len-0")), DecodeError::InvalidLength { len: 0 })
        ];
    }
    body_checks! {
        mod beacon_response_checks;
        body: BeaconMetricsResponse<'a>;
        golden: &fixture(("0x9a", "golden")), beacon_response();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,1), field(8,1), field(9,1), field(10,29), field(39,1), field(40,1), field(41,3)]);
        trailing_byte: &fixture(("0x9a", "golden"));
        count_too_large: many_measurement_reports(), TOO_MANY;
        reserved_bits: [(&fixture(("0x9a", "reserved-set")), BeaconMetricsResponse::decode_value(&fixture(("0x9a", "reserved-clear"))).unwrap(), &fixture(("0x9a", "reserved-clear")))];
    }
    body_checks! {
        mod traffic_stats_checks;
        body: AssociatedStaTrafficStats;
        golden: &fixture(("0xa2", "golden")), traffic_stats();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,4), field(10,4), field(14,4), field(18,4), field(22,4), field(26,4), field(30,4)]);
        trailing_byte: &fixture(("0xa2", "golden"));
    }
    body_checks! {
        mod collection_interval_checks;
        body: MetricCollectionInterval;
        golden: &fixture(("0xc5", "golden")), collection_interval();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0,4)]);
        trailing_byte: &fixture(("0xc5", "golden"));
        lenient_absent: [&fixture(("0xc5", "empty"))];
    }
    body_checks! {
        mod radio_metrics_checks;
        body: RadioMetrics;
        golden: &fixture(("0xc6", "golden")), radio_metrics();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,1), field(8,1), field(9,1)]);
        trailing_byte: &fixture(("0xc6", "golden"));
    }
    body_checks! {
        mod ap_extended_metrics_checks;
        body: ApExtendedMetrics;
        golden: &fixture(("0xc7", "golden")), ap_extended_metrics();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,4), field(10,4), field(14,4), field(18,4), field(22,4), field(26,4)]);
        trailing_byte: &fixture(("0xc7", "golden"));
    }
    body_checks! {
        mod extended_link_metrics_checks;
        body: AssociatedStaExtendedLinkMetrics;
        golden: &fixture(("0xc8", "golden")), extended_link_metrics();
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,6), field(13,4), field(17,4), field(21,4), field(25,4), field(29,6), field(35,4), field(39,4), field(43,4), field(47,4)]);
        trailing_byte: &fixture(("0xc8", "golden"));
        count_too_large: many_extended_link_metrics(), TOO_MANY;
    }
    body_checks! {
        mod wifi6_status_report_checks;
        body: AssociatedWifi6StaStatusReport;
        golden: &fixture(("0xb0", "golden")), wifi6_status_report();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0,6), field(6,1), field(7,1), field(8,1), field(9,1), field(10,1), field(11,1), field(12,1)]);
        trailing_byte: &fixture(("0xb0", "golden"));
        count_too_large: many_tids(), TOO_MANY;
        lenient_rejects: [
            // One octet short of upstream's form: spec bytes plus one stray octet.
            (&[0; 8], DecodeError::TrailingBytes { remaining: 1 }),
            // One octet over upstream's form.
            (&[0; 10], DecodeError::TrailingBytes { remaining: 3 })
        ];
    }

    /// The `reserved-bits-set-zero-flags` row decodes to this: no flag is set, the BE field is
    /// still on the wire, and the reserved bits are gone.
    fn zero_flags_ap_metrics() -> ApMetrics {
        ApMetrics {
            bssid: STA,
            channel_utilization: 12,
            station_count: 258,
            include_be: false,
            esp_be: ESP_BE,
            esp_bk: None,
            esp_vo: None,
            esp_vi: None,
        }
    }

    /// `bytes` of an AP Metrics value with the flags octet (offset 9) replaced.
    fn with_flags_octet(bytes: &[u8], flags: u8) -> Vec<u8> {
        let mut changed = bytes.to_vec();
        changed[9] = flags;
        changed
    }

    /// A flags octet mask, whether its flag adds a field, and the setter of the matching field.
    type FlagCase = (u8, bool, fn(&mut ApMetrics, [u8; 3]));

    /// An AP Metrics value of zeros after the BSSID, with no flag set.
    fn zero_ap_metrics_bytes() -> Vec<u8> {
        let mut bytes = STA.octets().to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    #[test]
    fn ap_metrics_flags_each_use_their_own_bit() {
        let zero = ApMetrics {
            bssid: STA,
            channel_utilization: 0,
            station_count: 0,
            include_be: false,
            esp_be: [0; 3],
            esp_bk: None,
            esp_vo: None,
            esp_vi: None,
        };
        let zero_bytes = zero_ap_metrics_bytes();
        assert_eq!(ApMetrics::decode_value(&zero_bytes), Ok(zero));
        assert_eq!(zero.encode_value().as_deref(), Ok(zero_bytes.as_slice()));

        let esp = [0x11, 0x22, 0x33];
        let cases: [FlagCase; 4] = [
            (0x80, false, |body, _| body.include_be = true),
            (0x40, true, |body, esp| body.esp_bk = Some(esp)),
            (0x20, true, |body, esp| body.esp_vo = Some(esp)),
            (0x10, true, |body, esp| body.esp_vi = Some(esp)),
        ];
        for (mask, has_field, set) in cases {
            let mut bytes = zero_ap_metrics_bytes();
            bytes[9] = mask;
            if has_field {
                bytes.extend_from_slice(&esp);
            }
            let mut expected = zero;
            set(&mut expected, esp);
            assert_eq!(
                ApMetrics::decode_value(&bytes),
                Ok(expected),
                "decode, mask {mask:#04x}"
            );
            assert_eq!(
                expected.encode_value(),
                Ok(bytes),
                "encode, mask {mask:#04x}"
            );
        }
    }

    #[test]
    fn ap_metrics_reserved_flag_bits_are_ignored_and_written_as_zero() {
        let mut bytes = STA.octets().to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0x0f, 0, 0, 0]);
        let body = ApMetrics::decode_value(&bytes).unwrap();
        assert!(!body.include_be && body.esp_bk.is_none());
        assert_eq!(body.encode_value().unwrap()[9], 0);
    }

    #[test]
    fn ap_metrics_optional_fields_follow_in_bk_vo_vi_order() {
        // VI and BK set, VO clear: BK comes first on the wire although VI is the lower bit.
        let mut body = ap_metrics();
        body.esp_vo = None;
        let bytes = body.encode_value().unwrap();
        assert_eq!(bytes[9], 0x90 | 0x40);
        assert_eq!(&bytes[13..16], &ESP_BK);
        assert_eq!(&bytes[16..19], &ESP_VI);
        assert_eq!(ApMetrics::decode_value(&bytes), Ok(body));
    }

    #[test]
    fn ap_metrics_include_be_clear_is_kept_and_written_back() {
        // Upstream checks neither the flag nor the length (audit row 87); the spec says the flag
        // shall be 1, but the BE field is on the wire either way.
        let bytes = fixture(("0x94", "include-be-clear"));
        let body = ApMetrics::decode_value(&bytes).unwrap();
        assert!(!body.include_be);
        assert_eq!(body.esp_be, [0; 3]);
        assert_eq!(body.encode_value(), Ok(bytes));
    }

    #[test]
    fn ap_metrics_empty_value_is_upstreams_absent_tlv() {
        // intentional-difference: upstream em_metrics.cpp:1946 sends an empty AP Metrics value
        // for a radio that was not requested (audit row 85); only the lenient decoder accepts it.
        assert_eq!(ApMetrics::decode_value_lenient(&[]), Ok(None));
        assert!(ApMetrics::decode_value(&[]).is_err());
    }

    #[test]
    fn metric_collection_interval_is_little_endian_like_upstream() {
        // Audit row 195 (UPSTREAM): upstream copies the host-order value; every other
        // multi-octet field of this group is big-endian.
        let body = MetricCollectionInterval { interval: 1000 };
        assert_eq!(body.encode_value(), Ok(vec![0xe8, 0x03, 0x00, 0x00]));
        assert_eq!(
            MetricCollectionInterval::decode_value(&[0x0d, 0x0c, 0x0b, 0x0a]),
            Ok(collection_interval())
        );
        assert_ne!(
            MetricCollectionInterval::decode_value(&[0x0a, 0x0b, 0x0c, 0x0d]),
            Ok(collection_interval())
        );
    }

    #[test]
    fn metric_collection_interval_empty_value_is_absent() {
        // intentional-difference: upstream em.cpp:1886-1896 sends an empty Metric Collection
        // Interval value without radio capability information; only the lenient decoder accepts it.
        assert_eq!(
            MetricCollectionInterval::decode_value_lenient(&[]),
            Ok(None)
        );
        assert_eq!(
            MetricCollectionInterval::decode_value_lenient(&[1, 0, 0]),
            Err(DecodeError::Truncated {
                needed: 4,
                remaining: 3
            })
        );
    }

    #[test]
    fn beacon_query_without_num_ids_is_lenient_only_and_equals_the_spec_sibling() {
        // intentional-difference: upstream em_metrics.cpp:416-418 accepts a Beacon Metrics Query
        // value that ends without Num_Ids (audit rows 101, 105); only the lenient decoder does here.
        let mut checked = 0;
        for row in body_fixtures::all_rows(GROUP) {
            if row.tlv != "0x99" || row.kind() != Kind::Lenient {
                continue;
            }
            assert_eq!(row.field("decoded"), Some("num-ids-zero"), "{}", row.case);
            let sibling_case = format!("{}-spec", row.case);
            let sibling_bytes = fixtures_by_case("0x99", &sibling_case);
            let strict_sibling = BeaconMetricsQuery::decode_value(&sibling_bytes).unwrap();
            let bytes = row.bytes();
            assert_eq!(
                BeaconMetricsQuery::decode_value(&bytes),
                Err(DecodeError::Truncated {
                    needed: 1,
                    remaining: 0
                }),
                "{}",
                row.case
            );
            let lenient = BeaconMetricsQuery::decode_value_lenient(&bytes).unwrap();
            assert_eq!(lenient.as_ref(), Some(&strict_sibling), "{}", row.case);
            assert_eq!(strict_sibling.encode_value().unwrap(), sibling_bytes);
            checked += 1;
        }
        assert_eq!(checked, 5);
    }

    /// The bytes of a fixture row whose case name is built at run time.
    fn fixtures_by_case(tlv: &str, case: &str) -> Vec<u8> {
        body_fixtures::all_rows(GROUP)
            .into_iter()
            .find(|row| row.tlv == tlv && row.case == case)
            .unwrap_or_else(|| panic!("missing fixture {tlv}/{case}"))
            .bytes()
    }

    #[test]
    fn beacon_query_report_length_zero_is_invalid_length() {
        let bytes = fixture(("0x99", "report-len-0"));
        assert_eq!(
            BeaconMetricsQuery::decode_value(&bytes),
            Err(DecodeError::InvalidLength { len: 0 })
        );
    }

    #[test]
    fn beacon_query_value_rules_are_for_the_consumer() {
        // Reports with a Channel Number other than 255, Num_Ids with Reporting Detail 0, and the
        // reserved Reporting Details are carried as given.
        for case in [
            "upstream-default-spec",
            "ids-with-detail-0",
            "detail-reserved-3",
            "detail-reserved-ff",
        ] {
            let bytes = fixtures_by_case("0x99", case);
            let body = BeaconMetricsQuery::decode_value(&bytes).unwrap();
            assert_eq!(body.encode_value(), Ok(bytes), "{case}");
        }
    }

    #[test]
    fn beacon_query_lengths_that_do_not_fit_are_refused_on_encode() {
        let ssid = [b'x'; 256];
        let mut body = beacon_query();
        body.ssid = &ssid;
        assert_eq!(body.encode_value(), Err(TOO_MANY));

        let channels = [36; 255];
        let mut body = beacon_query();
        body.ap_channel_reports[0].channels = &channels;
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::CountTooLarge { len: 255, max: 254 })
        );
        // 254 channels plus the operating class is the largest Report Length.
        body.ap_channel_reports[0].channels = &channels[..254];
        let bytes = body.encode_value().unwrap();
        assert_eq!(BeaconMetricsQuery::decode_value(&bytes), Ok(body));

        let element_ids = [0; 256];
        let mut body = beacon_query();
        body.element_ids = &element_ids;
        assert_eq!(body.encode_value(), Err(TOO_MANY));
    }

    #[test]
    fn beacon_response_keeps_any_element_id_and_checks_element_lengths() {
        let other = fixture(("0x9a", "other-element-id"));
        let body = BeaconMetricsResponse::decode_value(&other).unwrap();
        assert_eq!(body.reports[0].element_id, 0xdd);
        assert_eq!(body.encode_value(), Ok(other));

        let big = [0; 256];
        let mut body = beacon_response();
        body.reports[0].body = &big;
        assert_eq!(body.encode_value(), Err(TOO_MANY));
    }

    #[test]
    fn nested_counts_are_checked_before_encoding() {
        let mut body = unassociated_query();
        body.channels[0].sta_macs = vec![STA; 256];
        assert_eq!(body.encode_value(), Err(TOO_MANY));
    }

    #[test]
    fn largest_counts_round_trip() {
        // intentional-difference (0x97 stations): upstream's receiver rejects the whole query
        // above 64 stations per channel (em_metrics.cpp:2607-2610, EM_MAX_STA_PER_CHANNEL at
        // em_base.h:238; audit row 96); 255 stations round-trip here.
        let body = ApMetricsQuery {
            ap_macs: vec![STA; 255],
        };
        let bytes = body.encode_value().unwrap();
        assert_eq!(bytes.len(), 1 + 255 * 6);
        assert_eq!(ApMetricsQuery::decode_value(&bytes), Ok(body));

        let body = UnassociatedStaLinkMetricsQuery {
            operating_class: 81,
            channels: vec![UnassociatedQueryChannel {
                channel: 1,
                sta_macs: vec![STA; 255],
            }],
        };
        let bytes = body.encode_value().unwrap();
        assert_eq!(
            UnassociatedStaLinkMetricsQuery::decode_value(&bytes),
            Ok(body)
        );
    }

    #[test]
    fn zero_bssid_counts_decode_where_upstreams_validator_wants_one() {
        // intentional-difference: upstream's message validator (em_msg.cpp:953, 975, 977; the
        // mandatory min-length check at :683-688) requires room for one BSSID in AP Metrics Query,
        // Associated STA Link Metrics and Associated STA Extended Link Metrics (audit rows 84,
        // 94, 202); a count of zero is spec-valid and these bodies accept it (note 0009 D2).
        let value = fixture(("0x93", "zero-bssids"));
        let body = ApMetricsQuery::decode_value(&value).unwrap();
        assert_eq!(body.encode_value(), Ok(value));
        let value = fixture(("0x96", "zero-bssids"));
        let body = AssociatedStaLinkMetrics::decode_value(&value).unwrap();
        assert_eq!(body.encode_value(), Ok(value));
        let value = fixture(("0xc8", "zero-bssids"));
        let body = AssociatedStaExtendedLinkMetrics::decode_value(&value).unwrap();
        assert_eq!(body.encode_value(), Ok(value));
    }

    #[test]
    fn unassociated_query_accepts_more_than_16_channels_where_upstream_stops() {
        // intentional-difference: upstream's receiver rejects the whole Unassociated STA Link
        // Metrics Query above 16 channels (em_metrics.cpp:2581-2584, EM_MAX_CHANNELS_PER_OPCLASS at
        // em_base.h:237; audit row 96); the count here runs to its full wire range of 255.
        for count in [17_u8, 255] {
            let body = UnassociatedStaLinkMetricsQuery {
                operating_class: 81,
                channels: (1..=count)
                    .map(|channel| UnassociatedQueryChannel {
                        channel,
                        sta_macs: vec![STA],
                    })
                    .collect(),
            };
            let bytes = body.encode_value().unwrap();
            let decoded = UnassociatedStaLinkMetricsQuery::decode_value(&bytes).unwrap();
            assert_eq!(decoded.channels.len(), usize::from(count));
            assert_eq!(decoded, body);
            assert_eq!(decoded.encode_value().unwrap(), bytes);
        }
    }

    #[test]
    fn unassociated_response_keeps_entries_after_64_where_upstream_drops() {
        // intentional-difference: upstream em_metrics.cpp:503-509 (EM_MAX_UNASSOC_STA, em_base.h:650)
        // drops Unassociated STA Link Metrics Response entries after 64 (audit row 100); this body
        // keeps every entry.
        for count in [65_usize, 255] {
            let mut body = unassociated_response();
            body.stations.resize(count, body.stations[0]);
            let bytes = body.encode_value().unwrap();
            let decoded = UnassociatedStaLinkMetricsResponse::decode_value(&bytes).unwrap();
            assert_eq!(decoded.stations.len(), count);
            assert_eq!(decoded, body);
            assert_eq!(decoded.encode_value().unwrap(), bytes);
        }
    }

    #[test]
    fn wifi6_status_report_upstream_nine_zero_octets_is_lenient_absent() {
        // intentional-difference: upstream em_metrics.cpp:2299-2306 sends nine zero octets as the
        // Wi-Fi 6 STA Status Report; the strict decoder rejects them, the lenient one gives None.
        // A7 exception: the bytes upstream sends (`em_metrics.cpp:2299-2306`) are not spec bytes
        // (one STA MAC of zeros, no TIDs, two stray octets) and mean "no report".
        assert_eq!(
            AssociatedWifi6StaStatusReport::decode_value(&[0; 9]),
            Err(DecodeError::TrailingBytes { remaining: 2 })
        );
        assert_eq!(
            AssociatedWifi6StaStatusReport::decode_value_lenient(&[0; 9]),
            Ok(None)
        );
        // The exception is for exactly those nine octets; the spec form with a zero MAC and no
        // TIDs is still a body, and so are nine octets that are not all zero.
        let empty_report = AssociatedWifi6StaStatusReport {
            sta_mac: ZERO_MAC,
            tids: vec![],
        };
        assert_eq!(
            AssociatedWifi6StaStatusReport::decode_value_lenient(&[0; 7]),
            Ok(Some(empty_report))
        );
        let mut nonzero = [0; 9];
        nonzero[8] = 1;
        assert_eq!(
            AssociatedWifi6StaStatusReport::decode_value_lenient(&nonzero),
            Err(DecodeError::TrailingBytes { remaining: 2 })
        );
        assert_eq!(
            AssociatedWifi6StaStatusReport::decode_value_lenient(&[]),
            Err(DecodeError::Truncated {
                needed: 6,
                remaining: 0
            })
        );
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<ApMetricsQuery>() <= 3 * word);
        assert!(size_of::<ApMetrics>() <= 40);
        assert!(size_of::<StaMacAddress>() <= 6);
        assert!(size_of::<AssociatedStaBssMetrics>() <= 20);
        assert!(size_of::<AssociatedStaLinkMetrics>() <= 3 * word + 8);
        assert!(size_of::<UnassociatedQueryChannel>() <= 4 * word);
        assert!(size_of::<UnassociatedStaLinkMetricsQuery>() <= 4 * word);
        assert!(size_of::<UnassociatedStaMetrics>() <= 16);
        assert!(size_of::<UnassociatedStaLinkMetricsResponse>() <= 4 * word);
        assert!(size_of::<ApChannelReport<'_>>() <= 3 * word);
        assert!(size_of::<BeaconMetricsQuery<'_>>() <= 10 * word);
        assert!(size_of::<MeasurementReportElement<'_>>() <= 3 * word);
        assert!(size_of::<BeaconMetricsResponse<'_>>() <= 4 * word);
        assert!(size_of::<AssociatedStaTrafficStats>() <= 40);
        assert!(size_of::<MetricCollectionInterval>() <= 4);
        assert!(size_of::<RadioMetrics>() <= 10);
        assert!(size_of::<ApExtendedMetrics>() <= 32);
        assert!(size_of::<ExtendedLinkMetricsBss>() <= 24);
        assert!(size_of::<AssociatedStaExtendedLinkMetrics>() <= 4 * word);
        assert!(size_of::<Wifi6TidQueueSize>() <= 2);
        assert!(size_of::<AssociatedWifi6StaStatusReport>() <= 4 * word);
    }
}
