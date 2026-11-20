// SPDX-License-Identifier: Apache-2.0

//! `EasyMesh` channel selection and channel scan TLV bodies.

use em_types::{MacAddr, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

fn checked_bits(value: u8, max: u8) -> Result<u8, EncodeError> {
    if value > max {
        return Err(EncodeError::ValueTooLarge {
            value: u32::from(value),
            max: u32::from(max),
        });
    }
    Ok(value)
}

fn read_octets<'a>(reader: &mut Reader<'a>) -> Result<&'a [u8], DecodeError> {
    let len = usize::from(reader.u8()?);
    reader.bytes(len)
}

fn write_octets(writer: &mut Writer, value: &[u8]) -> Result<(), EncodeError> {
    writer.count_u8(value.len())?;
    writer.bytes(value);
    Ok(())
}

/// One operating-class preference, `EasyMesh` §17.2.13, Table 35.
///
/// The upstream record is `em_channel_pref_op_class_t` (`em_base.h:1455-1459`);
/// see [`ChannelPreference`] for sender, receiver and audit details. An empty
/// channel list means that the preference applies to all channels in the class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreferenceOperatingClass<'a> {
    /// Global operating class.
    pub operating_class: u8,
    /// Channel numbers, or an empty list for all channels.
    pub channels: &'a [u8],
    /// Four-bit preference, including reserved value 15.
    pub preference: u8,
    /// Four-bit reason, including reserved values.
    pub reason: u8,
}

/// Channel Preference, `EasyMesh` §17.2.13, Table 35; audit rows 61–66.
///
/// Structs `em_channel_pref_t` and `em_channel_pref_op_class_t` are in
/// `em_base.h:1455-1465`. Senders: `em_channel.cpp:58-107,236-329`;
/// receivers: `em_channel.cpp:1776-1872,1920-2004`. Unlike those receivers,
/// the preference octet is retained for an empty channel list (row 61),
/// counts are not capped at 64 and all fields are bounds-checked (rows 62–64).
/// Reserved preference and reason values are carried raw (0009 D2; row 65),
/// although the controller sender skips preference 15 (`em_channel.cpp:302`).
#[derive(Debug, PartialEq, Eq)]
pub struct ChannelPreference<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Operating-class preferences, in wire order.
    pub operating_classes: Vec<PreferenceOperatingClass<'a>>,
}

impl<'a> TlvBody<'a> for ChannelPreference<'a> {
    const TLV_TYPE: TlvType = TlvType::CHANNEL_PREFERENCE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut operating_classes = bounded_vec(count, reader.remaining(), 3);
        for _ in 0..count {
            let operating_class = reader.u8()?;
            let channels = read_octets(&mut reader)?;
            let bits = reader.u8()?;
            operating_classes.push(PreferenceOperatingClass {
                operating_class,
                channels,
                preference: bits >> 4,
                reason: bits & 0x0f,
            });
        }
        reader.finish()?;
        Ok(Self {
            ruid,
            operating_classes,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.count_u8(self.operating_classes.len())?;
        for class in &self.operating_classes {
            writer.u8(class.operating_class);
            write_octets(&mut writer, class.channels)?;
            writer.u8((checked_bits(class.preference, 15)? << 4) | checked_bits(class.reason, 15)?);
        }
        Ok(writer.into_bytes())
    }
}

/// A channel and minimum separation, `EasyMesh` §17.2.14, Table 36.
///
/// The upstream unused struct is `em_radio_op_restrict_channel_t`
/// (`em_base.h:1490-1493`); see [`RadioOperationRestriction`], audit row 67.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestrictedChannel {
    /// Channel number.
    pub channel: u8,
    /// Minimum frequency separation in units of 10 MHz.
    pub separation: u8,
}

/// An operating class with restricted channels, `EasyMesh` §17.2.14, Table 36.
///
/// The upstream unused struct is `em_radio_op_restrict_op_class_t`
/// (`em_base.h:1495-1499`); see [`RadioOperationRestriction`], audit row 67.
#[derive(Debug, PartialEq, Eq)]
pub struct RestrictedOperatingClass {
    /// Global operating class.
    pub operating_class: u8,
    /// Restricted channels in wire order.
    pub channels: Vec<RestrictedChannel>,
}

/// Radio Operation Restriction, `EasyMesh` §17.2.14, Table 36; audit row 67.
///
/// Spec-only, no upstream reference sender or receiver. The unused structs are
/// `em_radio_op_restriction_t` and its records (`em_base.h:1490-1505`).
#[derive(Debug, PartialEq, Eq)]
pub struct RadioOperationRestriction {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Restricted operating classes in wire order.
    pub operating_classes: Vec<RestrictedOperatingClass>,
}

impl TlvBody<'_> for RadioOperationRestriction {
    const TLV_TYPE: TlvType = TlvType::RADIO_OPERATION_RESTRICTION;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut operating_classes = bounded_vec(count, reader.remaining(), 2);
        for _ in 0..count {
            let operating_class = reader.u8()?;
            let channel_count = usize::from(reader.u8()?);
            let mut channels = bounded_vec(channel_count, reader.remaining(), 2);
            for _ in 0..channel_count {
                channels.push(RestrictedChannel {
                    channel: reader.u8()?,
                    separation: reader.u8()?,
                });
            }
            operating_classes.push(RestrictedOperatingClass {
                operating_class,
                channels,
            });
        }
        reader.finish()?;
        Ok(Self {
            ruid,
            operating_classes,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.count_u8(self.operating_classes.len())?;
        for class in &self.operating_classes {
            writer.u8(class.operating_class);
            writer.count_u8(class.channels.len())?;
            for channel in &class.channels {
                writer.u8(channel.channel);
                writer.u8(channel.separation);
            }
        }
        Ok(writer.into_bytes())
    }
}

/// Transmit Power Limit, `EasyMesh` §17.2.15, Table 37; audit rows 68–71.
///
/// Struct `em_tx_power_limit_t` is at `em_base.h:1467-1470`; sender
/// `em_channel.cpp:557-572`, receiver `em_channel.cpp:2033-2035` and consumer
/// `dm_easy_mesh_agent.cpp:454-463,588-595`. The codec interprets power as
/// signed, accepts zero, and requires exactly seven octets (rows 68, 69, 71).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransmitPowerLimit {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Transmit power limit in signed dBm.
    pub tx_power_limit: i8,
}

impl TlvBody<'_> for TransmitPowerLimit {
    const TLV_TYPE: TlvType = TlvType::TRANSMIT_POWER_LIMIT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let body = Self {
            ruid: reader.mac()?,
            tx_power_limit: i8::from_ne_bytes([reader.u8()?]),
        };
        reader.finish()?;
        Ok(body)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(self.tx_power_limit.to_ne_bytes()[0]);
        Ok(writer.into_bytes())
    }
}

/// Channel Selection Response, `EasyMesh` §17.2.16, Table 38; audit rows 72–74.
///
/// Struct `em_channel_sel_rsp_t` is at `em_base.h:1446-1449`; sender
/// `em_channel.cpp:1053-1110`, receiver `em_channel.cpp:2055-2095`. The
/// codec retains the response code that the receiver ignores (row 72), checks
/// the exact seven-octet value (row 73), and preserves reserved codes (0009 D2).
/// The sender copies the first byte of an enum (`:1097`); this codec writes the
/// octet explicitly, including on big-endian hosts (row 74).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSelectionResponse {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Response code, including reserved values.
    pub response_code: u8,
}

