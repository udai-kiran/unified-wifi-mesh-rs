// SPDX-License-Identifier: Apache-2.0

//! Policy bodies: steering, metric reporting, traffic separation, `QoS` and related policies
//! (note 0009, B09).
//!
//! All multi-octet integers are big-endian (§3.1.2; audit rows 172, 175 and 193 for the three
//! that upstream writes). Reserved values are kept raw (note 0009, D2); reserved bits are ignored
//! on decode and written as zero.

use em_types::{MacAddr, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// `STA_Traffic_Stats` bit of a Metric Reporting Policy radio flags octet.
const STA_TRAFFIC_STATS: u8 = 0x80;
/// `STA_Link_Metrics` bit of a Metric Reporting Policy radio flags octet.
const STA_LINK_METRICS: u8 = 0x40;
/// `STA_Status` bit of a Metric Reporting Policy radio flags octet.
const STA_STATUS: u8 = 0x20;

/// Bit position of the PCP field in the Default 802.1Q Settings octet (bits 7-5).
const PCP_SHIFT: u8 = 5;
/// Largest PCP value, which fills the three PCP bits.
const MAX_PCP: u8 = 7;

/// `Report Unsuccessful Associations` bit of the Unsuccessful Association Policy octet.
const REPORT_UNSUCCESSFUL: u8 = 0x80;

/// `Profile-1 bSTA Disallowed` bit of the Backhaul BSS Configuration octet.
const PROFILE_1_BSTA_DISALLOWED: u8 = 0x80;
/// `Profile-2 bSTA Disallowed` bit of the Backhaul BSS Configuration octet.
const PROFILE_2_BSTA_DISALLOWED: u8 = 0x40;

/// `Add-Remove` bit of the Service Prioritization Rule octet.
const RULE_ADD: u8 = 0x80;
/// `Always Match` bit of the last Service Prioritization Rule octet.
const RULE_ALWAYS_MATCH: u8 = 0x80;

/// Number of PCP values in a DSCP Mapping Table: one per DSCP value 0x00 to 0x3F.
const DSCP_VALUES: usize = 64;

/// Number of reserved octets at the end of a `QoS` Management Policy.
const QOS_POLICY_RESERVED_OCTETS: usize = 20;

/// Returns `mask` if `is_set`, else zero.
fn flag_bit(is_set: bool, mask: u8) -> u8 {
    if is_set { mask } else { 0 }
}

/// Reads a one-octet count and that many MAC addresses.
fn read_macs(reader: &mut Reader<'_>) -> Result<Vec<MacAddr>, DecodeError> {
    let count = usize::from(reader.u8()?);
    let mut macs = bounded_vec(count, reader.remaining(), 6);
    for _ in 0..count {
        macs.push(reader.mac()?);
    }
    Ok(macs)
}

/// Writes a one-octet count and the MAC addresses.
fn write_macs(writer: &mut Writer, macs: &[MacAddr]) -> Result<(), EncodeError> {
    writer.count_u8(macs.len())?;
    for mac in macs {
        writer.mac(*mac);
    }
    Ok(())
}

/// The steering policy of one radio in [`SteeringPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioSteeringPolicy {
    /// Radio unique identifier of the AP radio the policy is for.
    pub ruid: MacAddr,
    /// Steering policy, raw: 0 agent-initiated steering disallowed, 1 agent-initiated RCPI-based
    /// steering mandated, 2 agent-initiated RCPI-based steering allowed, 3-255 reserved.
    pub steering_policy: u8,
    /// Channel utilization threshold (BSS Load element, IEEE 802.11 §9.4.2.28).
    pub utilization_threshold: u8,
    /// RCPI steering threshold: 0-220 is an RCPI, 221-255 is reserved. Kept raw.
    pub rcpi_threshold: u8,
}

/// Steering Policy, `EasyMesh` §17.2.11 Table 33; audit rows 53–56.
///
/// Upstream `em_steering_policy_t` (`em_base.h:2109-2126`), sender `create_steering_policy_tlv`
/// (`em_policy_cfg.cpp:147-271`, header `:530-541`), receiver `handle_policy_cfg_req`
/// (`em_policy_cfg.cpp:745-768`). The value is two MAC lists (local steering disallowed, BTM
/// steering disallowed) and a list of per-radio policies; each list has a one-octet count that
/// may be zero.
///
/// Intentional difference: upstream stores at most 16 MACs per list (`EM_MAX_STA_PER_STEER_POLICY`,
/// `em_base.h:101`) and 4 radios (`EM_MAX_RADIO_PER_AGENT`, `em_base.h:90`). Its receiver copies
/// each wire count into those arrays (`em_policy_cfg.cpp:747-768`), so a larger count overflows
/// its stack object (audit 53, 54). This body accepts every count from 0 to 255.
#[derive(Debug, PartialEq, Eq)]
pub struct SteeringPolicy {
    /// STAs for which local steering is disallowed.
    pub local_steering_disallowed: Vec<MacAddr>,
    /// STAs for which BTM steering is disallowed.
    pub btm_steering_disallowed: Vec<MacAddr>,
    /// The per-radio policies, in wire order.
    pub radios: Vec<RadioSteeringPolicy>,
}

impl TlvBody<'_> for SteeringPolicy {
    const TLV_TYPE: TlvType = TlvType::STEERING_POLICY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let local_steering_disallowed = read_macs(&mut reader)?;
        let btm_steering_disallowed = read_macs(&mut reader)?;
        let radio_count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(radio_count, reader.remaining(), 9);
        for _ in 0..radio_count {
            radios.push(RadioSteeringPolicy {
                ruid: reader.mac()?,
                steering_policy: reader.u8()?,
                utilization_threshold: reader.u8()?,
                rcpi_threshold: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            local_steering_disallowed,
            btm_steering_disallowed,
            radios,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        write_macs(&mut writer, &self.local_steering_disallowed)?;
        write_macs(&mut writer, &self.btm_steering_disallowed)?;
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.u8(radio.steering_policy);
            writer.u8(radio.utilization_threshold);
            writer.u8(radio.rcpi_threshold);
        }
        Ok(writer.into_bytes())
    }
}

/// The metric reporting policy of one radio in [`MetricReportingPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioMetricReportingPolicy {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// STA Metrics Reporting RCPI threshold: 0 means do not report on RCPI, 1-220 is an RCPI,
    /// 221-255 is reserved. Kept raw.
    pub sta_rcpi_threshold: u8,
    /// STA Metrics Reporting RCPI hysteresis margin override in dB; 0 uses the agent's default.
    /// The spec table (§17.2.12) names this field `STA_RCPI_Threshold` a second time; audit row 57
    /// gives it as `STA_RCPI_Hysteresis`.
    pub sta_rcpi_hysteresis: u8,
    /// AP Metrics channel utilization reporting threshold; 0 means do not report on utilization.
    pub ap_utilization_threshold: u8,
    /// `STA_Traffic_Stats` (bit 7): include Associated STA Traffic Stats TLVs in the AP Metrics
    /// Response.
    pub include_sta_traffic_stats: bool,
    /// `STA_Link_Metrics` (bit 6): include Associated STA Link Metrics TLVs.
    pub include_sta_link_metrics: bool,
    /// `STA_Status` (bit 5): include Associated Wi-Fi 6 STA Status Report TLVs.
    pub include_sta_status: bool,
}