impl TlvBody<'_> for ChannelSelectionResponse {
    const TLV_TYPE: TlvType = TlvType::CHANNEL_SELECTION_RESPONSE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let body = Self {
            ruid: reader.mac()?,
            response_code: reader.u8()?,
        };
        reader.finish()?;
        Ok(body)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(self.response_code);
        Ok(writer.into_bytes())
    }
}

/// Current class and channel, `EasyMesh` §17.2.17, Table 39.
///
/// Struct `em_op_class_ch_rprt_t` is at `em_base.h:1373-1376`; see
/// [`OperatingChannelReport`] for sender, receiver and audit rows 75–76.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatingChannel {
    /// Global operating class.
    pub operating_class: u8,
    /// Current channel number.
    pub channel: u8,
}

/// Operating Channel Report, `EasyMesh` §17.2.17, Table 39; audit rows 75–76.
///
/// Structs `em_op_channel_rprt_t` and `em_op_class_ch_rprt_t` are at
/// `em_base.h:1373-1381`; sender `em_channel.cpp:1123-1164`, receiver
/// `em_channel.cpp:1686-1750`. All class/channel pairs are kept (row 75),
/// and zero classes are accepted even though the receiver rejects them (row 76).
#[derive(Debug, PartialEq, Eq)]
pub struct OperatingChannelReport {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Current operating classes and channels.
    pub operating_classes: Vec<OperatingChannel>,
    /// Current transmit power in signed dBm.
    pub tx_power: i8,
}

impl TlvBody<'_> for OperatingChannelReport {
    const TLV_TYPE: TlvType = TlvType::OPERATING_CHANNEL_REPORT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut operating_classes = bounded_vec(count, reader.remaining(), 2);
        for _ in 0..count {
            operating_classes.push(OperatingChannel {
                operating_class: reader.u8()?,
                channel: reader.u8()?,
            });
        }
        let tx_power = i8::from_ne_bytes([reader.u8()?]);
        reader.finish()?;
        Ok(Self {
            ruid,
            operating_classes,
            tx_power,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.count_u8(self.operating_classes.len())?;
        for class in &self.operating_classes {
            writer.u8(class.operating_class);
            writer.u8(class.channel);
        }
        writer.u8(self.tx_power.to_ne_bytes()[0]);
        Ok(writer.into_bytes())
    }
}

/// Channel Scan Reporting Policy, `EasyMesh` §17.2.37, Table 59; audit rows 127–128.
///
/// Struct `em_channel_scan_rprt_policy_t` is at `em_base.h:2142-2145`;
/// sender `em_policy_cfg.cpp:273-302,590-599`, receiver `:821-828`. Strict
/// values contain exactly one octet (0009 D3 row 128). The sender may write
/// an empty value when no policy exists; lenient decoding maps it to `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelScanReportingPolicy {
    /// Report independent channel scans.
    pub independent: bool,
}

impl TlvBody<'_> for ChannelScanReportingPolicy {
    const TLV_TYPE: TlvType = TlvType::CHANNEL_SCAN_REPORTING_POLICY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let independent = reader.u8()? & 0x80 != 0;
        reader.finish()?;
        Ok(Self { independent })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        Ok(vec![u8::from(self.independent) << 7])
    }
}

impl TlvBodyLenient<'_> for ChannelScanReportingPolicy {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// A scan operating class and channel list, `EasyMesh` §17.2.38 Table 60 and
/// §17.2.39 Table 61.
///
/// Structs `em_scan_cap_op_class_info_t` (`em_base.h:536-540`) and
/// `em_channel_scan_req_op_class_t` (`em_base.h:1115-1119`); senders
/// `em.cpp:1748-1765`, `em_channel.cpp:199-229`; receivers
/// `em_capability.cpp:1421-1457`, `em_channel.cpp:2217-2269`. Counts use their
/// full wire width (audit rows 133, 137), with zero channels meaning all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOperatingClass<'a> {
    /// Global operating class.
    pub operating_class: u8,
    /// Channel numbers, or an empty list for all channels.
    pub channels: &'a [u8],
}

fn read_scan_classes<'a>(
    reader: &mut Reader<'a>,
    count: usize,
) -> Result<Vec<ScanOperatingClass<'a>>, DecodeError> {
    let mut classes = bounded_vec(count, reader.remaining(), 2);
    for _ in 0..count {
        classes.push(ScanOperatingClass {
            operating_class: reader.u8()?,
            channels: read_octets(reader)?,
        });
    }
    Ok(classes)
}

fn write_scan_classes(
    writer: &mut Writer,
    classes: &[ScanOperatingClass<'_>],
) -> Result<(), EncodeError> {
    writer.count_u8(classes.len())?;
    for class in classes {
        writer.u8(class.operating_class);
        write_octets(writer, class.channels)?;
    }
    Ok(())
}

/// Scan capabilities of one radio, `EasyMesh` §17.2.38, Table 60.
///
/// Struct `em_channel_scan_cap_radio_t` is at `em_base.h:2063-2071`;
/// see [`ChannelScanCapabilities`] for sender, receiver and audit rows 129–133.
#[derive(Debug, PartialEq, Eq)]
pub struct ScanCapabilityRadio<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// The radio can scan only on boot.
    pub on_boot_only: bool,
    /// Two-bit scan impact code.
    pub scan_impact: u8,
    /// Minimum interval between scans, in seconds.
    pub minimum_scan_interval: u32,
    /// Scan-capable operating classes.
    pub operating_classes: Vec<ScanOperatingClass<'a>>,
}

/// Channel Scan Capabilities, `EasyMesh` §17.2.38, Table 60; audit rows 129–133.
///
/// Structs `em_channel_scan_cap_t`, `em_channel_scan_cap_radio_t` are at
/// `em_base.h:2063-2076`; sender `em.cpp:1703-1770` and caller
/// `em_capability.cpp:196-202`, receiver `em_capability.cpp:1352-1462`.
/// All radios and classes are retained without upstream's 3-radio/64-class
/// limits; zero counts are accepted (rows 130, 132–133). Upstream's empty
/// error-path value is accepted only leniently as `None` (0009 A6; row 131).
#[derive(Debug, PartialEq, Eq)]
pub struct ChannelScanCapabilities<'a> {
    /// Radio scan capabilities in wire order.
    pub radios: Vec<ScanCapabilityRadio<'a>>,
}

impl<'a> TlvBody<'a> for ChannelScanCapabilities<'a> {
    const TLV_TYPE: TlvType = TlvType::CHANNEL_SCAN_CAPABILITIES;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 12);
        for _ in 0..count {
            let ruid = reader.mac()?;
            let flags = reader.u8()?;
            let minimum_scan_interval = reader.be_u32()?;
            let class_count = usize::from(reader.u8()?);
            let operating_classes = read_scan_classes(&mut reader, class_count)?;
            radios.push(ScanCapabilityRadio {
                ruid,
                on_boot_only: flags & 0x80 != 0,
                scan_impact: (flags >> 5) & 3,
                minimum_scan_interval,
                operating_classes,
            });
        }
        reader.finish()?;
        Ok(Self { radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer
                .u8((u8::from(radio.on_boot_only) << 7)
                    | (checked_bits(radio.scan_impact, 3)? << 5));
            writer.be_u32(radio.minimum_scan_interval);
            write_scan_classes(&mut writer, &radio.operating_classes)?;
        }
        Ok(writer.into_bytes())
    }
}