/// Metric Reporting Policy, `EasyMesh` §17.2.12 Table 34; audit rows 57–60.
///
/// Upstream `em_metric_rprt_policy_t` (`em_base.h:2128-2140`), sender
/// `create_metrics_rep_policy_tlv` (`em_policy_cfg.cpp:44-145`, added at `:546-556`), receiver
/// `handle_policy_cfg_req` (`em_policy_cfg.cpp:769-784`). The value is the interval, the radio
/// count and one 10-octet record per radio. Bits 4-0 of each flags octet are reserved: ignored
/// on decode and written as zero (audit 59).
///
/// Lenient form: when the sender has no policy it still emits the TLV header with length 0
/// (audit 57), so an empty value is upstream's "no policy" and decodes to `Ok(None)`. The
/// strict decoder rejects it with `Truncated`.
///
/// Intentional difference: upstream stores at most 4 radios (`EM_MAX_RADIO_PER_AGENT`,
/// `em_base.h:90`). Its receiver copies every wire record into that array
/// (`em_policy_cfg.cpp:770-784`), so more overflows its stack object (audit 58). This body accepts
/// every count from 0 to 255.
#[derive(Debug, PartialEq, Eq)]
pub struct MetricReportingPolicy {
    /// AP Metrics reporting interval in seconds; 0 means do not report periodically.
    pub interval: u8,
    /// The per-radio policies, in wire order.
    pub radios: Vec<RadioMetricReportingPolicy>,
}

impl TlvBody<'_> for MetricReportingPolicy {
    const TLV_TYPE: TlvType = TlvType::METRIC_REPORTING_POLICY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let interval = reader.u8()?;
        let radio_count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(radio_count, reader.remaining(), 10);
        for _ in 0..radio_count {
            let ruid = reader.mac()?;
            let sta_rcpi_threshold = reader.u8()?;
            let sta_rcpi_hysteresis = reader.u8()?;
            let ap_utilization_threshold = reader.u8()?;
            let flags = reader.u8()?;
            radios.push(RadioMetricReportingPolicy {
                ruid,
                sta_rcpi_threshold,
                sta_rcpi_hysteresis,
                ap_utilization_threshold,
                include_sta_traffic_stats: flags & STA_TRAFFIC_STATS != 0,
                include_sta_link_metrics: flags & STA_LINK_METRICS != 0,
                include_sta_status: flags & STA_STATUS != 0,
            });
        }
        reader.finish()?;
        Ok(Self { interval, radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.interval);
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.u8(radio.sta_rcpi_threshold);
            writer.u8(radio.sta_rcpi_hysteresis);
            writer.u8(radio.ap_utilization_threshold);
            writer.u8(flag_bit(radio.include_sta_traffic_stats, STA_TRAFFIC_STATS)
                | flag_bit(radio.include_sta_link_metrics, STA_LINK_METRICS)
                | flag_bit(radio.include_sta_status, STA_STATUS));
        }
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for MetricReportingPolicy {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// Default 802.1Q Settings, `EasyMesh` §17.2.49 Table 71; audit rows 172–174.
///
/// Upstream `em_8021q_settings_t` (`em_base.h:1697-1701`), sender
/// `create_def_8021q_settings_policy_tlv` (`em.cpp:2109-2138`, added at
/// `em_policy_cfg.cpp:561-570`), receiver `em_policy_cfg.cpp:785-792`. The value is exactly
/// 3 octets: the VLAN ID, then the PCP in bits 7-5 with bits 4-0 reserved (ignored on decode,
/// written as zero).
///
/// Any VLAN ID is kept, including 0, which autoconfiguration sends as a placeholder
/// (`em_configuration.cpp:3758-3761`, audit 174, note 0009 D3). A PCP above 7 does not encode:
/// [`EncodeError::ValueTooLarge`].
///
/// Lenient form: when the data model has no policy the sender emits the TLV header with length 0
/// (audit 173), so an empty value decodes to `Ok(None)`. The strict decoder rejects it with
/// `Truncated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Default8021QSettings {
    /// Primary VLAN ID.
    pub vlan: u16,
    /// Default PCP, 0 to 7.
    pub pcp: u8,
}

impl TlvBody<'_> for Default8021QSettings {
    const TLV_TYPE: TlvType = TlvType::DEFAULT_8021Q_SETTINGS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let vlan = reader.be_u16()?;
        let pcp_octet = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            vlan,
            pcp: pcp_octet >> PCP_SHIFT,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        if self.pcp > MAX_PCP {
            return Err(EncodeError::ValueTooLarge {
                value: u32::from(self.pcp),
                max: u32::from(MAX_PCP),
            });
        }
        let mut writer = Writer::new();
        writer.be_u16(self.vlan);
        writer.u8(self.pcp << PCP_SHIFT);
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for Default8021QSettings {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// One SSID and its VLAN in [`TrafficSeparationPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficSeparationSsid<'a> {
    /// SSID bytes, raw: any length from 0 to 255 octets is kept, including lengths above 32
    /// (note 0009, D2).
    pub ssid: &'a [u8],
    /// VLAN ID. Values 0x0000-0x0002 and 0x0FFF-0xFFFF are reserved; kept raw.
    pub vlan: u16,
}

/// Traffic Separation Policy, `EasyMesh` §17.2.50 Table 72; audit rows 175–180.
///
/// Upstream sender `create_traffic_separation_policy_tlv` (`em.cpp:2070-2107`, added at
/// `em_policy_cfg.cpp:575-585`), receiver `handle_policy_cfg_req` (`em_policy_cfg.cpp:793-820`).
/// The value is the SSID count and, per SSID, a length octet, the SSID and a VLAN. The count is
/// the list length (audit 180, note 0009 D3). An SSID longer than 255 octets does not encode:
/// [`EncodeError::CountTooLarge`], as for the SSID count.
///
/// Intentional difference: upstream stores `Num_SSID` but parses the entries only when it is at
/// most 5 (`em_haul_type_max`, `em_base.h:2494`; `em_policy_cfg.cpp:795-796`), so a spec-valid TLV
/// with more SSIDs is silently dropped (audit 177). It also skips an SSID above
/// `MAX_SSID_NAME_LEN` without advancing (`em_policy_cfg.cpp:804`, audit 178). This body parses
/// every entry.
#[derive(Debug, PartialEq, Eq)]
pub struct TrafficSeparationPolicy<'a> {
    /// The SSIDs and their VLANs, in wire order.
    pub ssids: Vec<TrafficSeparationSsid<'a>>,
}

impl<'a> TlvBody<'a> for TrafficSeparationPolicy<'a> {
    const TLV_TYPE: TlvType = TlvType::TRAFFIC_SEPARATION_POLICY;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut ssids = bounded_vec(count, reader.remaining(), 3);
        for _ in 0..count {
            let ssid_len = usize::from(reader.u8()?);
            ssids.push(TrafficSeparationSsid {
                ssid: reader.bytes(ssid_len)?,
                vlan: reader.be_u16()?,
            });
        }
        reader.finish()?;
        Ok(Self { ssids })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.ssids.len())?;
        for entry in &self.ssids {
            writer.count_u8(entry.ssid.len())?;
            writer.bytes(entry.ssid);
            writer.be_u16(entry.vlan);
        }
        Ok(writer.into_bytes())
    }
}

/// Unsuccessful Association Policy, `EasyMesh` §17.2.58 Table 80; audit rows 193–194.
///
/// Upstream `em_unsuccessful_assoc_policy_t` (`em_base.h:2147-2151`), sender
/// `create_unsucc_assoc_policy_tlv` (`em_policy_cfg.cpp:304-332`, added at `:604-616`), receiver
/// `em_policy_cfg.cpp:829-849`. The value is exactly 5 octets: the report flag in bit 7 (bits
/// 6-0 reserved: ignored on decode, written as zero) and the 4-octet maximum rate.
///
/// Lenient form: when the data model has no policy the sender emits the TLV header with length 0,
/// so an empty value decodes to `Ok(None)`. The strict decoder rejects it with `Truncated`.
///
/// Intentional difference: the receiver stores the rate in a 16-bit field through
/// `static_cast<unsigned short>` (`em_policy_cfg.cpp:840-841`, audit 194). This body keeps all 32
/// bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsuccessfulAssociationPolicy {
    /// `true` if the agent reports unsuccessful association attempts to the controller.
    pub report: bool,
    /// Maximum rate for reporting unsuccessful association attempts, in attempts per minute.
    pub max_reporting_rate: u32,
}

impl TlvBody<'_> for UnsuccessfulAssociationPolicy {
    const TLV_TYPE: TlvType = TlvType::UNSUCCESSFUL_ASSOCIATION_POLICY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let flags = reader.u8()?;
        let max_reporting_rate = reader.be_u32()?;
        reader.finish()?;
        Ok(Self {
            report: flags & REPORT_UNSUCCESSFUL != 0,
            max_reporting_rate,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(flag_bit(self.report, REPORT_UNSUCCESSFUL));
        writer.be_u32(self.max_reporting_rate);
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for UnsuccessfulAssociationPolicy {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// Backhaul BSS Configuration, `EasyMesh` §17.2.66 Table 88; audit rows 211–212.
///
/// Upstream `em_bh_bss_config_t` (`em_base.h:2153-2158`), sender
/// `create_backhaul_bss_conf_policy_tlv` (`em_policy_cfg.cpp:336-349`, loop `:618-646`), receiver
/// `em_policy_cfg.cpp:850-862`. One TLV describes one BSS: the value is exactly 7 octets, so a
/// second entry in one TLV is `TrailingBytes`. Bits 5-0 of the flags octet are reserved: ignored
/// on decode and written as zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackhaulBssConfiguration {
    /// BSSID of the BSS the configuration applies to.
    pub bssid: MacAddr,
    /// Profile-1 backhaul STA association is disallowed (bit 7).
    pub profile_1_bsta_disallowed: bool,
    /// Profile-2 backhaul STA association is disallowed (bit 6).
    pub profile_2_bsta_disallowed: bool,
}

impl TlvBody<'_> for BackhaulBssConfiguration {
    const TLV_TYPE: TlvType = TlvType::BACKHAUL_BSS_CONFIGURATION;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let bssid = reader.mac()?;
        let flags = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            bssid,
            profile_1_bsta_disallowed: flags & PROFILE_1_BSTA_DISALLOWED != 0,
            profile_2_bsta_disallowed: flags & PROFILE_2_BSTA_DISALLOWED != 0,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.u8(
            flag_bit(self.profile_1_bsta_disallowed, PROFILE_1_BSTA_DISALLOWED)
                | flag_bit(self.profile_2_bsta_disallowed, PROFILE_2_BSTA_DISALLOWED),
        );
        Ok(writer.into_bytes())
    }
}

/// Service Prioritization Rule, `EasyMesh` §17.2.70 Table 92; audit rows 216–217.
///
/// Upstream `em_service_prio_rule_t` (`em_base.h:1761-1769`) has no sender or receiver; only the
/// validator table (`em_msg.cpp:1117`) and a test generator (`validation_test.cpp:2544-2548`)
/// mention it. The value is exactly 8 octets. The rule ID is big-endian (§3.1.2; the upstream
/// struct is host order but nothing sends it, audit 216). Bits 6-0 of the add octet and of the
/// last octet are reserved: ignored on decode and written as zero. Precedence 0xFF and output
/// 0x0A-0xFF are reserved; kept raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServicePrioritizationRule {
    /// Service Prioritization Rule identifier.
    pub rule_id: u32,
    /// `true` to add the rule, `false` to remove it (bit 7).
    pub add: bool,
    /// Rule precedence; a higher number is a higher priority.
    pub precedence: u8,
    /// Rule output: how the 802.1Q C-TAG PCP value is selected, raw.
    pub output: u8,
    /// `true` if the rule always matches (bit 7 of the last octet).
    pub always_match: bool,
}

impl TlvBody<'_> for ServicePrioritizationRule {
    const TLV_TYPE: TlvType = TlvType::SERVICE_PRIORITIZATION_RULE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let rule_id = reader.be_u32()?;
        let add_octet = reader.u8()?;
        let precedence = reader.u8()?;
        let output = reader.u8()?;
        let match_octet = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            rule_id,
            add: add_octet & RULE_ADD != 0,
            precedence,
            output,
            always_match: match_octet & RULE_ALWAYS_MATCH != 0,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.be_u32(self.rule_id);
        writer.u8(flag_bit(self.add, RULE_ADD));
        writer.u8(self.precedence);
        writer.u8(self.output);
        writer.u8(flag_bit(self.always_match, RULE_ALWAYS_MATCH));
        Ok(writer.into_bytes())
    }
}

/// DSCP Mapping Table, `EasyMesh` §17.2.71 Table 93; audit row 218.
///
/// Upstream `em_dscp_map_table_t` (`em_base.h:1771-1773`) has no sender or receiver; the
/// validator table (`em_msg.cpp:1118`) and a test generator (`validation_test.cpp:2552-2556`)
/// only. The value is exactly 64 octets, one PCP per DSCP value 0x00 to 0x3F in increasing
/// order. PCP values 0x08-0xFF are reserved; kept raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DscpMappingTable {
    /// The PCP value for DSCP `n`, at index `n`.
    pub pcp: [u8; DSCP_VALUES],
}

impl TlvBody<'_> for DscpMappingTable {
    const TLV_TYPE: TlvType = TlvType::DSCP_MAPPING_TABLE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let pcp = reader.array()?;
        reader.finish()?;
        Ok(Self { pcp })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.bytes(&self.pcp);
        Ok(writer.into_bytes())
    }
}

/// `QoS` Management Policy, `EasyMesh` §17.2.92 Table 114; audit rows 270–271.
///
/// Upstream sender `create_qos_mgt_policy_tlv` (`em_policy_cfg.cpp:351-392`, caller `:648-669`),
/// receiver `em_policy_cfg.cpp:863-892`. The value is two MAC lists (STAs for which MSCS, then
/// SCS, is disallowed), each with a one-octet count that may be zero, then 20 reserved octets.
/// Decoding ignores the reserved octets and encoding writes zeros.
///
/// Intentional difference: upstream sends at most 16 MACs per list (`em_policy_cfg.cpp:357-362`,
/// `em_base.h:102-103`; audit 271). This body keeps every count from 0 to 255. Upstream's receiver
/// also keeps at most 4 of these TLVs per message (`em_policy_cfg.cpp:864`, `EM_MAX_QOS_MGMT_POLICY`,
/// `em_base.h:89`); that is a message-level limit for the consumer, like the 16-TLV limit on Backhaul
/// BSS Configuration (`em_policy_cfg.cpp:851`), not a body difference.
#[derive(Debug, PartialEq, Eq)]
pub struct QosManagementPolicy {
    /// STAs for which MSCS operation is disallowed.
    pub mscs_disallowed: Vec<MacAddr>,
    /// STAs for which SCS operation is disallowed.
    pub scs_disallowed: Vec<MacAddr>,
}

impl TlvBody<'_> for QosManagementPolicy {
    const TLV_TYPE: TlvType = TlvType::QOS_MANAGEMENT_POLICY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let mscs_disallowed = read_macs(&mut reader)?;
        let scs_disallowed = read_macs(&mut reader)?;
        reader.skip(QOS_POLICY_RESERVED_OCTETS)?;
        reader.finish()?;
        Ok(Self {
            mscs_disallowed,
            scs_disallowed,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        write_macs(&mut writer, &self.mscs_disallowed)?;
        write_macs(&mut writer, &self.scs_disallowed)?;
        writer.zeros(QOS_POLICY_RESERVED_OCTETS);
        Ok(writer.into_bytes())
    }
}