impl<'a> TlvBodyLenient<'a> for ChannelScanCapabilities<'a> {
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// One radio in a scan request, `EasyMesh` §17.2.39, Table 61.
///
/// Struct `em_channel_scan_req_t` is at `em_base.h:1121-1128`; see
/// [`ChannelScanRequest`] for sender, receiver and audit rows 134–138.
#[derive(Debug, PartialEq, Eq)]
pub struct ScanRequestRadio<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Requested operating classes; empty for stored results.
    pub operating_classes: Vec<ScanOperatingClass<'a>>,
}

/// Channel Scan Request, `EasyMesh` §17.2.39, Table 61; audit rows 134–138.
///
/// Structs `em_channel_scan_req_t` and `em_channel_scan_req_op_class_t` are
/// at `em_base.h:1115-1128`; sender `em_channel.cpp:178-234`, receiver
/// `:2159-2289`. Multiple radios, counts up to the wire limit, and reserved
/// bits are accepted despite receiver restrictions (rows 135–137). Fresh
/// requests with no classes are accepted (0009 D3 row 138), unlike the sender.
/// A stored-results request with classes is invalid, so fields are private.
/// Decoding one gives `ValueOutOfRange`, as upstream's receiver does
/// (`em_channel.cpp:2206-2209`).
#[derive(Debug, PartialEq, Eq)]
pub struct ChannelScanRequest<'a> {
    fresh: bool,
    radios: Vec<ScanRequestRadio<'a>>,
}

impl<'a> ChannelScanRequest<'a> {
    /// Creates a request, returning `None` if a stored-results radio has classes.
    #[must_use]
    pub fn new(fresh: bool, radios: Vec<ScanRequestRadio<'a>>) -> Option<Self> {
        if !fresh
            && radios
                .iter()
                .any(|radio| !radio.operating_classes.is_empty())
        {
            None
        } else {
            Some(Self { fresh, radios })
        }
    }

    /// Whether a fresh scan is requested.
    #[must_use]
    pub const fn fresh(&self) -> bool {
        self.fresh
    }

    /// The requested radios, in wire order.
    #[must_use]
    pub fn radios(&self) -> &[ScanRequestRadio<'a>] {
        &self.radios
    }
}

impl<'a> TlvBody<'a> for ChannelScanRequest<'a> {
    const TLV_TYPE: TlvType = TlvType::CHANNEL_SCAN_REQUEST;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let fresh = reader.u8()? & 0x80 != 0;
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 7);
        for _ in 0..count {
            let ruid = reader.mac()?;
            let class_count = reader.u8()?;
            if !fresh && class_count != 0 {
                return Err(DecodeError::ValueOutOfRange {
                    value: u32::from(class_count),
                });
            }
            let operating_classes = read_scan_classes(&mut reader, usize::from(class_count))?;
            radios.push(ScanRequestRadio {
                ruid,
                operating_classes,
            });
        }
        reader.finish()?;
        Ok(Self { fresh, radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(u8::from(self.fresh) << 7);
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            write_scan_classes(&mut writer, &radio.operating_classes)?;
        }
        Ok(writer.into_bytes())
    }
}

/// BSS load data carried when the load-present bit is set, `EasyMesh` §17.2.40,
/// Table 62. Sender `em_channel.cpp:772-793`, receiver `:2370-2385`;
/// see [`ChannelScanResult`], audit rows 142–152.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanBssLoad {
    /// Neighbor's channel utilization.
    pub channel_utilization: u8,
    /// Neighbor's station count.
    pub station_count: u16,
}

/// One neighboring BSS, `EasyMesh` §17.2.40, Table 62.
///
/// Upstream stores neighbors in `em_neighbor_t` (`em_base.h:1073-1082`);
/// sender `em_channel.cpp:687-793`, receiver `:2330-2387`. The bandwidth
/// length is always emitted, including for an empty string (row 143); upstream
/// omits it for unknown bandwidths at `:767`. Load data is present only when
/// bit 7 is set, as the spec says (`:772-793`), and reserved bit 6 is ignored.
/// Strings and reserved values, including SSID lengths above 32, remain raw
/// (0009 D2; row 150 decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanNeighbor<'a> {
    /// Neighbor BSSID.
    pub bssid: MacAddr,
    /// SSID bytes, including reserved lengths.
    pub ssid: &'a [u8],
    /// Encoded signal-strength value, including reserved values.
    pub signal_strength: u8,
    /// Raw channel bandwidth string.
    pub bandwidth: &'a [u8],
    /// Six-bit BSS color, including reserved zero.
    pub bss_color: u8,
    /// BSS load values, present exactly when the load-present bit is set.
    pub bss_load: Option<ScanBssLoad>,
}

/// Successful scan fields, `EasyMesh` §17.2.40, Table 62.
///
/// Upstream stores results in `em_scan_result_t` (`em_base.h:1084-1094`);
/// sender `em_channel.cpp:662-809`, receiver `:2312-2400`. The neighbor count
/// is not limited to upstream's 16 (audit rows 148–149). Timestamps are raw
/// bytes (0009 D2). The scan type is bit 7 alone, as the spec says
/// (`em_base.h:223`; sender `em_channel.cpp:803-807`, receiver `:2400`).
#[derive(Debug, PartialEq, Eq)]
pub struct ScanResultDetails<'a> {
    /// Scan start timestamp as raw bytes.
    pub timestamp: &'a [u8],
    /// Channel utilization.
    pub utilization: u8,
    /// Encoded noise, including reserved values.
    pub noise: u8,
    /// Discovered neighboring BSS records.
    pub neighbors: Vec<ScanNeighbor<'a>>,
    /// Aggregate scan duration in milliseconds.
    pub aggregate_scan_duration: u32,
    /// Whether the scan used active probing.
    pub active: bool,
}

impl<'a> ScanResultDetails<'a> {
    fn decode(reader: &mut Reader<'a>) -> Result<Self, DecodeError> {
        let timestamp = read_octets(reader)?;
        let utilization = reader.u8()?;
        let noise = reader.u8()?;
        let count = usize::from(reader.be_u16()?);
        let mut neighbors = bounded_vec(count, reader.remaining(), 10);
        for _ in 0..count {
            let bssid = reader.mac()?;
            let ssid = read_octets(reader)?;
            let signal_strength = reader.u8()?;
            let bandwidth = read_octets(reader)?;
            let flags = reader.u8()?;
            let bss_load = if flags & 0x80 != 0 {
                Some(ScanBssLoad {
                    channel_utilization: reader.u8()?,
                    station_count: reader.be_u16()?,
                })
            } else {
                None
            };
            neighbors.push(ScanNeighbor {
                bssid,
                ssid,
                signal_strength,
                bandwidth,
                bss_color: flags & 0x3f,
                bss_load,
            });
        }
        Ok(Self {
            timestamp,
            utilization,
            noise,
            neighbors,
            aggregate_scan_duration: reader.be_u32()?,
            active: reader.u8()? & 0x80 != 0,
        })
    }

    fn encode(&self, writer: &mut Writer) -> Result<(), EncodeError> {
        write_octets(writer, self.timestamp)?;
        writer.u8(self.utilization);
        writer.u8(self.noise);
        writer.count_u16(self.neighbors.len())?;
        for neighbor in &self.neighbors {
            writer.mac(neighbor.bssid);
            write_octets(writer, neighbor.ssid)?;
            writer.u8(neighbor.signal_strength);
            write_octets(writer, neighbor.bandwidth)?;
            writer.u8((u8::from(neighbor.bss_load.is_some()) << 7)
                | checked_bits(neighbor.bss_color, 63)?);
            if let Some(load) = neighbor.bss_load {
                writer.u8(load.channel_utilization);
                writer.be_u16(load.station_count);
            }
        }
        writer.be_u32(self.aggregate_scan_duration);
        writer.u8(u8::from(self.active) << 7);
        Ok(())
    }
}

/// Channel Scan Result, `EasyMesh` §17.2.40, Table 62; audit rows 142–152.
///
/// Struct `em_channel_scan_result_t` is at `em_base.h:1096-1103`; sender
/// `em_channel.cpp:645-809`, receiver `:2291-2403`. A nonzero status has no
/// details (row 144). Unlike the sender, which writes them after any status
/// (`em_channel.cpp:662-681`, `797-809`), the codec omits them. Lenient decoding
/// accepts the sender's complete extra field set after a nonzero status and
/// drops it, so the value re-encodes as the nine-octet spec form (0009 A6). The
/// codec bounds-checks all fields, keeps every neighbor, and accepts a
/// nine-octet failure (rows 146–149, 152), unlike upstream's unchecked parser,
/// 16-neighbor cap and message minimum. The bandwidth length is always present
/// (row 143; upstream omits it for an unknown bandwidth, `em_channel.cpp:767`).
/// Utilization and station count depend on load bit 7, and the scan type uses
/// only bit 7, as the spec says. Unlike the receiver's bandwidth matching, the
/// codec retains raw bandwidth bytes (row 147). Raw timestamps are not validated
/// or generated; upstream writes message-build time rather than scan start time
/// (row 151). Private status and details enforce their presence invariant.
#[derive(Debug, PartialEq, Eq)]
pub struct ChannelScanResult<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Global operating class.
    pub operating_class: u8,
    /// Scanned channel.
    pub channel: u8,
    status: u8,
    details: Option<ScanResultDetails<'a>>,
}

impl<'a> ChannelScanResult<'a> {
    /// Creates a result if status 0 has details and every other status has none.
    #[must_use]
    pub fn new(
        ruid: MacAddr,
        operating_class: u8,
        channel: u8,
        status: u8,
        details: Option<ScanResultDetails<'a>>,
    ) -> Option<Self> {
        if (status == 0) == details.is_some() {
            Some(Self {
                ruid,
                operating_class,
                channel,
                status,
                details,
            })
        } else {
            None
        }
    }

    /// Scan status, including reserved values.
    #[must_use]
    pub const fn status(&self) -> u8 {
        self.status
    }

    /// Successful scan details, absent for every nonzero status.
    #[must_use]
    pub const fn details(&self) -> Option<&ScanResultDetails<'a>> {
        self.details.as_ref()
    }

    /// Mutable successful scan details, without changing their presence.
    pub fn details_mut(&mut self) -> Option<&mut ScanResultDetails<'a>> {
        self.details.as_mut()
    }
}

impl<'a> TlvBody<'a> for ChannelScanResult<'a> {
    const TLV_TYPE: TlvType = TlvType::CHANNEL_SCAN_RESULT;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let operating_class = reader.u8()?;
        let channel = reader.u8()?;
        let status = reader.u8()?;
        let details = if status == 0 {
            Some(ScanResultDetails::decode(&mut reader)?)
        } else {
            None
        };
        reader.finish()?;
        Ok(Self {
            ruid,
            operating_class,
            channel,
            status,
            details,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(self.operating_class);
        writer.u8(self.channel);
        writer.u8(self.status);
        if let Some(details) = &self.details {
            details.encode(&mut writer)?;
        }
        Ok(writer.into_bytes())
    }
}

impl<'a> TlvBodyLenient<'a> for ChannelScanResult<'a> {
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        match Self::decode_value(value) {
            Ok(body) => Ok(Some(body)),
            Err(error @ DecodeError::TrailingBytes { .. }) if value.len() > 9 && value[8] != 0 => {
                let mut reader = Reader::new(value);
                let ruid = reader.mac()?;
                let operating_class = reader.u8()?;
                let channel = reader.u8()?;
                let status = reader.u8()?;
                if ScanResultDetails::decode(&mut reader).is_err() || reader.finish().is_err() {
                    return Err(error);
                }
                Ok(Some(Self {
                    ruid,
                    operating_class,
                    channel,
                    status,
                    details: None,
                }))
            }
            Err(error) => Err(error),
        }
    }
}

/// Timestamp, `EasyMesh` §17.2.41, Table 63; audit rows 139–141.
///
/// Struct `em_timestamp_t` is at `em_base.h:1053-1056`; sender
/// `em_channel.cpp:878-893`, receiver branch `:2442-2444` does nothing.
/// Unlike that receiver the codec checks the length (row 141). Timestamp bytes
/// are carried raw (0009 D2); upstream formats the current local time without
/// fractional seconds in `utils/util.cpp:68-79` (row 140).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp<'a> {
    /// Timestamp as raw octets.
    pub timestamp: &'a [u8],
}