/// `QoS` Management Descriptor, `EasyMesh` §17.2.93 Table 115; audit row 272.
///
/// Upstream `em_qos_mgmt_des_t` (`em_base.h:960-965`) has no sender or receiver; only validator
/// minimum lengths (`em_msg.cpp:1015-1017,1119`) and test generators. The QMID is big-endian
/// (§3.1.2; nothing upstream writes it). The descriptor element (an MSCS, SCS or `QoS` Management
/// element) is the rest of the value as raw octets, and may be empty (note 0009, D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QosManagementDescriptor<'a> {
    /// Identifier of the `QoS` Management rule.
    pub qmid: u16,
    /// BSSID of the BSS the descriptor applies to.
    pub bssid: MacAddr,
    /// The client MAC (the affiliated STA MAC address of a client MLD) the descriptor applies to.
    pub client_mac: MacAddr,
    /// The descriptor element, raw.
    pub descriptor: &'a [u8],
}

impl<'a> TlvBody<'a> for QosManagementDescriptor<'a> {
    const TLV_TYPE: TlvType = TlvType::QOS_MANAGEMENT_DESCRIPTOR;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let qmid = reader.be_u16()?;
        let bssid = reader.mac()?;
        let client_mac = reader.mac()?;
        let descriptor = reader.rest();
        reader.finish()?;
        Ok(Self {
            qmid,
            bssid,
            client_mac,
            descriptor,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.be_u16(self.qmid);
        writer.mac(self.bssid);
        writer.mac(self.client_mac);
        writer.bytes(self.descriptor);
        Ok(writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, sweep_row, sweep_row_lenient, truncated_in};
    use core::mem::size_of;

    const GROUP: &str = "policy";
    const BSSID: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4]);
    const RUID0: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]);
    const RUID1: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]);
    const STA1: MacAddr = MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]);
    const STA2: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]);
    const OTHER: MacAddr = MacAddr::new([0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f]);
    const TOO_MANY: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    /// Invalid rows that a lenient decoder accepts; the only lenient form is the empty value,
    /// which is a kind=lenient row.
    const LENIENT_INPUTS: &[Key] = &[];
    const CANONICAL: &[(Key, Key)] = &[
        (
            ("0x8a", "reserved-bits-set"),
            ("0x8a", "reserved-bits-clear"),
        ),
        (
            ("0x8a", "reserved-bits-set-zero-flags"),
            ("0x8a", "no-flags"),
        ),
        (
            ("0xb5", "reserved-bits-set"),
            ("0xb5", "reserved-bits-clear"),
        ),
        (
            ("0xc4", "reserved-bits-set"),
            ("0xc4", "reserved-bits-clear"),
        ),
        (
            ("0xc4", "reserved-bits-set-report-off"),
            ("0xc4", "report-off"),
        ),
        (
            ("0xd0", "reserved-bits-set"),
            ("0xd0", "reserved-bits-clear"),
        ),
        (("0xb9", "add-reserved-set"), ("0xb9", "add-reserved-clear")),
        (
            ("0xb9", "always-reserved-set"),
            ("0xb9", "always-reserved-clear"),
        ),
        (
            ("0xdb", "reserved-octets-set"),
            ("0xdb", "reserved-octets-clear"),
        ),
    ];

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0x89" => sweep_row::<SteeringPolicy>(row, bytes, canonical),
            "0x8a" => {
                sweep_row_lenient::<MetricReportingPolicy>(row, bytes, canonical, LENIENT_INPUTS)
            }
            "0xb5" => {
                sweep_row_lenient::<Default8021QSettings>(row, bytes, canonical, LENIENT_INPUTS)
            }
            "0xb6" => sweep_row::<TrafficSeparationPolicy<'_>>(row, bytes, canonical),
            "0xc4" => sweep_row_lenient::<UnsuccessfulAssociationPolicy>(
                row,
                bytes,
                canonical,
                LENIENT_INPUTS,
            ),
            "0xd0" => sweep_row::<BackhaulBssConfiguration>(row, bytes, canonical),
            "0xb9" => sweep_row::<ServicePrioritizationRule>(row, bytes, canonical),
            "0xba" => sweep_row::<DscpMappingTable>(row, bytes, canonical),
            "0xdb" => sweep_row::<QosManagementPolicy>(row, bytes, canonical),
            "0xdc" => sweep_row::<QosManagementDescriptor<'_>>(row, bytes, canonical),
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

    fn steering_policy() -> SteeringPolicy {
        SteeringPolicy {
            local_steering_disallowed: vec![STA1, STA2],
            btm_steering_disallowed: vec![OTHER],
            radios: vec![
                RadioSteeringPolicy {
                    ruid: RUID0,
                    steering_policy: 1,
                    utilization_threshold: 156,
                    rcpi_threshold: 80,
                },
                RadioSteeringPolicy {
                    ruid: RUID1,
                    steering_policy: 2,
                    utilization_threshold: 200,
                    rcpi_threshold: 120,
                },
            ],
        }
    }

    fn radio_steering() -> RadioSteeringPolicy {
        RadioSteeringPolicy {
            ruid: RUID0,
            steering_policy: 1,
            utilization_threshold: 156,
            rcpi_threshold: 80,
        }
    }

    fn zero_radio_metrics() -> RadioMetricReportingPolicy {
        RadioMetricReportingPolicy {
            ruid: RUID0,
            sta_rcpi_threshold: 0,
            sta_rcpi_hysteresis: 0,
            ap_utilization_threshold: 0,
            include_sta_traffic_stats: false,
            include_sta_link_metrics: false,
            include_sta_status: false,
        }
    }

    fn metric_policy() -> MetricReportingPolicy {
        MetricReportingPolicy {
            interval: 30,
            radios: vec![
                RadioMetricReportingPolicy {
                    ruid: RUID0,
                    sta_rcpi_threshold: 120,
                    sta_rcpi_hysteresis: 5,
                    ap_utilization_threshold: 200,
                    include_sta_traffic_stats: true,
                    include_sta_link_metrics: true,
                    include_sta_status: false,
                },
                RadioMetricReportingPolicy {
                    ruid: RUID1,
                    include_sta_status: true,
                    ..zero_radio_metrics()
                },
            ],
        }
    }

    /// The radio of the `reserved-bits-*` rows of 0x8A, with the flags given.
    fn reserved_row_metrics(traffic: bool, link: bool, status: bool) -> MetricReportingPolicy {
        MetricReportingPolicy {
            interval: 5,
            radios: vec![RadioMetricReportingPolicy {
                ruid: RUID0,
                sta_rcpi_threshold: 100,
                sta_rcpi_hysteresis: 2,
                ap_utilization_threshold: 50,
                include_sta_traffic_stats: traffic,
                include_sta_link_metrics: link,
                include_sta_status: status,
            }],
        }
    }

    fn vlan_settings() -> Default8021QSettings {
        Default8021QSettings {
            vlan: 0x0123,
            pcp: 5,
        }
    }

    fn traffic_separation() -> TrafficSeparationPolicy<'static> {
        TrafficSeparationPolicy {
            ssids: vec![
                TrafficSeparationSsid {
                    ssid: b"HomeNet",
                    vlan: 0x0064,
                },
                TrafficSeparationSsid {
                    ssid: b"Guest",
                    vlan: 0x0ffe,
                },
            ],
        }
    }

    fn unsuccessful_policy() -> UnsuccessfulAssociationPolicy {
        UnsuccessfulAssociationPolicy {
            report: true,
            max_reporting_rate: 60,
        }
    }

    fn backhaul_config() -> BackhaulBssConfiguration {
        BackhaulBssConfiguration {
            bssid: BSSID,
            profile_1_bsta_disallowed: true,
            profile_2_bsta_disallowed: true,
        }
    }

    fn priority_rule() -> ServicePrioritizationRule {
        ServicePrioritizationRule {
            rule_id: 0x0102_0304,
            add: true,
            precedence: 200,
            output: 8,
            always_match: false,
        }
    }

    /// PCP 0 for DSCP 0-7, 1 for DSCP 8-15, and so on up to 7.
    fn dscp_golden() -> DscpMappingTable {
        let mut pcp = [0; DSCP_VALUES];
        for (dscp, value) in pcp.iter_mut().enumerate() {
            *value = u8::try_from(dscp / 8).unwrap();
        }
        DscpMappingTable { pcp }
    }

    fn qos_policy() -> QosManagementPolicy {
        QosManagementPolicy {
            mscs_disallowed: vec![STA1, STA2],
            scs_disallowed: vec![OTHER],
        }
    }

    fn descriptor() -> QosManagementDescriptor<'static> {
        QosManagementDescriptor {
            qmid: 0x1234,
            bssid: BSSID,
            client_mac: STA1,
            descriptor: &[
                0xff, 0x09, 0x58, 0x00, 0x02, 0x07, 0x04, 0x00, 0x00, 0x00, 0x0a,
            ],
        }
    }

    fn many_macs(count: usize) -> Vec<MacAddr> {
        vec![STA1; count]
    }

    fn steering_prefix_error(len: usize) -> DecodeError {
        truncated_in(&[
            field(0, 1),
            field(1, 6),
            field(7, 6),
            field(13, 1),
            field(14, 6),
            field(20, 1),
            field(21, 6),
            field(27, 1),
            field(28, 1),
            field(29, 1),
            field(30, 6),
            field(36, 1),
            field(37, 1),
            field(38, 1),
        ])(len)
    }

    fn metric_prefix_error(len: usize) -> DecodeError {
        truncated_in(&[
            field(0, 1),
            field(1, 1),
            field(2, 6),
            field(8, 1),
            field(9, 1),
            field(10, 1),
            field(11, 1),
            field(12, 6),
            field(18, 1),
            field(19, 1),
            field(20, 1),
            field(21, 1),
        ])(len)
    }

    fn traffic_prefix_error(len: usize) -> DecodeError {
        truncated_in(&[
            field(0, 1),
            field(1, 1),
            field(2, 7),
            field(9, 2),
            field(11, 1),
            field(12, 5),
            field(17, 2),
        ])(len)
    }

    fn qos_policy_prefix_error(len: usize) -> DecodeError {
        truncated_in(&[
            field(0, 1),
            field(1, 6),
            field(7, 6),
            field(13, 1),
            field(14, 6),
            field(20, 20),
        ])(len)
    }

    body_checks! {
        mod steering_policy_checks;
        body: SteeringPolicy;
        golden: &fixture(("0x89", "golden")), steering_policy();
        prefixes: |_| false, steering_prefix_error;
        trailing_byte: &fixture(("0x89", "golden"));
        count_too_large: SteeringPolicy { local_steering_disallowed: many_macs(256), ..steering_policy() }, TOO_MANY;
        alternates: [
            (&fixture(("0x89", "zero-all")), SteeringPolicy { local_steering_disallowed: vec![], btm_steering_disallowed: vec![], radios: vec![] }, &fixture(("0x89", "zero-all"))),
            (&fixture(("0x89", "policy-reserved-3")), SteeringPolicy { local_steering_disallowed: vec![], btm_steering_disallowed: vec![], radios: vec![RadioSteeringPolicy { steering_policy: 3, utilization_threshold: 100, rcpi_threshold: 90, ..radio_steering() }] }, &fixture(("0x89", "policy-reserved-3")))
        ];
    }
    body_checks! {
        mod metric_policy_checks;
        body: MetricReportingPolicy;
        golden: &fixture(("0x8a", "golden")), metric_policy();
        lenient;
        prefixes: |_| false, metric_prefix_error;
        trailing_byte: &fixture(("0x8a", "golden"));
        count_too_large: MetricReportingPolicy { interval: 0, radios: vec![zero_radio_metrics(); 256] }, TOO_MANY;
        reserved_bits: [
            (&fixture(("0x8a", "reserved-bits-set")), reserved_row_metrics(true, false, true), &fixture(("0x8a", "reserved-bits-clear"))),
            (&fixture(("0x8a", "reserved-bits-set-zero-flags")), reserved_row_metrics(false, false, false), &fixture(("0x8a", "no-flags")))
        ];
        alternates: [(&fixture(("0x8a", "zero-radios")), MetricReportingPolicy { interval: 0, radios: vec![] }, &fixture(("0x8a", "zero-radios")))];
        lenient_absent: [&fixture(("0x8a", "empty-upstream"))];
        lenient_rejects: [(&fixture(("0x8a", "missing-radio-count")), DecodeError::Truncated { needed: 1, remaining: 0 })];
    }
    body_checks! {
        mod vlan_settings_checks;
        body: Default8021QSettings;
        golden: &fixture(("0xb5", "golden")), vlan_settings();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0, 2), field(2, 1)]);
        trailing_byte: &fixture(("0xb5", "golden"));
        reserved_bits: [
            (&fixture(("0xb5", "reserved-bits-set")), vlan_settings(), &fixture(("0xb5", "reserved-bits-clear"))),
            (&fixture(("0xb5", "reserved-bits-set-pcp-zero")), Default8021QSettings { pcp: 0, ..vlan_settings() }, &[0x01, 0x23, 0x00])
        ];
        alternates: [(&fixture(("0xb5", "vlan-zero-upstream-m2")), Default8021QSettings { vlan: 0, pcp: 0 }, &fixture(("0xb5", "vlan-zero-upstream-m2")))];
        lenient_absent: [&fixture(("0xb5", "empty-upstream"))];
        lenient_rejects: [(&fixture(("0xb5", "missing-pcp-octet")), DecodeError::Truncated { needed: 1, remaining: 0 })];
    }
    body_checks! {
        mod traffic_separation_checks;
        body: TrafficSeparationPolicy<'a>;
        golden: &fixture(("0xb6", "golden")), traffic_separation();
        prefixes: |_| false, traffic_prefix_error;
        trailing_byte: &fixture(("0xb6", "golden"));
        count_too_large: TrafficSeparationPolicy { ssids: vec![TrafficSeparationSsid { ssid: b"A", vlan: 3 }; 256] }, TOO_MANY;
        alternates: [
            (&fixture(("0xb6", "zero-ssids")), TrafficSeparationPolicy { ssids: vec![] }, &fixture(("0xb6", "zero-ssids"))),
            (&fixture(("0xb6", "empty-ssid-name")), TrafficSeparationPolicy { ssids: vec![TrafficSeparationSsid { ssid: b"", vlan: 0x0010 }] }, &fixture(("0xb6", "empty-ssid-name")))
        ];
    }
    body_checks! {
        mod unsuccessful_policy_checks;
        body: UnsuccessfulAssociationPolicy;
        golden: &fixture(("0xc4", "golden")), unsuccessful_policy();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 4)]);
        trailing_byte: &fixture(("0xc4", "golden"));
        reserved_bits: [
            (&fixture(("0xc4", "reserved-bits-set")), unsuccessful_policy(), &fixture(("0xc4", "reserved-bits-clear"))),
            (&fixture(("0xc4", "reserved-bits-set-report-off")), UnsuccessfulAssociationPolicy { report: false, ..unsuccessful_policy() }, &fixture(("0xc4", "report-off")))
        ];
        alternates: [(&fixture(("0xc4", "rate-above-65535")), UnsuccessfulAssociationPolicy { report: true, max_reporting_rate: 100_000 }, &fixture(("0xc4", "rate-above-65535")))];
        lenient_absent: [&fixture(("0xc4", "empty-upstream"))];
        lenient_rejects: [(&fixture(("0xc4", "cut-rate")), DecodeError::Truncated { needed: 4, remaining: 3 })];
    }
    body_checks! {
        mod backhaul_config_checks;
        body: BackhaulBssConfiguration;
        golden: &fixture(("0xd0", "golden")), backhaul_config();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1)]);
        trailing_byte: &fixture(("0xd0", "golden"));
        reserved_bits: [
            (&fixture(("0xd0", "reserved-bits-set")), BackhaulBssConfiguration { profile_2_bsta_disallowed: false, ..backhaul_config() }, &fixture(("0xd0", "reserved-bits-clear"))),
            (&fixture(("0xd0", "reserved-bits-set-zero-flags")), BackhaulBssConfiguration { profile_1_bsta_disallowed: false, profile_2_bsta_disallowed: false, ..backhaul_config() }, &[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0x00])
        ];
        alternates: [(&fixture(("0xd0", "zero-bssid")), BackhaulBssConfiguration { bssid: MacAddr::new([0; 6]), profile_1_bsta_disallowed: true, profile_2_bsta_disallowed: false }, &fixture(("0xd0", "zero-bssid")))];
    }
    body_checks! {
        mod priority_rule_checks;
        body: ServicePrioritizationRule;
        golden: &fixture(("0xb9", "golden")), priority_rule();
        prefixes: |_| false, truncated_in(&[field(0, 4), field(4, 1), field(5, 1), field(6, 1), field(7, 1)]);
        trailing_byte: &fixture(("0xb9", "golden"));
        reserved_bits: [
            (&fixture(("0xb9", "add-reserved-set")), priority_rule(), &fixture(("0xb9", "add-reserved-clear"))),
            (&fixture(("0xb9", "always-reserved-set")), priority_rule(), &fixture(("0xb9", "always-reserved-clear"))),
            (&fixture(("0xb9", "both-reserved-set-zero-flags")), ServicePrioritizationRule { add: false, ..priority_rule() }, &[0x01, 0x02, 0x03, 0x04, 0x00, 0xc8, 0x08, 0x00])
        ];
        alternates: [(&fixture(("0xb9", "output-reserved-ff")), ServicePrioritizationRule { output: 0xff, ..priority_rule() }, &fixture(("0xb9", "output-reserved-ff")))];
    }
    body_checks! {
        mod dscp_table_checks;
        body: DscpMappingTable;
        golden: &fixture(("0xba", "golden")), dscp_golden();
        prefixes: |_| false, truncated_in(&[field(0, 64)]);
        trailing_byte: &fixture(("0xba", "golden"));
        alternates: [(&fixture(("0xba", "all-zero")), DscpMappingTable { pcp: [0; DSCP_VALUES] }, &fixture(("0xba", "all-zero")))];
    }
    body_checks! {
        mod qos_policy_checks;
        body: QosManagementPolicy;
        golden: &fixture(("0xdb", "golden")), qos_policy();
        prefixes: |_| false, qos_policy_prefix_error;
        trailing_byte: &fixture(("0xdb", "golden"));
        count_too_large: QosManagementPolicy { mscs_disallowed: many_macs(256), scs_disallowed: vec![] }, TOO_MANY;
        reserved_bits: [
            (&fixture(("0xdb", "reserved-octets-set")), QosManagementPolicy { mscs_disallowed: vec![STA1], scs_disallowed: vec![OTHER] }, &fixture(("0xdb", "reserved-octets-clear")))
        ];
        alternates: [(&fixture(("0xdb", "zero-both")), QosManagementPolicy { mscs_disallowed: vec![], scs_disallowed: vec![] }, &fixture(("0xdb", "zero-both")))];
    }
    body_checks! {
        mod descriptor_checks;
        body: QosManagementDescriptor<'a>;
        golden: &fixture(("0xdc", "golden")), descriptor();
        // The descriptor is the rest of the value, so every length from the fixed head on is complete.
        prefixes: |len| len >= 14, truncated_in(&[field(0, 2), field(2, 6), field(8, 6)]);
        alternates: [(&fixture(("0xdc", "empty-descriptor")), QosManagementDescriptor { descriptor: &[], ..descriptor() }, &fixture(("0xdc", "empty-descriptor")))];
    }

    /// A wire field: octet offset in the value, bit mask, and a setter for the matching field.
    type Flag<B> = (usize, u8, fn(&mut B));

    /// Checks that each flag, set alone in an otherwise zero value, lands on its own wire bit
    /// and nowhere else. Fixture rows that set several flags at once cannot tell two flags
    /// apart when the pair is always set or clear together.
    fn assert_each_flag_alone<B>(zero_bytes: &[u8], zero_body: fn() -> B, flags: &[Flag<B>])
    where
        B: for<'a> TlvBody<'a> + core::fmt::Debug + PartialEq,
    {
        assert_eq!(B::decode_value(zero_bytes), Ok(zero_body()));
        for &(offset, mask, set) in flags {
            let mut bytes = zero_bytes.to_vec();
            bytes[offset] |= mask;
            let mut expected = zero_body();
            set(&mut expected);
            assert_eq!(
                B::decode_value(&bytes),
                Ok(expected),
                "decode, octet {offset} mask {mask:#04x}"
            );
            let mut expected = zero_body();
            set(&mut expected);
            assert_eq!(
                expected.encode_value(),
                Ok(bytes),
                "encode, octet {offset} mask {mask:#04x}"
            );
        }
    }

    #[test]
    fn metric_reporting_flags_each_use_their_own_bit() {
        let flags: &[Flag<MetricReportingPolicy>] = &[
            (11, 0x80, |b| b.radios[0].include_sta_traffic_stats = true),
            (11, 0x40, |b| b.radios[0].include_sta_link_metrics = true),
            (11, 0x20, |b| b.radios[0].include_sta_status = true),
        ];
        assert_each_flag_alone(
            &[0, 1, 0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0, 0, 0, 0, 0],
            || MetricReportingPolicy {
                interval: 0,
                radios: vec![zero_radio_metrics()],
            },
            flags,
        );
    }

    #[test]
    fn default_pcp_uses_bits_7_to_5() {
        let flags: &[Flag<Default8021QSettings>] = &[
            (2, 0x20, |b| b.pcp = 1),
            (2, 0x40, |b| b.pcp = 2),
            (2, 0x80, |b| b.pcp = 4),
        ];
        assert_each_flag_alone(
            &[0x01, 0x23, 0x00],
            || Default8021QSettings {
                vlan: 0x0123,
                pcp: 0,
            },
            flags,
        );
    }

    #[test]
    fn report_uses_bit_7_alone() {
        let flags: &[Flag<UnsuccessfulAssociationPolicy>] = &[(0, 0x80, |b| b.report = true)];
        assert_each_flag_alone(
            &[0, 0, 0, 0, 0],
            || UnsuccessfulAssociationPolicy {
                report: false,
                max_reporting_rate: 0,
            },
            flags,
        );
    }

    #[test]
    fn backhaul_profile_flags_each_use_their_own_bit() {
        let flags: &[Flag<BackhaulBssConfiguration>] = &[
            (6, 0x80, |b| b.profile_1_bsta_disallowed = true),
            (6, 0x40, |b| b.profile_2_bsta_disallowed = true),
        ];
        assert_each_flag_alone(
            &[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0],
            || BackhaulBssConfiguration {
                bssid: BSSID,
                profile_1_bsta_disallowed: false,
                profile_2_bsta_disallowed: false,
            },
            flags,
        );
    }

    #[test]
    fn rule_add_and_always_match_each_use_bit_7_of_their_own_octet() {
        let flags: &[Flag<ServicePrioritizationRule>] = &[
            (4, 0x80, |b| b.add = true),
            (7, 0x80, |b| b.always_match = true),
        ];
        assert_each_flag_alone(
            &[0x01, 0x02, 0x03, 0x04, 0, 0, 0, 0],
            || ServicePrioritizationRule {
                rule_id: 0x0102_0304,
                add: false,
                precedence: 0,
                output: 0,
                always_match: false,
            },
            flags,
        );
    }

    #[test]
    fn pcp_above_7_does_not_encode() {
        let body = Default8021QSettings {
            pcp: 8,
            ..vlan_settings()
        };
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 8, max: 7 })
        );
        let body = Default8021QSettings {
            pcp: 7,
            ..vlan_settings()
        };
        assert_eq!(body.encode_value(), Ok(vec![0x01, 0x23, 0xe0]));
    }

    #[test]
    fn ssid_above_255_octets_does_not_encode() {
        let too_long = [b'x'; 256];
        let body = TrafficSeparationPolicy {
            ssids: vec![TrafficSeparationSsid {
                ssid: &too_long,
                vlan: 3,
            }],
        };
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::CountTooLarge { len: 256, max: 255 })
        );
        let longest = [b'x'; 255];
        let body = TrafficSeparationPolicy {
            ssids: vec![TrafficSeparationSsid {
                ssid: &longest,
                vlan: 3,
            }],
        };
        let bytes = body.encode_value().unwrap();
        assert_eq!(TrafficSeparationPolicy::decode_value(&bytes), Ok(body));
    }

    #[test]
    fn every_list_over_255_does_not_encode() {
        let mut body = steering_policy();
        body.btm_steering_disallowed = many_macs(256);
        assert_eq!(body.encode_value(), Err(TOO_MANY));
        let mut body = steering_policy();
        body.radios = vec![radio_steering(); 256];
        assert_eq!(body.encode_value(), Err(TOO_MANY));
        let body = QosManagementPolicy {
            mscs_disallowed: vec![],
            scs_disallowed: many_macs(256),
        };
        assert_eq!(body.encode_value(), Err(TOO_MANY));
    }

    #[test]
    fn counts_above_one_hundred_and_up_to_255_round_trip() {
        let body = SteeringPolicy {
            local_steering_disallowed: many_macs(255),
            btm_steering_disallowed: many_macs(255),
            radios: vec![radio_steering(); 255],
        };
        let bytes = body.encode_value().unwrap();
        assert_eq!(SteeringPolicy::decode_value(&bytes), Ok(body));
    }

    #[test]
    fn steering_policy_accepts_counts_above_the_upstream_arrays() {
        // intentional-difference: upstream em_policy_cfg.cpp:747-768 copies Num_Local and
        // Num_BTM entries into 16-entry arrays and Num_Radio entries into a 4-entry array, and
        // overflows its stack object above that (audit 53, 54). The port keeps them all.
        let locals = SteeringPolicy::decode_value(&fixture(("0x89", "seventeen-locals"))).unwrap();
        assert_eq!(locals.local_steering_disallowed.len(), 17);
        let btms = SteeringPolicy::decode_value(&fixture(("0x89", "seventeen-btms"))).unwrap();
        assert_eq!(btms.btm_steering_disallowed.len(), 17);
        let radios = SteeringPolicy::decode_value(&fixture(("0x89", "five-radios"))).unwrap();
        assert_eq!(radios.radios.len(), 5);
    }

    #[test]
    fn metric_reporting_policy_accepts_more_radios_than_the_upstream_array() {
        // intentional-difference: upstream em_policy_cfg.cpp:770-784 copies Num_Radio entries
        // into a 4-entry array and overflows its stack object above that (audit 58).
        let body = MetricReportingPolicy::decode_value(&fixture(("0x8a", "five-radios"))).unwrap();
        assert_eq!(body.radios.len(), 5);
        assert!(body.radios[1].include_sta_traffic_stats);
        assert_eq!(body.radios[4].sta_rcpi_hysteresis, 4);
    }

    #[test]
    fn traffic_separation_policy_parses_more_ssids_than_upstream_does() {
        // intentional-difference: upstream em_policy_cfg.cpp:795-796 parses the entries only if
        // Num_SSID is at most 5, so a TLV with 6 SSIDs is silently dropped (audit 177).
        let value = fixture(("0xb6", "six-ssids"));
        let body = TrafficSeparationPolicy::decode_value(&value).unwrap();
        assert_eq!(body.ssids.len(), 6);
        assert_eq!(body.ssids[5].ssid, b"S5");
        assert_eq!(body.ssids[5].vlan, 0x0105);
        assert_eq!(body.encode_value(), Ok(value));
    }

    #[test]
    fn qos_management_policy_keeps_more_than_16_macs_per_list() {
        // intentional-difference: upstream sends at most 16 MACs per list and drops the rest
        // (em_policy_cfg.cpp:357-362, audit 271).
        let body = QosManagementPolicy::decode_value(&fixture(("0xdb", "seventeen-mscs"))).unwrap();
        assert_eq!(body.mscs_disallowed.len(), 17);
        let body = QosManagementPolicy {
            mscs_disallowed: many_macs(255),
            scs_disallowed: many_macs(255),
        };
        let bytes = body.encode_value().unwrap();
        assert_eq!(QosManagementPolicy::decode_value(&bytes), Ok(body));
    }

    #[test]
    fn reporting_rate_keeps_32_bits_where_upstream_truncates_to_16() {
        // intentional-difference: upstream em_policy_cfg.cpp:840-841 casts the rate to
        // unsigned short, so 100000 becomes 34464 (audit 194).
        let value = fixture(("0xc4", "rate-above-65535"));
        let body = UnsuccessfulAssociationPolicy::decode_value(&value).unwrap();
        assert_eq!(body.max_reporting_rate, 100_000);
        assert_eq!(body.encode_value(), Ok(value));
        let max = fixture(("0xc4", "rate-max"));
        let body = UnsuccessfulAssociationPolicy::decode_value(&max).unwrap();
        assert_eq!(body.max_reporting_rate, u32::MAX);
        assert_eq!(body.encode_value(), Ok(max));
    }

    #[test]
    fn upstream_sends_an_empty_metric_reporting_policy_when_it_has_none() {
        // intentional-difference: upstream em_policy_cfg.cpp:44-145 gives no policy and the caller
        // at :546-556 still emits the TLV with length 0 (audit 57). The lenient decoder reads that
        // as absent; the strict decoder rejects it.
        assert_eq!(MetricReportingPolicy::decode_value_lenient(&[]), Ok(None));
        assert_eq!(
            MetricReportingPolicy::decode_value(&[]),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
    }

    #[test]
    fn upstream_sends_an_empty_default_8021q_settings_when_it_has_none() {
        // intentional-difference: upstream em.cpp:2109-2138 gives no policy and the caller at
        // em_policy_cfg.cpp:561-570 still emits the TLV with length 0 (audit 173).
        assert_eq!(Default8021QSettings::decode_value_lenient(&[]), Ok(None));
        assert_eq!(
            Default8021QSettings::decode_value(&[]),
            Err(DecodeError::Truncated {
                needed: 2,
                remaining: 0
            })
        );
    }

    #[test]
    fn upstream_sends_an_empty_unsuccessful_association_policy_when_it_has_none() {
        // intentional-difference: upstream em_policy_cfg.cpp:304-332 gives no policy and the
        // caller at :604-616 still emits the TLV with length 0.
        assert_eq!(
            UnsuccessfulAssociationPolicy::decode_value_lenient(&[]),
            Ok(None)
        );
        assert_eq!(
            UnsuccessfulAssociationPolicy::decode_value(&[]),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
    }

    #[test]
    fn vlan_zero_is_kept_in_default_settings_and_traffic_separation() {
        // Note 0009 D3 (audit 174, 179): the codec carries every VLAN ID, reserved ones included.
        let value = fixture(("0xb5", "vlan-zero-upstream-m2"));
        let body = Default8021QSettings::decode_value(&value).unwrap();
        assert_eq!(body, Default8021QSettings { vlan: 0, pcp: 0 });
        assert_eq!(body.encode_value(), Ok(value));
        let value = fixture(("0xb6", "vlan-untagged-zero"));
        let body = TrafficSeparationPolicy::decode_value(&value).unwrap();
        assert_eq!(body.ssids[0].vlan, 0);
        assert_eq!(body.encode_value(), Ok(value));
    }

    #[test]
    fn ssids_are_raw_bytes() {
        // intentional-difference: upstream em_policy_cfg.cpp:804 skips an SSID longer than
        // MAX_SSID_NAME_LEN without advancing (audit 178); the 33-octet case is parsed here.
        // Length 0, 32, 33 and a trailing NUL are all kept as given (note 0009 D2).
        for case in [
            "empty-ssid-name",
            "ssid-32-octets",
            "ssid-33-octets",
            "ssid-trailing-nul",
        ] {
            let value = fixture(("0xb6", case));
            let body = TrafficSeparationPolicy::decode_value(&value).unwrap();
            assert_eq!(body.encode_value(), Ok(value), "{case}");
        }
        let nul_value = fixture(("0xb6", "ssid-trailing-nul"));
        let nul = TrafficSeparationPolicy::decode_value(&nul_value).unwrap();
        assert_eq!(nul.ssids[0].ssid, b"Guest\0");
    }

    #[test]
    fn reserved_values_are_kept_raw() {
        let value = fixture(("0x89", "rcpi-reserved-ff"));
        let body = SteeringPolicy::decode_value(&value).unwrap();
        assert_eq!(body.radios[0].rcpi_threshold, 0xff);
        assert_eq!(body.encode_value(), Ok(value));
        let value = fixture(("0x8a", "rcpi-reserved-ff"));
        let body = MetricReportingPolicy::decode_value(&value).unwrap();
        assert_eq!(body.radios[0].sta_rcpi_threshold, 0xff);
        assert_eq!(body.encode_value(), Ok(value));
        let value = fixture(("0xb9", "precedence-reserved-ff"));
        let body = ServicePrioritizationRule::decode_value(&value).unwrap();
        assert_eq!(body.precedence, 0xff);
        assert_eq!(body.encode_value(), Ok(value));
        let value = fixture(("0xba", "reserved-values"));
        let body = DscpMappingTable::decode_value(&value).unwrap();
        assert_eq!((body.pcp[0], body.pcp[63]), (8, 255));
        assert_eq!(body.encode_value(), Ok(value));
    }

    #[test]
    fn multi_octet_fields_are_big_endian() {
        let rule = ServicePrioritizationRule {
            rule_id: 0x0a0b_0c0d,
            ..priority_rule()
        };
        assert_eq!(rule.encode_value().unwrap()[0..4], [0x0a, 0x0b, 0x0c, 0x0d]);
        let policy = UnsuccessfulAssociationPolicy {
            report: false,
            max_reporting_rate: 0x0a0b_0c0d,
        };
        assert_eq!(
            policy.encode_value().unwrap()[1..5],
            [0x0a, 0x0b, 0x0c, 0x0d]
        );
        let settings = Default8021QSettings {
            vlan: 0x0102,
            pcp: 0,
        };
        assert_eq!(settings.encode_value().unwrap()[0..2], [0x01, 0x02]);
        let traffic = TrafficSeparationPolicy {
            ssids: vec![TrafficSeparationSsid {
                ssid: b"",
                vlan: 0x0102,
            }],
        };
        assert_eq!(traffic.encode_value().unwrap()[2..4], [0x01, 0x02]);
        let body = QosManagementDescriptor {
            qmid: 0x0102,
            ..descriptor()
        };
        assert_eq!(body.encode_value().unwrap()[0..2], [0x01, 0x02]);
    }

    #[test]
    fn qos_policy_reserved_octets_are_ignored_and_written_as_zero() {
        let set = fixture(("0xdb", "reserved-octets-set"));
        let body = QosManagementPolicy::decode_value(&set).unwrap();
        let bytes = body.encode_value().unwrap();
        assert_eq!(bytes[bytes.len() - 20..], [0; 20]);
        assert_eq!(bytes.len(), set.len());
    }

    #[test]
    fn qos_policy_with_19_reserved_octets_is_truncated() {
        assert_eq!(
            QosManagementPolicy::decode_value(&fixture(("0xdb", "reserved-19-octets"))),
            Err(DecodeError::Truncated {
                needed: 20,
                remaining: 19
            })
        );
    }

    #[test]
    fn dscp_table_is_exactly_64_octets() {
        assert_eq!(dscp_golden().encode_value().unwrap().len(), 64);
        assert_eq!(
            DscpMappingTable::decode_value(&fixture(("0xba", "sixty-three-octets"))),
            Err(DecodeError::Truncated {
                needed: 64,
                remaining: 63
            })
        );
    }

    #[test]
    fn backhaul_two_entries_in_one_tlv_is_trailing_bytes() {
        assert_eq!(
            BackhaulBssConfiguration::decode_value(&fixture(("0xd0", "two-entries-one-tlv"))),
            Err(DecodeError::TrailingBytes { remaining: 7 })
        );
    }

    #[test]
    fn descriptor_element_is_the_rest_of_the_value() {
        let value = fixture(("0xdc", "empty-descriptor"));
        let body = QosManagementDescriptor::decode_value(&value).unwrap();
        assert_eq!(body.descriptor, &[] as &[u8]);
        let value = fixture(("0xdc", "qos-mgmt-element"));
        let body = QosManagementDescriptor::decode_value(&value).unwrap();
        assert_eq!(body.descriptor, &value[14..]);
        assert_eq!(body.qmid, 0x0a0b);
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<RadioSteeringPolicy>() <= 9);
        assert!(size_of::<SteeringPolicy>() <= 9 * word);
        assert!(size_of::<RadioMetricReportingPolicy>() <= 12);
        assert!(size_of::<MetricReportingPolicy>() <= 4 * word);
        assert!(size_of::<Default8021QSettings>() <= 4);
        assert!(size_of::<TrafficSeparationSsid<'_>>() <= 3 * word);
        assert!(size_of::<TrafficSeparationPolicy<'_>>() <= 3 * word);
        assert!(size_of::<UnsuccessfulAssociationPolicy>() <= 8);
        assert!(size_of::<BackhaulBssConfiguration>() <= 8);
        assert!(size_of::<ServicePrioritizationRule>() <= 12);
        assert!(size_of::<DscpMappingTable>() <= 64);
        assert!(size_of::<QosManagementPolicy>() <= 6 * word);
        assert!(size_of::<QosManagementDescriptor<'_>>() <= 2 * word + 16);
    }
}