impl<'a> TlvBody<'a> for Timestamp<'a> {
    const TLV_TYPE: TlvType = TlvType::TIMESTAMP;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let timestamp = read_octets(&mut reader)?;
        reader.finish()?;
        Ok(Self { timestamp })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        write_octets(&mut writer, self.timestamp)?;
        Ok(writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, sweep_row, sweep_row_lenient, truncated_in};
    use core::mem::size_of;

    const GROUP: &str = "channel";
    const RUID: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]);
    const RUID2: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf0]);
    const BSSID1: MacAddr = MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]);
    const BSSID2: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]);
    const TOO_MANY: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    const LENIENT_INPUTS: &[Key] = &[("0xa4", "empty"), ("0xa5", "empty")];
    const CANONICAL: &[(Key, Key)] = &[
        (("0xa4", "reserved-bits-set"), ("0xa4", "golden")),
        (
            ("0xa4", "reserved-bits-set-clear-flag"),
            ("0xa4", "do-not-report"),
        ),
        (
            ("0xa5", "reserved-bits-set"),
            ("0xa5", "reserved-bits-clear"),
        ),
        (
            ("0xa6", "reserved-bits-set"),
            ("0xa6", "reserved-bits-clear"),
        ),
        (
            ("0xa6", "reserved-bits-set-stored"),
            ("0xa6", "stored-results"),
        ),
        (
            ("0xa7", "nbr-reserved-bit6-set"),
            ("0xa7", "nbr-reserved-bit6-clear"),
        ),
        (
            ("0xa7", "nbr-reserved-bit6-set-no-load"),
            ("0xa7", "nbr-reserved-bit6-clear-no-load"),
        ),
        (
            ("0xa7", "scan-type-reserved-bits-set"),
            ("0xa7", "scan-type-reserved-bits-clear"),
        ),
        (
            ("0xa7", "scan-type-reserved-bits-set-passive"),
            ("0xa7", "scan-type-reserved-bits-clear-passive"),
        ),
    ];

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0x8b" => sweep_row::<ChannelPreference<'_>>(row, bytes, canonical),
            "0x8c" => sweep_row::<RadioOperationRestriction>(row, bytes, canonical),
            "0x8d" => sweep_row::<TransmitPowerLimit>(row, bytes, canonical),
            "0x8e" => sweep_row::<ChannelSelectionResponse>(row, bytes, canonical),
            "0x8f" => sweep_row::<OperatingChannelReport>(row, bytes, canonical),
            "0xa4" => sweep_row_lenient::<ChannelScanReportingPolicy>(
                row,
                bytes,
                canonical,
                LENIENT_INPUTS,
            ),
            "0xa5" => sweep_row_lenient::<ChannelScanCapabilities<'_>>(
                row,
                bytes,
                canonical,
                LENIENT_INPUTS,
            ),
            "0xa6" => sweep_row::<ChannelScanRequest<'_>>(row, bytes, canonical),
            "0xa7" => {
                sweep_row_lenient::<ChannelScanResult<'_>>(row, bytes, canonical, LENIENT_INPUTS)
            }
            "0xa8" => sweep_row::<Timestamp<'_>>(row, bytes, canonical),
            other => Err(format!("no body for TLV {other}")),
        }
    }

    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, CANONICAL, dispatch_row);
    }

    fn preference() -> ChannelPreference<'static> {
        ChannelPreference {
            ruid: RUID,
            operating_classes: vec![
                PreferenceOperatingClass {
                    operating_class: 115,
                    channels: &[36, 44],
                    preference: 12,
                    reason: 5,
                },
                PreferenceOperatingClass {
                    operating_class: 128,
                    channels: &[42],
                    preference: 0,
                    reason: 7,
                },
            ],
        }
    }
    fn restriction() -> RadioOperationRestriction {
        RadioOperationRestriction {
            ruid: RUID,
            operating_classes: vec![
                RestrictedOperatingClass {
                    operating_class: 115,
                    channels: vec![
                        RestrictedChannel {
                            channel: 36,
                            separation: 3,
                        },
                        RestrictedChannel {
                            channel: 48,
                            separation: 8,
                        },
                    ],
                },
                RestrictedOperatingClass {
                    operating_class: 125,
                    channels: vec![RestrictedChannel {
                        channel: 149,
                        separation: 16,
                    }],
                },
            ],
        }
    }
    fn power() -> TransmitPowerLimit {
        TransmitPowerLimit {
            ruid: RUID,
            tx_power_limit: 23,
        }
    }
    fn response() -> ChannelSelectionResponse {
        ChannelSelectionResponse {
            ruid: RUID,
            response_code: 0,
        }
    }
    fn operating() -> OperatingChannelReport {
        OperatingChannelReport {
            ruid: RUID,
            operating_classes: vec![
                OperatingChannel {
                    operating_class: 128,
                    channel: 42,
                },
                OperatingChannel {
                    operating_class: 115,
                    channel: 36,
                },
            ],
            tx_power: 23,
        }
    }
    fn policy() -> ChannelScanReportingPolicy {
        ChannelScanReportingPolicy { independent: true }
    }
    fn capabilities() -> ChannelScanCapabilities<'static> {
        ChannelScanCapabilities {
            radios: vec![
                ScanCapabilityRadio {
                    ruid: RUID,
                    on_boot_only: false,
                    scan_impact: 2,
                    minimum_scan_interval: 300,
                    operating_classes: vec![
                        ScanOperatingClass {
                            operating_class: 115,
                            channels: &[36, 40, 44, 48],
                        },
                        ScanOperatingClass {
                            operating_class: 118,
                            channels: &[],
                        },
                    ],
                },
                ScanCapabilityRadio {
                    ruid: RUID2,
                    on_boot_only: true,
                    scan_impact: 3,
                    minimum_scan_interval: 3600,
                    operating_classes: vec![ScanOperatingClass {
                        operating_class: 81,
                        channels: &[1, 6, 11],
                    }],
                },
            ],
        }
    }
    fn request() -> ChannelScanRequest<'static> {
        ChannelScanRequest::new(
            true,
            vec![
                ScanRequestRadio {
                    ruid: RUID,
                    operating_classes: vec![
                        ScanOperatingClass {
                            operating_class: 115,
                            channels: &[36, 44],
                        },
                        ScanOperatingClass {
                            operating_class: 121,
                            channels: &[],
                        },
                    ],
                },
                ScanRequestRadio {
                    ruid: RUID2,
                    operating_classes: vec![ScanOperatingClass {
                        operating_class: 81,
                        channels: &[1, 6, 11],
                    }],
                },
            ],
        )
        .unwrap()
    }
    fn result() -> ChannelScanResult<'static> {
        ChannelScanResult::new(
            RUID,
            115,
            36,
            0,
            Some(ScanResultDetails {
                timestamp: b"2026-11-20T10:15:30.25+01:00",
                utilization: 90,
                noise: 60,
                neighbors: vec![
                    ScanNeighbor {
                        bssid: BSSID1,
                        ssid: b"HomeNet",
                        signal_strength: 155,
                        bandwidth: b"80",
                        bss_color: 45,
                        bss_load: Some(ScanBssLoad {
                            channel_utilization: 127,
                            station_count: 258,
                        }),
                    },
                    ScanNeighbor {
                        bssid: BSSID2,
                        ssid: b"Cafe-5G",
                        signal_strength: 100,
                        bandwidth: b"160",
                        bss_color: 7,
                        bss_load: None,
                    },
                ],
                aggregate_scan_duration: 1200,
                active: true,
            }),
        )
        .unwrap()
    }
    fn failed(
        ruid: MacAddr,
        operating_class: u8,
        channel: u8,
        status: u8,
    ) -> ChannelScanResult<'static> {
        ChannelScanResult::new(ruid, operating_class, channel, status, None).unwrap()
    }
    fn timestamp() -> Timestamp<'static> {
        Timestamp {
            timestamp: b"2026-11-20T09:15:30.125Z",
        }
    }

    fn many_preference() -> ChannelPreference<'static> {
        ChannelPreference {
            ruid: RUID,
            operating_classes: vec![
                PreferenceOperatingClass {
                    operating_class: 1,
                    channels: &[],
                    preference: 1,
                    reason: 0
                };
                256
            ],
        }
    }
    fn many_restrictions() -> RadioOperationRestriction {
        RadioOperationRestriction {
            ruid: RUID,
            operating_classes: (0..256)
                .map(|_| RestrictedOperatingClass {
                    operating_class: 1,
                    channels: vec![],
                })
                .collect(),
        }
    }
    fn many_operating() -> OperatingChannelReport {
        OperatingChannelReport {
            ruid: RUID,
            operating_classes: vec![
                OperatingChannel {
                    operating_class: 1,
                    channel: 1
                };
                256
            ],
            tx_power: 0,
        }
    }
    fn many_capabilities() -> ChannelScanCapabilities<'static> {
        ChannelScanCapabilities {
            radios: (0..256)
                .map(|_| ScanCapabilityRadio {
                    ruid: RUID,
                    on_boot_only: false,
                    scan_impact: 0,
                    minimum_scan_interval: 0,
                    operating_classes: vec![],
                })
                .collect(),
        }
    }
    fn many_requests() -> ChannelScanRequest<'static> {
        ChannelScanRequest::new(
            false,
            (0..256)
                .map(|_| ScanRequestRadio {
                    ruid: RUID,
                    operating_classes: vec![],
                })
                .collect(),
        )
        .unwrap()
    }
    fn many_neighbors() -> ChannelScanResult<'static> {
        let mut body = result();
        let neighbor = ScanNeighbor {
            bssid: BSSID1,
            ssid: &[],
            signal_strength: 0,
            bandwidth: &[],
            bss_color: 0,
            bss_load: None,
        };
        body.details_mut().unwrap().neighbors = vec![neighbor; 65_536];
        body
    }
    fn long_timestamp() -> Timestamp<'static> {
        Timestamp {
            timestamp: &[0; 256],
        }
    }

    const fn field(offset: usize, width: usize) -> Field {
        Field { offset, width }
    }

    body_checks! {
        mod preference_checks;
        body: ChannelPreference<'a>;
        golden: &fixture(("0x8b", "golden")), preference();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1), field(7, 1), field(8, 1), field(9, 2), field(11, 1), field(12, 1), field(13, 1), field(14, 1), field(15, 1)]);
        trailing_byte: &fixture(("0x8b", "golden"));
        count_too_large: many_preference(), TOO_MANY;
    }
    body_checks! {
        mod restriction_checks;
        body: RadioOperationRestriction;
        golden: &fixture(("0x8c", "golden")), restriction();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1), field(7, 1), field(8, 1), field(9, 1), field(10, 1), field(11, 1), field(12, 1), field(13, 1), field(14, 1), field(15, 1), field(16, 1)]);
        trailing_byte: &fixture(("0x8c", "golden"));
        count_too_large: many_restrictions(), TOO_MANY;
    }
    body_checks! {
        mod power_checks;
        body: TransmitPowerLimit;
        golden: &fixture(("0x8d", "golden")), power();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1)]);
        trailing_byte: &fixture(("0x8d", "golden"));
    }
    body_checks! {
        mod response_checks;
        body: ChannelSelectionResponse;
        golden: &fixture(("0x8e", "golden")), response();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1)]);
        trailing_byte: &fixture(("0x8e", "golden"));
    }
    body_checks! {
        mod operating_checks;
        body: OperatingChannelReport;
        golden: &fixture(("0x8f", "golden")), operating();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1), field(7, 1), field(8, 1), field(9, 1), field(10, 1), field(11, 1)]);
        trailing_byte: &fixture(("0x8f", "golden"));
        count_too_large: many_operating(), TOO_MANY;
    }
    body_checks! {
        mod policy_checks;
        body: ChannelScanReportingPolicy;
        golden: &fixture(("0xa4", "golden")), policy();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0xa4", "golden"));
        reserved_bits: [(&fixture(("0xa4", "reserved-bits-set")), policy(), &fixture(("0xa4", "golden")))];
        lenient_absent: [&fixture(("0xa4", "empty"))];
    }
    body_checks! {
        mod capabilities_checks;
        body: ChannelScanCapabilities<'a>;
        golden: &fixture(("0xa5", "golden")), capabilities();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1), field(8, 4), field(12, 1), field(13, 1), field(14, 1), field(15, 4), field(19, 1), field(20, 1), field(21, 6), field(27, 1), field(28, 4), field(32, 1), field(33, 1), field(34, 1), field(35, 3)]);
        trailing_byte: &fixture(("0xa5", "golden"));
        count_too_large: many_capabilities(), TOO_MANY;
        reserved_bits: [(&fixture(("0xa5", "reserved-bits-set")), ChannelScanCapabilities::decode_value(&fixture(("0xa5", "reserved-bits-clear"))).unwrap(), &fixture(("0xa5", "reserved-bits-clear")))];
        lenient_absent: [&fixture(("0xa5", "empty"))];
    }
    body_checks! {
        mod request_checks;
        body: ChannelScanRequest<'a>;
        golden: &fixture(("0xa6", "golden")), request();
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 1), field(2, 6), field(8, 1), field(9, 1), field(10, 1), field(11, 2), field(13, 1), field(14, 1), field(15, 6), field(21, 1), field(22, 1), field(23, 1), field(24, 3)]);
        trailing_byte: &fixture(("0xa6", "golden"));
        count_too_large: many_requests(), TOO_MANY;
        reserved_bits: [(&fixture(("0xa6", "reserved-bits-set")), ChannelScanRequest::decode_value(&fixture(("0xa6", "reserved-bits-clear"))).unwrap(), &fixture(("0xa6", "reserved-bits-clear")))];
    }
    body_checks! {
        mod result_checks;
        body: ChannelScanResult<'a>;
        golden: &fixture(("0xa7", "golden")), result();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1), field(7, 1), field(8, 1), field(9, 1), field(10, 28), field(38, 1), field(39, 1), field(40, 2), field(42, 6), field(48, 1), field(49, 7), field(56, 1), field(57, 1), field(58, 2), field(60, 1), field(61, 1), field(62, 2), field(64, 6), field(70, 1), field(71, 7), field(78, 1), field(79, 1), field(80, 3), field(83, 1), field(84, 4), field(88, 1)]);
        trailing_byte: &fixture(("0xa7", "golden"));
        count_too_large: many_neighbors(), EncodeError::CountTooLarge { len: 65_536, max: 65_535 };
        reserved_bits: [(&fixture(("0xa7", "nbr-reserved-bit6-set")), ChannelScanResult::decode_value(&fixture(("0xa7", "nbr-reserved-bit6-clear"))).unwrap(), &fixture(("0xa7", "nbr-reserved-bit6-clear"))), (&fixture(("0xa7", "scan-type-reserved-bits-set")), ChannelScanResult::decode_value(&fixture(("0xa7", "scan-type-reserved-bits-clear"))).unwrap(), &fixture(("0xa7", "scan-type-reserved-bits-clear")))];
        lenient: [(&fixture(("0xa7", "failed-full-fields-upstream")), failed(RUID, 115, 36, 4), &fixture(("0xa7", "status-04-not-completed"))), (&fixture(("0xa7", "failed-full-fields-upstream-neighbors")), failed(RUID2, 81, 6, 5), &fixture(("0xa7", "failed-full-fields-upstream-neighbors"))[..9])];
        lenient_rejects: [(&fixture(("0xa7", "failed-status-trailing-byte")), DecodeError::TrailingBytes { remaining: 1 })];
    }
    body_checks! {
        mod timestamp_checks;
        body: Timestamp<'a>;
        golden: &fixture(("0xa8", "golden")), timestamp();
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 24)]);
        trailing_byte: &fixture(("0xa8", "golden"));
        count_too_large: long_timestamp(), TOO_MANY;
    }

    #[test]
    fn request_and_result_constructors_enforce_presence_conditions() {
        let radio = || ScanRequestRadio {
            ruid: RUID,
            operating_classes: vec![ScanOperatingClass {
                operating_class: 81,
                channels: &[],
            }],
        };
        assert!(ChannelScanRequest::new(false, vec![radio()]).is_none());
        assert!(ChannelScanRequest::new(true, vec![radio()]).is_some());
        assert!(
            ChannelScanRequest::new(
                true,
                vec![ScanRequestRadio {
                    ruid: RUID,
                    operating_classes: vec![]
                }]
            )
            .is_some()
        );
        assert_eq!(
            ChannelScanRequest::decode_value(&fixture(("0xa6", "stored-with-opclasses"))),
            Err(DecodeError::ValueOutOfRange { value: 1 })
        );
        assert!(ChannelScanResult::new(RUID, 1, 1, 0, None).is_none());
        let details = result().details.take();
        assert!(ChannelScanResult::new(RUID, 1, 1, 1, details).is_none());
        assert!(ChannelScanResult::new(RUID, 1, 1, 1, None).is_some());
    }

    #[test]
    fn failed_scan_leniency_requires_one_complete_tail() {
        for case in [
            "failed-full-fields-upstream",
            "failed-full-fields-upstream-neighbors",
        ] {
            let bytes = fixture(("0xa7", case));
            for end in 10..bytes.len() {
                assert!(
                    matches!(
                        ChannelScanResult::decode_value_lenient(&bytes[..end]),
                        Err(DecodeError::TrailingBytes { .. })
                    ),
                    "{case} accepted partial tail ending at {end}"
                );
            }
            let mut extra = bytes;
            extra.push(0xa5);
            assert!(matches!(
                ChannelScanResult::decode_value_lenient(&extra),
                Err(DecodeError::TrailingBytes { .. })
            ));
        }
        assert_eq!(
            ChannelScanResult::decode_value_lenient(&fixture((
                "0xa7",
                "failed-status-trailing-byte"
            ))),
            Err(DecodeError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn nested_counts_and_bit_widths_are_checked_on_encode() {
        let mut pref = preference();
        pref.operating_classes[0].channels = &[0; 256];
        assert_eq!(pref.encode_value(), Err(TOO_MANY));
        pref.operating_classes[0].channels = &[];
        pref.operating_classes[0].preference = 16;
        assert_eq!(
            pref.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 16, max: 15 })
        );
        pref.operating_classes[0].preference = 0;
        pref.operating_classes[0].reason = 16;
        assert_eq!(
            pref.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 16, max: 15 })
        );
        let mut restrictions = restriction();
        restrictions.operating_classes[0].channels = vec![
            RestrictedChannel {
                channel: 1,
                separation: 0
            };
            256
        ];
        assert_eq!(restrictions.encode_value(), Err(TOO_MANY));
        let mut cap = capabilities();
        cap.radios[0].scan_impact = 4;
        assert_eq!(
            cap.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 4, max: 3 })
        );
        cap.radios[0].scan_impact = 0;
        cap.radios[0].operating_classes[0].channels = &[0; 256];
        assert_eq!(cap.encode_value(), Err(TOO_MANY));
        let mut request = request();
        request.radios[0].operating_classes = vec![
            ScanOperatingClass {
                operating_class: 1,
                channels: &[]
            };
            256
        ];
        assert_eq!(request.encode_value(), Err(TOO_MANY));
        let mut result = result();
        let details = result.details_mut().unwrap();
        details.timestamp = &[0; 256];
        assert_eq!(result.encode_value(), Err(TOO_MANY));
        let details = result.details_mut().unwrap();
        details.timestamp = &[];
        details.neighbors[0].ssid = &[0; 256];
        assert_eq!(result.encode_value(), Err(TOO_MANY));
        let details = result.details_mut().unwrap();
        details.neighbors[0].ssid = &[];
        details.neighbors[0].bandwidth = &[0; 256];
        assert_eq!(result.encode_value(), Err(TOO_MANY));
        let details = result.details_mut().unwrap();
        details.neighbors[0].bandwidth = &[];
        details.neighbors[0].bss_color = 64;
        assert_eq!(
            result.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 64, max: 63 })
        );
    }

    #[test]
    fn spec_edges_keep_values_upstream_discards_or_limits() {
        // Audit 61: the empty list still has a preference and a reason.
        let value = fixture(("0x8b", "zero-chans-all-channels"));
        let body = ChannelPreference::decode_value(&value).unwrap();
        assert_eq!(
            body.operating_classes[0],
            PreferenceOperatingClass {
                operating_class: 81,
                channels: &[],
                preference: 3,
                reason: 1,
            }
        );
        // Audit 68/71: power is signed and zero is not an absent sentinel.
        assert_eq!(
            TransmitPowerLimit::decode_value(&fixture(("0x8d", "negative")))
                .unwrap()
                .tx_power_limit,
            -10
        );
        assert_eq!(
            TransmitPowerLimit::decode_value(&fixture(("0x8d", "zero-dbm")))
                .unwrap()
                .tx_power_limit,
            0
        );
        // Audits 75/76, 130/132: retain all entries, including zero-count lists.
        assert_eq!(
            OperatingChannelReport::decode_value(&fixture(("0x8f", "zero-opclasses")))
                .unwrap()
                .operating_classes
                .len(),
            0
        );
        assert_eq!(
            ChannelScanCapabilities::decode_value(&fixture(("0xa5", "four-radios")))
                .unwrap()
                .radios
                .len(),
            4
        );
        assert_eq!(
            ChannelScanCapabilities::decode_value(&fixture(("0xa5", "zero-radios")))
                .unwrap()
                .radios,
            []
        );
        let value = fixture(("0xa6", "fresh-zero-opclasses"));
        let request = ChannelScanRequest::decode_value(&value).unwrap();
        assert!(request.fresh());
        assert_eq!(request.radios()[0].operating_classes, []);
        // Audit 149: retain every neighbor after the upstream limit of 16.
        let value = fixture(("0xa7", "seventeen-neighbors"));
        assert_eq!(
            ChannelScanResult::decode_value(&value)
                .unwrap()
                .details()
                .unwrap()
                .neighbors
                .len(),
            17
        );
    }

    #[test]
    fn scan_result_load_strings_and_scan_type_follow_spec_bits() {
        let value = fixture(("0xa7", "nbr-reserved-bit6-set"));
        let body = ChannelScanResult::decode_value(&value).unwrap();
        let details = body.details().unwrap();
        assert_eq!(details.neighbors[0].bss_color, 63);
        assert_eq!(
            details.neighbors[0].bss_load,
            Some(ScanBssLoad {
                channel_utilization: 200,
                station_count: 2571
            })
        );
        assert_eq!(details.neighbors[0].bandwidth, b"40");
        assert!(details.active);
        let value = fixture(("0xa7", "nbr-reserved-bit6-set-no-load"));
        let body = ChannelScanResult::decode_value(&value).unwrap();
        assert_eq!(body.details().unwrap().neighbors[0].bss_load, None);
        let value = fixture(("0xa7", "scan-type-reserved-bits-set-passive"));
        assert!(
            !ChannelScanResult::decode_value(&value)
                .unwrap()
                .details()
                .unwrap()
                .active
        );
        let value = fixture(("0xa7", "ssid-33-reserved-length"));
        let body = ChannelScanResult::decode_value(&value).unwrap();
        assert_eq!(body.details().unwrap().neighbors[0].ssid.len(), 33);
        let value = fixture(("0xa7", "bw-empty"));
        let body = ChannelScanResult::decode_value(&value).unwrap();
        assert_eq!(body.details().unwrap().neighbors[0].bandwidth, b"");
    }

    #[test]
    fn decoded_strings_borrow_the_value() {
        let value = fixture(("0xa7", "golden"));
        let body = ChannelScanResult::decode_value(&value).unwrap();
        let details = body.details().unwrap();
        assert!(core::ptr::eq(
            details.timestamp.as_ptr(),
            value[10..].as_ptr()
        ));
        assert!(core::ptr::eq(
            details.neighbors[0].ssid.as_ptr(),
            value[49..].as_ptr()
        ));
        assert!(core::ptr::eq(
            details.neighbors[0].bandwidth.as_ptr(),
            value[58..].as_ptr()
        ));
    }

    #[test]
    fn maximal_count_claim_in_short_scan_result_is_truncated() {
        let value = [0, 0, 0, 0, 0, 0, 81, 6, 0, 0, 0, 0, 0xff, 0xff];
        assert_eq!(
            ChannelScanResult::decode_value(&value),
            Err(DecodeError::Truncated {
                needed: 6,
                remaining: 0
            })
        );
    }

    #[test]
    fn maximum_tlv_value_round_trips_through_message() {
        let mut classes = vec![
            PreferenceOperatingClass {
                operating_class: 1,
                channels: &[0; 255],
                preference: 1,
                reason: 0
            };
            253
        ];
        classes.push(PreferenceOperatingClass {
            operating_class: 2,
            channels: &[0; 248],
            preference: 2,
            reason: 0,
        });
        classes.push(PreferenceOperatingClass {
            operating_class: 3,
            channels: &[],
            preference: 3,
            reason: 0,
        });
        let body = ChannelPreference {
            ruid: RUID,
            operating_classes: classes,
        };
        assert_eq!(body.encode_value().unwrap().len(), 65_535);
        let message = crate::body_testkit::build_message(&body).unwrap();
        let tlv = crate::body_testkit::single_tlv(&message).unwrap();
        assert_eq!(&tlv.decode::<ChannelPreference<'_>>().unwrap(), &body);
        assert!(
            crate::body_testkit::build_message_with_budget(&body, crate::builder::RAW_BUDGET)
                .is_err()
        );
        let mut body = body;
        body.operating_classes[253].channels = &[0; 249];
        assert_eq!(body.encode_value().unwrap().len(), 65_536);
        let error = crate::body_testkit::build_message(&body).unwrap_err();
        assert!(error.contains("TlvTooLong"), "{error}");
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<PreferenceOperatingClass<'_>>() <= 4 * word);
        assert!(size_of::<ChannelPreference<'_>>() <= 5 * word);
        assert!(size_of::<RestrictedChannel>() <= 2);
        assert!(size_of::<RestrictedOperatingClass>() <= 4 * word);
        assert!(size_of::<RadioOperationRestriction>() <= 5 * word);
        assert!(size_of::<TransmitPowerLimit>() <= 8);
        assert!(size_of::<ChannelSelectionResponse>() <= 8);
        assert!(size_of::<OperatingChannel>() <= 2);
        assert!(size_of::<OperatingChannelReport>() <= 5 * word);
        assert!(size_of::<ChannelScanReportingPolicy>() <= 1);
        assert!(size_of::<ScanOperatingClass<'_>>() <= 3 * word);
        assert!(size_of::<ScanCapabilityRadio<'_>>() <= 6 * word);
        assert!(size_of::<ChannelScanCapabilities<'_>>() <= 4 * word);
        assert!(size_of::<ScanRequestRadio<'_>>() <= 5 * word);
        assert!(size_of::<ChannelScanRequest<'_>>() <= 5 * word);
        assert!(size_of::<ScanBssLoad>() <= 4);
        assert!(size_of::<ScanNeighbor<'_>>() <= 6 * word);
        assert!(size_of::<ScanResultDetails<'_>>() <= 7 * word);
        assert!(size_of::<ChannelScanResult<'_>>() <= 10 * word);
        assert!(size_of::<Timestamp<'_>>() <= 2 * word);
    }

    #[test]
    fn preference_and_reason_each_use_their_own_bits() {
        let zero = MacAddr::new([0; 6]);
        for (mask, preference, reason) in [
            (0x10, 1, 0),
            (0x20, 2, 0),
            (0x40, 4, 0),
            (0x80, 8, 0),
            (0x01, 0, 1),
            (0x02, 0, 2),
            (0x04, 0, 4),
            (0x08, 0, 8),
        ] {
            let mut bytes = [0_u8; 10];
            bytes[6] = 1;
            bytes[9] = mask;
            let expected = ChannelPreference {
                ruid: zero,
                operating_classes: vec![PreferenceOperatingClass {
                    operating_class: 0,
                    channels: &[],
                    preference,
                    reason,
                }],
            };
            assert_eq!(ChannelPreference::decode_value(&bytes), Ok(expected));
            let decoded = ChannelPreference::decode_value(&bytes).unwrap();
            assert_eq!(decoded.encode_value(), Ok(bytes.to_vec()));
        }
    }

    #[test]
    fn reporting_policy_flag_uses_its_own_bit() {
        let bytes = [0x80];
        let expected = ChannelScanReportingPolicy { independent: true };
        assert_eq!(
            ChannelScanReportingPolicy::decode_value(&bytes),
            Ok(expected)
        );
        assert_eq!(expected.encode_value(), Ok(bytes.to_vec()));
        assert_eq!(
            ChannelScanReportingPolicy::decode_value(&[0]),
            Ok(ChannelScanReportingPolicy { independent: false })
        );
    }

    #[test]
    fn scan_capability_boot_and_impact_each_use_their_own_bits() {
        let zero = MacAddr::new([0; 6]);
        for (mask, on_boot_only, scan_impact) in
            [(0x80, true, 0), (0x20, false, 1), (0x40, false, 2)]
        {
            let mut bytes = [0_u8; 13];
            bytes[0] = 1;
            bytes[7] = mask;
            let expected = ChannelScanCapabilities {
                radios: vec![ScanCapabilityRadio {
                    ruid: zero,
                    on_boot_only,
                    scan_impact,
                    minimum_scan_interval: 0,
                    operating_classes: vec![],
                }],
            };
            assert_eq!(ChannelScanCapabilities::decode_value(&bytes), Ok(expected));
            let decoded = ChannelScanCapabilities::decode_value(&bytes).unwrap();
            assert_eq!(decoded.encode_value(), Ok(bytes.to_vec()));
        }
    }

    #[test]
    fn scan_request_fresh_flag_uses_its_own_bit() {
        let bytes = [0x80, 0];
        let expected = ChannelScanRequest::new(true, vec![]).unwrap();
        assert_eq!(ChannelScanRequest::decode_value(&bytes), Ok(expected));
        let decoded = ChannelScanRequest::decode_value(&bytes).unwrap();
        assert_eq!(decoded.encode_value(), Ok(bytes.to_vec()));
        assert_eq!(
            ChannelScanRequest::decode_value(&[0, 0]),
            Ok(ChannelScanRequest::new(false, vec![]).unwrap())
        );
    }

    #[test]
    fn scan_neighbor_load_and_color_each_use_their_own_bits() {
        let zero = MacAddr::new([0; 6]);
        for (mask, color, load) in [
            (0x01, 1, false),
            (0x02, 2, false),
            (0x04, 4, false),
            (0x08, 8, false),
            (0x10, 16, false),
            (0x20, 32, false),
            (0x80, 0, true),
        ] {
            let mut bytes = vec![0_u8; if load { 32 } else { 29 }];
            bytes[13] = 1;
            bytes[23] = mask;
            let expected = ChannelScanResult::new(
                zero,
                0,
                0,
                0,
                Some(ScanResultDetails {
                    timestamp: &[],
                    utilization: 0,
                    noise: 0,
                    neighbors: vec![ScanNeighbor {
                        bssid: zero,
                        ssid: &[],
                        signal_strength: 0,
                        bandwidth: &[],
                        bss_color: color,
                        bss_load: if load {
                            Some(ScanBssLoad {
                                channel_utilization: 0,
                                station_count: 0,
                            })
                        } else {
                            None
                        },
                    }],
                    aggregate_scan_duration: 0,
                    active: false,
                }),
            )
            .unwrap();
            assert_eq!(
                ChannelScanResult::decode_value(&bytes),
                Ok(expected),
                "mask {mask:#04x}"
            );
            let decoded = ChannelScanResult::decode_value(&bytes).unwrap();
            assert_eq!(decoded.encode_value(), Ok(bytes), "mask {mask:#04x}");
        }
    }

    #[test]
    fn scan_result_type_flag_uses_its_own_bit() {
        let zero = MacAddr::new([0; 6]);
        let mut bytes = [0_u8; 19];
        bytes[18] = 0x80;
        let expected = ChannelScanResult::new(
            zero,
            0,
            0,
            0,
            Some(ScanResultDetails {
                timestamp: &[],
                utilization: 0,
                noise: 0,
                neighbors: vec![],
                aggregate_scan_duration: 0,
                active: true,
            }),
        )
        .unwrap();
        assert_eq!(ChannelScanResult::decode_value(&bytes), Ok(expected));
        let decoded = ChannelScanResult::decode_value(&bytes).unwrap();
        assert_eq!(decoded.encode_value(), Ok(bytes.to_vec()));
    }
}
