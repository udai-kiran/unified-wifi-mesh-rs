// SPDX-License-Identifier: Apache-2.0

//! `EasyMesh` capability TLV bodies.

use em_types::{MacAddr, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

fn bit(value: u8, shift: u8) -> bool {
    value & (1 << shift) != 0
}

fn field(value: u8, max: u8, shift: u8) -> Result<u8, EncodeError> {
    if value > max {
        return Err(EncodeError::ValueTooLarge {
            value: u32::from(value),
            max: u32::from(max),
        });
    }
    Ok(value << shift)
}

macro_rules! lenient_empty {
    ($body:ty) => {
        impl<'a> TlvBodyLenient<'a> for $body {
            fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
                if value.is_empty() {
                    Ok(None)
                } else {
                    Self::decode_value(value).map(Some)
                }
            }
        }
    };
}

/// One operating class in AP Radio Basic Capabilities, `EasyMesh` §17.2.7 Table 29.
/// Upstream `em_op_class_t` (`em_base.h:529-534`), used by `em_ap_radio_basic_cap_t`, sender
/// `em.cpp:1086-1135`, receiver `em_capability.cpp:1262-1320`; audit rows 40–44.
/// Groups are contiguous, unlike upstream's holes (`em.cpp:1105-1130`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasicOperatingClass<'a> {
    /// Operating class code.
    pub operating_class: u8,
    /// Maximum EIRP in dBm, a signed two's-complement value.
    pub max_tx_power: i8,
    /// Statically non-operable channels.
    pub non_operable_channels: &'a [u8],
}

/// AP Radio Basic Capabilities, `EasyMesh` §17.2.7 Table 29; audit rows 40–44.
/// Upstream `em_ap_radio_basic_cap_t` (`em_base.h:552-557`), sender
/// `em.cpp:1086-1135`; receiver `em_capability.cpp:1262-1320`, `em_configuration.cpp:6102-6160`.
/// Audit 40: groups are contiguous instead of the sender's holed layout (`em.cpp:1105-1130`);
/// holes are ambiguous, so no lenient decoder is provided. `max_bss` is the supported maximum,
/// not upstream's configured BSS count (audit 44); zero is carried raw. Transmit power
/// is signed (audit 43), and counts are bounded by the value (audit 41–42).
#[derive(Debug, PartialEq, Eq)]
pub struct ApRadioBasicCapabilities<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Maximum number of BSSs (zero is carried as received).
    pub max_bss: u8,
    /// Operating classes in wire order.
    pub operating_classes: Vec<BasicOperatingClass<'a>>,
}

impl<'a> TlvBody<'a> for ApRadioBasicCapabilities<'a> {
    const TLV_TYPE: TlvType = TlvType::AP_RADIO_BASIC_CAPABILITIES;
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let max_bss = reader.u8()?;
        let count = usize::from(reader.u8()?);
        let mut operating_classes = bounded_vec(count, reader.remaining(), 3);
        for _ in 0..count {
            let operating_class = reader.u8()?;
            let max_tx_power = i8::from_ne_bytes([reader.u8()?]);
            let count = usize::from(reader.u8()?);
            let non_operable_channels = reader.bytes(count)?;
            operating_classes.push(BasicOperatingClass {
                operating_class,
                max_tx_power,
                non_operable_channels,
            });
        }
        reader.finish()?;
        Ok(Self {
            ruid,
            max_bss,
            operating_classes,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(self.max_bss);
        writer.count_u8(self.operating_classes.len())?;
        for entry in &self.operating_classes {
            writer.u8(entry.operating_class);
            writer.u8(entry.max_tx_power.to_ne_bytes()[0]);
            writer.count_u8(entry.non_operable_channels.len())?;
            writer.bytes(entry.non_operable_channels);
        }
        Ok(writer.into_bytes())
    }
}

/// Client Info, `EasyMesh` §17.2.18 Table 40; audit rows 77–78.
/// Upstream `em_client_info_t` (`em_base.h:1362-1365`), sender
/// `em_capability.cpp:586-600`, receiver `:1157-1184`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientInfo {
    /// BSSID or AP MLD address.
    pub bssid: MacAddr,
    /// Station or station MLD address.
    pub station: MacAddr,
}
impl TlvBody<'_> for ClientInfo {
    const TLV_TYPE: TlvType = TlvType::CLIENT_INFO;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let body = Self {
            bssid: reader.mac()?,
            station: reader.mac()?,
        };
        reader.finish()?;
        Ok(body)
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.mac(self.station);
        Ok(writer.into_bytes())
    }
}

/// Client Capability Report, `EasyMesh` §17.2.19 Table 41; audit rows 79–80.
/// Upstream `em_client_cap_rprt_t` (`em_base.h:1367-1371`), sender
/// `em_capability.cpp:542-580`, receiver `:926-1017`. A nonzero result carries no
/// frame, including reserved result codes; the borrowed frame is raw 802.11 data.
/// The upstream sender can return zero octets, but its own validation then fails and the report
/// is not sent (audit 80). Strict decoding requires a result byte, and there is no lenient form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientCapabilityReport<'a> {
    result_code: u8,
    frame_body: Option<&'a [u8]>,
}
impl<'a> ClientCapabilityReport<'a> {
    /// Creates a report if and only if code zero has a frame and other codes do not.
    #[must_use]
    pub const fn new(result_code: u8, frame_body: Option<&'a [u8]>) -> Option<Self> {
        if (result_code == 0) == frame_body.is_some() {
            Some(Self {
                result_code,
                frame_body,
            })
        } else {
            None
        }
    }
    /// Returns the result code, including reserved codes.
    #[must_use]
    pub const fn result_code(&self) -> u8 {
        self.result_code
    }
    /// Returns the frame body on success.
    #[must_use]
    pub const fn frame_body(&self) -> Option<&'a [u8]> {
        self.frame_body
    }
}
impl<'a> TlvBody<'a> for ClientCapabilityReport<'a> {
    const TLV_TYPE: TlvType = TlvType::CLIENT_CAPABILITY_REPORT;
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let result_code = reader.u8()?;
        let frame_body = if result_code == 0 {
            Some(reader.rest())
        } else {
            None
        };
        reader.finish()?;
        Ok(Self {
            result_code,
            frame_body,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.result_code);
        if let Some(frame) = self.frame_body {
            writer.bytes(frame);
        }
        Ok(writer.into_bytes())
    }
}

/// Backhaul STA Radio Capabilities, `EasyMesh` §17.2.65 Table 87; audit rows 207–210.
/// Upstream `em_bh_sta_radio_cap_t` (`em_base.h:1738-1743`), senders
/// `em_capability.cpp:619-659,861`, `em_configuration.cpp:3871-3911`; receivers
/// `em_capability.cpp:1099-1155` (`handle_bsta_radio_cap`) and `:1236-1244`. The optional MAC
/// determines Included;
/// empty upstream values decode leniently to absence (note 0009 D4). Reserved bits
/// are zero instead of uninitialized stack data (audit 208). Unlike the upstream
/// sender, the Included=0 form omits the MAC (audit 209–210); Included=1 with an
/// all-zero MAC is also preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackhaulStaRadioCapabilities {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Backhaul STA MAC, when Included is set.
    pub backhaul_sta: Option<MacAddr>,
}
impl TlvBody<'_> for BackhaulStaRadioCapabilities {
    const TLV_TYPE: TlvType = TlvType::BACKHAUL_STA_RADIO_CAPABILITIES;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let included = bit(reader.u8()?, 7);
        let backhaul_sta = if included { Some(reader.mac()?) } else { None };
        reader.finish()?;
        Ok(Self { ruid, backhaul_sta })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(u8::from(self.backhaul_sta.is_some()) << 7);
        if let Some(mac) = self.backhaul_sta {
            writer.mac(mac);
        }
        Ok(writer.into_bytes())
    }
}
lenient_empty!(BackhaulStaRadioCapabilities);

/// AP Capability, `EasyMesh` §17.2.6 Table 28; audit rows 38–39.
/// Upstream `em_ap_capability_t` (`em_base.h:2209-2215`), sender `em.cpp:1137-1155`;
/// receiver `em_capability.cpp:1523-1540,1801-1816`. Table reserved bits are interpreted as 3–0;
/// a missing capability byte is rejected even when upstream emits an empty value (audit 39).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent wire capability bits.
pub struct ApCapability {
    /// Unassociated STA metrics on operating channels.
    pub on_channel_metrics: bool,
    /// Unassociated STA metrics on other channels.
    pub off_channel_metrics: bool,
    /// Agent-initiated RCPI steering.
    pub rcpi_steering: bool,
    /// Backhaul STA reconfiguration using M8.
    pub m8_backhaul_reconfiguration: bool,
}
impl TlvBody<'_> for ApCapability {
    const TLV_TYPE: TlvType = TlvType::AP_CAPABILITY;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let flags1 = reader.u8()?;
        let on_channel_metrics = bit(flags1, 7);
        let off_channel_metrics = bit(flags1, 6);
        let rcpi_steering = bit(flags1, 5);
        let m8_backhaul_reconfiguration = bit(flags1, 4);
        reader.finish()?;
        Ok(Self {
            on_channel_metrics,
            off_channel_metrics,
            rcpi_steering,
            m8_backhaul_reconfiguration,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8((u8::from(self.on_channel_metrics) << 7)
            | (u8::from(self.off_channel_metrics) << 6)
            | (u8::from(self.rcpi_steering) << 5)
            | (u8::from(self.m8_backhaul_reconfiguration) << 4));
        Ok(writer.into_bytes())
    }
}

/// AP HT Capabilities, `EasyMesh` §17.2.8 Table 30; audit rows 45–47.
/// Upstream `em_ap_ht_cap_t` (`em_base.h:1881-1889`), sender `em.cpp:1276-1300`;
/// receivers `em_capability.cpp:1544-1568,1823-1847`. An empty value is lenient (0009 A5): the
/// sender `em.cpp:1283-1293` returns 0 when radio data is missing, and the callers
/// `em_capability.cpp:144-169,359-383` still add the TLV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApHtCapabilities {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Transmit spatial streams minus one.
    pub max_tx_streams_code: u8,
    /// Receive spatial streams minus one.
    pub max_rx_streams_code: u8,
    /// Short guard interval for 20 MHz.
    pub short_gi_20: bool,
    /// Short guard interval for 40 MHz.
    pub short_gi_40: bool,
    /// HT support for 40 MHz.
    pub ht_40: bool,
}
impl TlvBody<'_> for ApHtCapabilities {
    const TLV_TYPE: TlvType = TlvType::AP_HT_CAPABILITIES;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let flags1 = reader.u8()?;
        let short_gi_20 = bit(flags1, 3);
        let short_gi_40 = bit(flags1, 2);
        let ht_40 = bit(flags1, 1);
        reader.finish()?;
        Ok(Self {
            ruid,
            max_tx_streams_code: (flags1 >> 6) & 3,
            max_rx_streams_code: (flags1 >> 4) & 3,
            short_gi_20,
            short_gi_40,
            ht_40,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(field(self.max_tx_streams_code, 3, 6)?
            | field(self.max_rx_streams_code, 3, 4)?
            | (u8::from(self.short_gi_20) << 3)
            | (u8::from(self.short_gi_40) << 2)
            | (u8::from(self.ht_40) << 1));
        Ok(writer.into_bytes())
    }
}
lenient_empty!(ApHtCapabilities);

/// AP VHT Capabilities, `EasyMesh` §17.2.9 Table 31; audit rows 48–52.
/// Upstream `em_ap_vht_cap_t` (`em_base.h:1891-1904`), sender `em.cpp:1369-1391`;
/// receivers `em_capability.cpp:1569-1581,1848-1860`. Audit 48 uses big-endian maps, unlike
/// upstream's host-order struct copy (`em.cpp:1388`); stream codes are streams minus one. An
/// empty value is lenient (0009 A5): the sender `em.cpp:1376-1386` returns 0 when radio data is
/// missing, and the callers `em_capability.cpp:144-169,359-383` still add the TLV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent wire capability bits.
pub struct ApVhtCapabilities {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// VHT transmit MCS map.
    pub tx_mcs: u16,
    /// VHT receive MCS map.
    pub rx_mcs: u16,
    /// Transmit spatial streams minus one.
    pub max_tx_streams_code: u8,
    /// Receive spatial streams minus one.
    pub max_rx_streams_code: u8,
    /// Short guard interval for 80 MHz.
    pub short_gi_80: bool,
    /// Short guard interval for 160 and 80+80 MHz.
    pub short_gi_160: bool,
    /// VHT support for 80+80 MHz.
    pub vht_80p80: bool,
    /// VHT support for 160 MHz.
    pub vht_160: bool,
    /// SU beamformer support.
    pub su_beamformer: bool,
    /// MU beamformer support.
    pub mu_beamformer: bool,
}
impl TlvBody<'_> for ApVhtCapabilities {
    const TLV_TYPE: TlvType = TlvType::AP_VHT_CAPABILITIES;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let tx_mcs = reader.be_u16()?;
        let rx_mcs = reader.be_u16()?;
        let flags1 = reader.u8()?;
        let short_gi_80 = bit(flags1, 1);
        let short_gi_160 = bit(flags1, 0);
        let flags2 = reader.u8()?;
        let vht_80p80 = bit(flags2, 7);
        let vht_160 = bit(flags2, 6);
        let su_beamformer = bit(flags2, 5);
        let mu_beamformer = bit(flags2, 4);
        reader.finish()?;
        Ok(Self {
            ruid,
            tx_mcs,
            rx_mcs,
            max_tx_streams_code: (flags1 >> 5) & 7,
            max_rx_streams_code: (flags1 >> 2) & 7,
            short_gi_80,
            short_gi_160,
            vht_80p80,
            vht_160,
            su_beamformer,
            mu_beamformer,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.be_u16(self.tx_mcs);
        writer.be_u16(self.rx_mcs);
        writer.u8(field(self.max_tx_streams_code, 7, 5)?
            | field(self.max_rx_streams_code, 7, 2)?
            | (u8::from(self.short_gi_80) << 1)
            | u8::from(self.short_gi_160));
        writer.u8((u8::from(self.vht_80p80) << 7)
            | (u8::from(self.vht_160) << 6)
            | (u8::from(self.su_beamformer) << 5)
            | (u8::from(self.mu_beamformer) << 4));
        Ok(writer.into_bytes())
    }
}
lenient_empty!(ApVhtCapabilities);

/// Profile-2 AP Capability, `EasyMesh` §17.2.48 Table 70; audit rows 169–171.
/// Upstream `em_profile_2_ap_cap_t` (`em_base.h:1750-1759`), sender `em.cpp:1772-1797`;
/// receivers `em_capability.cpp:1634-1648,1987-2002`. Four octets match the pinned sender vector.
/// The encoder writes all four octets, with reserved bits zero; upstream's M1 path leaves them
/// unwritten (audit 170).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile2ApCapability {
    /// Maximum prioritization rules.
    pub max_prioritization_rules: u8,
    /// Byte counter units code, including reserved 3.
    pub byte_counter_units: u8,
    /// Service prioritization support.
    pub prioritization: bool,
    /// DPP onboarding support.
    pub dpp_onboarding: bool,
    /// Traffic separation support.
    pub traffic_separation: bool,
    /// Maximum VLAN IDs.
    pub max_vids: u8,
}
impl TlvBody<'_> for Profile2ApCapability {
    const TLV_TYPE: TlvType = TlvType::PROFILE_2_AP_CAPABILITY;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let max_prioritization_rules = reader.u8()?;
        reader.skip(1)?;
        let flags1 = reader.u8()?;
        let byte_counter_units = (flags1 >> 6) & 3;
        let prioritization = bit(flags1, 5);
        let dpp_onboarding = bit(flags1, 4);
        let traffic_separation = bit(flags1, 3);
        let max_vids = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            max_prioritization_rules,
            byte_counter_units,
            prioritization,
            dpp_onboarding,
            traffic_separation,
            max_vids,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.max_prioritization_rules);
        writer.zeros(1);
        writer.u8(field(self.byte_counter_units, 3, 6)?
            | (u8::from(self.prioritization) << 5)
            | (u8::from(self.dpp_onboarding) << 4)
            | (u8::from(self.traffic_separation) << 3));
        writer.u8(self.max_vids);
        Ok(writer.into_bytes())
    }
}

/// AP Radio Advanced Capabilities, `EasyMesh` §17.2.52 Table 74; audit rows 183–185.
/// Upstream `em_ap_radio_advanced_cap_t` (`em_base.h:2218-2228`), sender
/// `em.cpp:1256-1272`, receivers `em_capability.cpp:1651-1670,2038-2060` and stub
/// `em_configuration.cpp:6094-6100`. The port reads one record per TLV; upstream's handlers
/// accept several seven-octet records (audit 185). RUID and flags are written even where
/// upstream's M1 path leaves the value unwritten (audit 184).
/// Unlike its normal sender, which leaves every capability bit zero, the encoder
/// writes each supplied flag (audit 183).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent wire capability bits.
pub struct ApRadioAdvancedCapabilities {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Combined fronthaul and backhaul support.
    pub combined_front_back: bool,
    /// Combined Profile-1 and Profile-2 support.
    pub combined_profile_1_2: bool,
    /// MSCS support.
    pub mscs: bool,
    /// SCS support.
    pub scs: bool,
    /// `QoS` Map support.
    pub qos_map: bool,
    /// DSCP policy support.
    pub dscp_policy: bool,
    /// `QoS` management SCS traffic description support.
    pub qm_scs_traffic_description: bool,
}
impl TlvBody<'_> for ApRadioAdvancedCapabilities {
    const TLV_TYPE: TlvType = TlvType::AP_RADIO_ADVANCED_CAPABILITIES;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let flags1 = reader.u8()?;
        let combined_front_back = bit(flags1, 7);
        let combined_profile_1_2 = bit(flags1, 6);
        let mscs = bit(flags1, 5);
        let scs = bit(flags1, 4);
        let qos_map = bit(flags1, 3);
        let dscp_policy = bit(flags1, 2);
        let qm_scs_traffic_description = bit(flags1, 1);
        reader.finish()?;
        Ok(Self {
            ruid,
            combined_front_back,
            combined_profile_1_2,
            mscs,
            scs,
            qos_map,
            dscp_policy,
            qm_scs_traffic_description,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8((u8::from(self.combined_front_back) << 7)
            | (u8::from(self.combined_profile_1_2) << 6)
            | (u8::from(self.mscs) << 5)
            | (u8::from(self.scs) << 4)
            | (u8::from(self.qos_map) << 3)
            | (u8::from(self.dscp_policy) << 2)
            | (u8::from(self.qm_scs_traffic_description) << 1));
        Ok(writer.into_bytes())
    }
}

/// Controller Capability, `EasyMesh` §17.2.94 Table 116; audit row 273.
/// Upstream `em_ctrl_cap_t` (`em_base.h:2250-2254`), sender
/// `em_configuration.cpp:4455-4462`, receiver `:6351-6356`. Reserved extension octets
/// are ignored and omitted on encoding; bits 5–0 are reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerCapability {
    /// KiB/MiB byte counters supported.
    pub kib_mib_counter: bool,
    /// Early AP Capability Report preferred.
    pub early_ap_capability: bool,
}
impl TlvBody<'_> for ControllerCapability {
    const TLV_TYPE: TlvType = TlvType::CONTROLLER_CAPABILITY;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let flags1 = reader.u8()?;
        let kib_mib_counter = bit(flags1, 7);
        let early_ap_capability = bit(flags1, 6);
        reader.rest();
        reader.finish()?;
        Ok(Self {
            kib_mib_counter,
            early_ap_capability,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer
            .u8((u8::from(self.kib_mib_counter) << 7) | (u8::from(self.early_ap_capability) << 6));
        Ok(writer.into_bytes())
    }
}

/// One HE MCS/NSS pair, `EasyMesh` §17.2.10 Table 32 and §17.2.72 Table 94.
/// Upstream `em_ap_he_mcs_maps_t` (`em_base.h:1906-1909`), senders
/// `em.cpp:1393-1456,1458-1554`, receivers `em_capability.cpp:1582-1597`
/// and `em.cpp:2652-2745`; audit rows 3 and 221, note 0009 A3.
/// Wire order is receive then transmit; upstream writes transmit first (`em.cpp:1429-1432`,
/// `em.cpp:1515-1517` for Wi-Fi 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeMcsSet {
    /// Receive MCS map.
    pub rx: u16,
    /// Transmit MCS map.
    pub tx: u16,
}

/// One, two or three HE MCS/NSS bandwidth sets, in wire order, `EasyMesh`
/// §17.2.10 Table 32 and §17.2.72 Table 94; audit rows 3, 6, 221–222.
/// Upstream `em_ap_he_mcs_maps_t` (`em_base.h:1906-1909`), senders
/// `em.cpp:1393-1456,1458-1554`, receivers `em_capability.cpp:1582-1597`
/// and `em.cpp:2652-2745`; note 0009 A3 specifies Rx before Tx.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeMcsSets {
    /// Only the at-most-80 MHz pair.
    One([HeMcsSet; 1]),
    /// Two bandwidth pairs.
    Two([HeMcsSet; 2]),
    /// Three bandwidth pairs.
    Three([HeMcsSet; 3]),
}
impl HeMcsSets {
    /// Returns the sets in wire order.
    #[must_use]
    pub fn as_slice(&self) -> &[HeMcsSet] {
        match self {
            Self::One(sets) => sets,
            Self::Two(sets) => sets,
            Self::Three(sets) => sets,
        }
    }
    fn read(reader: &mut Reader<'_>, len: u8, little_endian: bool) -> Result<Self, DecodeError> {
        fn one(reader: &mut Reader<'_>, le: bool) -> Result<HeMcsSet, DecodeError> {
            let (rx, tx) = if le {
                (reader.le_u16()?, reader.le_u16()?)
            } else {
                (reader.be_u16()?, reader.be_u16()?)
            };
            Ok(HeMcsSet { rx, tx })
        }
        if !matches!(len, 4 | 8 | 12) {
            return Err(DecodeError::InvalidLength {
                len: usize::from(len),
            });
        }
        let first = one(reader, little_endian)?;
        match len {
            4 => Ok(Self::One([first])),
            8 => Ok(Self::Two([first, one(reader, little_endian)?])),
            _ => Ok(Self::Three([
                first,
                one(reader, little_endian)?,
                one(reader, little_endian)?,
            ])),
        }
    }
    fn write(&self, writer: &mut Writer, little_endian: bool) {
        for set in self.as_slice() {
            if little_endian {
                writer.le_u16(set.rx);
                writer.le_u16(set.tx);
            } else {
                writer.be_u16(set.rx);
                writer.be_u16(set.tx);
            }
        }
    }
    fn octet_len(&self) -> u8 {
        match self {
            Self::One(_) => 4,
            Self::Two(_) => 8,
            Self::Three(_) => 12,
        }
    }
}

/// AP HE Capabilities, `EasyMesh` §17.2.10 Table 32; audit rows 2–6.
/// Upstream `em_ap_he_cap_t` (`em_base.h:1911-1927`), sender `em.cpp:1393-1456`;
/// receivers `em_capability.cpp:1582-1597,1861-1875`. The second octet uses the spec bits,
/// SU beamformer bit 7 through DL OFDMA bit 1. Upstream's sender packs each flag one bit lower,
/// DL OFDMA bit 0 through SU beamformer bit 6 (`em.cpp:1444-1450`), while its struct declares
/// the spec bits (audit 2). MCS maps are Rx then Tx, big-endian, unlike upstream's Tx then Rx in
/// host order (`em.cpp:1429-1432`; 0009 A3). Capability bytes follow the actual MCS length,
/// rather than a fixed three-set receiver offset, and invalid lengths are rejected (audit 4–6).
/// An empty value is lenient (0009 A5): the sender `em.cpp:1403-1413` returns 0 when radio data
/// is missing, and the callers `em_capability.cpp:144-169,359-383` still add the TLV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent wire capability bits.
pub struct ApHeCapabilities {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// HE MCS/NSS bandwidth pairs.
    pub mcs: HeMcsSets,
    /// Transmit spatial streams minus one.
    pub max_tx_streams_code: u8,
    /// Receive spatial streams minus one.
    pub max_rx_streams_code: u8,
    /// 80+80 MHz support.
    pub he_80p80: bool,
    /// 160 MHz support.
    pub he_160: bool,
    /// SU beamformer support.
    pub su_beamformer: bool,
    /// MU beamformer support.
    pub mu_beamformer: bool,
    /// Uplink MU-MIMO support.
    pub ul_mu_mimo: bool,
    /// Uplink MU-MIMO with OFDMA support.
    pub ul_mu_mimo_ofdma: bool,
    /// Downlink MU-MIMO with OFDMA support.
    pub dl_mu_mimo_ofdma: bool,
    /// Uplink OFDMA support.
    pub ul_ofdma: bool,
    /// Downlink OFDMA support.
    pub dl_ofdma: bool,
}
impl TlvBody<'_> for ApHeCapabilities {
    const TLV_TYPE: TlvType = TlvType::AP_HE_CAPABILITIES;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let len = reader.u8()?;
        let mcs = HeMcsSets::read(&mut reader, len, false)?;
        let a = reader.u8()?;
        let b = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            ruid,
            mcs,
            max_tx_streams_code: a >> 5,
            max_rx_streams_code: (a >> 2) & 7,
            he_80p80: bit(a, 1),
            he_160: bit(a, 0),
            su_beamformer: bit(b, 7),
            mu_beamformer: bit(b, 6),
            ul_mu_mimo: bit(b, 5),
            ul_mu_mimo_ofdma: bit(b, 4),
            dl_mu_mimo_ofdma: bit(b, 3),
            ul_ofdma: bit(b, 2),
            dl_ofdma: bit(b, 1),
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.u8(self.mcs.octet_len());
        self.mcs.write(&mut writer, false);
        writer.u8(field(self.max_tx_streams_code, 7, 5)?
            | field(self.max_rx_streams_code, 7, 2)?
            | (u8::from(self.he_80p80) << 1)
            | u8::from(self.he_160));
        writer.u8((u8::from(self.su_beamformer) << 7)
            | (u8::from(self.mu_beamformer) << 6)
            | (u8::from(self.ul_mu_mimo) << 5)
            | (u8::from(self.ul_mu_mimo_ofdma) << 4)
            | (u8::from(self.dl_mu_mimo_ofdma) << 3)
            | (u8::from(self.ul_ofdma) << 2)
            | (u8::from(self.dl_ofdma) << 1));
        Ok(writer.into_bytes())
    }
}
lenient_empty!(ApHeCapabilities);

/// One CAC operating class, `EasyMesh` §17.2.46 Table 68; audit rows 7–11.
/// Upstream `em_cac_op_class_t` (`em_base.h:542-550`), sender `em.cpp:1904-2007`,
/// receiver `em_capability.cpp:1893-1986`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacOperatingClass<'a> {
    /// Operating class code.
    pub operating_class: u8,
    /// Channels supporting this CAC method.
    pub channels: &'a [u8],
}
/// One CAC method, `EasyMesh` §17.2.46 Table 68; audit rows 7–11.
/// Upstream `em_cac_cap_method_t` (`em_base.h:2086-2091`), sender `em.cpp:1904-2007`,
/// receiver `em_capability.cpp:1893-1986`.
/// Duration is a three-octet big-endian value; upstream's early-report receiver reads it
/// little-endian (audit 8).
#[derive(Debug, PartialEq, Eq)]
pub struct CacMethod<'a> {
    /// Method code, including reserved values.
    pub method: u8,
    /// Duration in seconds, at most `0xFF_FFFF`.
    pub duration_seconds: u32,
    /// Operating classes in wire order.
    pub operating_classes: Vec<CacOperatingClass<'a>>,
}
/// One CAC radio, `EasyMesh` §17.2.46 Table 68; audit rows 7–11.
/// Upstream `em_cac_cap_radio_t` (`em_base.h:2093-2097`), sender `em.cpp:1904-2007`,
/// receiver `em_capability.cpp:1893-1986`.
#[derive(Debug, PartialEq, Eq)]
pub struct CacRadio<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Supported methods.
    pub methods: Vec<CacMethod<'a>>,
}
/// CAC Capabilities, `EasyMesh` §17.2.46 Table 68; audit rows 7–11.
/// Upstream `em_cac_cap_t` (`em_base.h:2099-2103`), sender `em.cpp:1904-2007`;
/// receivers `em_capability.cpp:1611-1633,1893-1986`. Packed groups and
/// one-octet counts follow the spec rather than upstream fixed arrays (audit 7, 9–10).
#[derive(Debug, PartialEq, Eq)]
pub struct CacCapabilities<'a> {
    /// Two country-code octets, preserved without UTF-8 validation.
    pub country_code: [u8; 2],
    /// Radios supporting simultaneous CAC.
    pub radios: Vec<CacRadio<'a>>,
}
impl<'a> TlvBody<'a> for CacCapabilities<'a> {
    const TLV_TYPE: TlvType = TlvType::CAC_CAPABILITIES;
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let country_code = reader.array()?;
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 7);
        for _ in 0..count {
            let ruid = reader.mac()?;
            let count = usize::from(reader.u8()?);
            let mut methods = bounded_vec(count, reader.remaining(), 5);
            for _ in 0..count {
                let method = reader.u8()?;
                let duration_seconds = reader.be_u24()?;
                let count = usize::from(reader.u8()?);
                let mut operating_classes = bounded_vec(count, reader.remaining(), 2);
                for _ in 0..count {
                    let operating_class = reader.u8()?;
                    let count = usize::from(reader.u8()?);
                    let channels = reader.bytes(count)?;
                    operating_classes.push(CacOperatingClass {
                        operating_class,
                        channels,
                    });
                }
                methods.push(CacMethod {
                    method,
                    duration_seconds,
                    operating_classes,
                });
            }
            radios.push(CacRadio { ruid, methods });
        }
        reader.finish()?;
        Ok(Self {
            country_code,
            radios,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.bytes(&self.country_code);
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.count_u8(radio.methods.len())?;
            for method in &radio.methods {
                writer.u8(method.method);
                writer.be_u24(method.duration_seconds)?;
                writer.count_u8(method.operating_classes.len())?;
                for class in &method.operating_classes {
                    writer.u8(class.operating_class);
                    writer.count_u8(class.channels.len())?;
                    writer.bytes(class.channels);
                }
            }
        }
        Ok(writer.into_bytes())
    }
}

/// One AP Wi-Fi 6 role, `EasyMesh` §17.2.72 Table 94; audit rows 219–223.
/// Upstream `em_wifi6_role_wire_t` (`em_base.h:1959-1963`) with its octet structs
/// `em_wifi6_cap_role_head_tlv_t` (`:1929-1934`) and `em_wifi6_cap_role_tail_tlv_t`
/// (`:1936-1957`), sender `em.cpp:1458-1554`, receiver `em.cpp:2652-2745`. MCS sets are Rx then
/// Tx, little-endian (0009 A3), unlike upstream's Tx then Rx (`em.cpp:1515-1517`). Length is an
/// octet count (audit 222).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent wire capability bits.
pub struct Wifi6Role {
    /// Agent role code, including reserved values 2–3.
    pub role: u8,
    /// HE 160 MHz support.
    pub he_160: bool,
    /// HE 80+80 MHz support.
    pub he_80p80: bool,
    /// MCS/NSS pairs, with count independent of the support flags.
    pub mcs: HeMcsSets,
    /// SU beamformer support.
    pub su_beamformer: bool,
    /// SU beamformee support.
    pub su_beamformee: bool,
    /// MU beamformer support.
    pub mu_beamformer: bool,
    /// Beamformee STS at most 80 MHz.
    pub beamformee_sts_le_80: bool,
    /// Beamformee STS above 80 MHz.
    pub beamformee_sts_gt_80: bool,
    /// Uplink MU-MIMO support.
    pub ul_mu_mimo: bool,
    /// Uplink OFDMA support.
    pub ul_ofdma: bool,
    /// Downlink OFDMA support.
    pub dl_ofdma: bool,
    /// Maximum downlink MU-MIMO users.
    pub max_dl_mu_mimo_tx: u8,
    /// Maximum uplink MU-MIMO users.
    pub max_ul_mu_mimo_rx: u8,
    /// Maximum downlink OFDMA users.
    pub max_dl_ofdma_tx: u8,
    /// Maximum uplink OFDMA users.
    pub max_ul_ofdma_rx: u8,
    /// RTS support.
    pub rts: bool,
    /// MU RTS support.
    pub mu_rts: bool,
    /// Multi-BSSID support.
    pub multi_bssid: bool,
    /// MU EDCA support.
    pub mu_edca: bool,
    /// TWT requester support.
    pub twt_requester: bool,
    /// TWT responder support.
    pub twt_responder: bool,
    /// Spatial reuse support.
    pub spatial_reuse: bool,
    /// Anticipated channel usage reporting.
    pub anticipated_channel_usage: bool,
}
impl Wifi6Role {
    fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let head = reader.u8()?;
        let mcs = HeMcsSets::read(reader, head & 15, true)?;
        let first = reader.u8()?;
        let users = reader.u8()?;
        let max_dl_ofdma_tx = reader.u8()?;
        let max_ul_ofdma_rx = reader.u8()?;
        let last = reader.u8()?;
        Ok(Self {
            role: head >> 6,
            he_160: bit(head, 5),
            he_80p80: bit(head, 4),
            mcs,
            su_beamformer: bit(first, 7),
            su_beamformee: bit(first, 6),
            mu_beamformer: bit(first, 5),
            beamformee_sts_le_80: bit(first, 4),
            beamformee_sts_gt_80: bit(first, 3),
            ul_mu_mimo: bit(first, 2),
            ul_ofdma: bit(first, 1),
            dl_ofdma: bit(first, 0),
            max_dl_mu_mimo_tx: users >> 4,
            max_ul_mu_mimo_rx: users & 15,
            max_dl_ofdma_tx,
            max_ul_ofdma_rx,
            rts: bit(last, 7),
            mu_rts: bit(last, 6),
            multi_bssid: bit(last, 5),
            mu_edca: bit(last, 4),
            twt_requester: bit(last, 3),
            twt_responder: bit(last, 2),
            spatial_reuse: bit(last, 1),
            anticipated_channel_usage: bit(last, 0),
        })
    }
    fn write(&self, writer: &mut Writer) -> Result<(), EncodeError> {
        writer.u8(field(self.role, 3, 6)?
            | (u8::from(self.he_160) << 5)
            | (u8::from(self.he_80p80) << 4)
            | self.mcs.octet_len());
        self.mcs.write(writer, true);
        writer.u8((u8::from(self.su_beamformer) << 7)
            | (u8::from(self.su_beamformee) << 6)
            | (u8::from(self.mu_beamformer) << 5)
            | (u8::from(self.beamformee_sts_le_80) << 4)
            | (u8::from(self.beamformee_sts_gt_80) << 3)
            | (u8::from(self.ul_mu_mimo) << 2)
            | (u8::from(self.ul_ofdma) << 1)
            | u8::from(self.dl_ofdma));
        writer.u8(field(self.max_dl_mu_mimo_tx, 15, 4)? | field(self.max_ul_mu_mimo_rx, 15, 0)?);
        writer.u8(self.max_dl_ofdma_tx);
        writer.u8(self.max_ul_ofdma_rx);
        writer.u8((u8::from(self.rts) << 7)
            | (u8::from(self.mu_rts) << 6)
            | (u8::from(self.multi_bssid) << 5)
            | (u8::from(self.mu_edca) << 4)
            | (u8::from(self.twt_requester) << 3)
            | (u8::from(self.twt_responder) << 2)
            | (u8::from(self.spatial_reuse) << 1)
            | u8::from(self.anticipated_channel_usage));
        Ok(())
    }
}
/// AP Wi-Fi 6 Capabilities, `EasyMesh` §17.2.72 Table 94; audit rows 219–223.
/// Upstream `em_radio_wifi6_cap_data_t` (`em_base.h:1965-1968`), sender
/// `em.cpp:1458-1554`, receiver `em.cpp:2652-2745`. Rx-then-Tx maps differ from
/// upstream's order (0009 A3); role counts are not capped at upstream's two-element
/// array (audit 220). An empty value is lenient (0009 A5): the sender `em.cpp:1466-1470` returns
/// 0 when radio data is missing, and the callers `em_capability.cpp:144-169,359-383` still add
/// the TLV. Flag and MCS length mismatches are carried unchanged.
#[derive(Debug, PartialEq, Eq)]
pub struct ApWifi6Capabilities {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Roles in wire order, including reserved role codes.
    pub roles: Vec<Wifi6Role>,
}
impl TlvBody<'_> for ApWifi6Capabilities {
    const TLV_TYPE: TlvType = TlvType::AP_WIFI_6_CAPABILITIES;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let count = usize::from(reader.u8()?);
        let mut roles = bounded_vec(count, reader.remaining(), 10);
        for _ in 0..count {
            roles.push(Wifi6Role::read(&mut reader)?);
        }
        reader.finish()?;
        Ok(Self { ruid, roles })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.count_u8(self.roles.len())?;
        for role in &self.roles {
            role.write(&mut writer)?;
        }
        Ok(writer.into_bytes())
    }
}
lenient_empty!(ApWifi6Capabilities);

/// A radio in Device Inventory, `EasyMesh` §17.2.76 Table 98; audit rows 234–236.
/// Upstream `em_radio_vendor_t` (`em_base.h:1871-1875`), sender `em.cpp:1799-1877`;
/// receiver `em_capability.cpp:2003-2036` stops before the radio list (audit 236).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryRadio<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// Chipset vendor as raw octets, at most 64 bytes.
    pub chipset_vendor: &'a [u8],
}
/// Device Inventory, `EasyMesh` §17.2.76 Table 98; audit rows 234–236.
/// Upstream `em_device_inventory_t` (`em_base.h:1877-1879`), sender `em.cpp:1799-1877`;
/// receiver `em_capability.cpp:2003-2036`. Strings borrow their raw bytes and
/// are capped at 64 octets. Unlike upstream, all radio records are decoded and are not
/// capped at three (audit 235–236). The strict constructor and decoder require a radio;
/// an upstream zero-radio value (sender `em.cpp:1851-1854`) decodes leniently, but encoding it
/// fails with `EncodeError::CountTooSmall { len: 0, min: 1 }` (0009 A5).
#[derive(Debug, PartialEq, Eq)]
pub struct DeviceInventory<'a> {
    /// Serial number as raw octets.
    pub serial_number: &'a [u8],
    /// Software version as raw octets.
    pub software_version: &'a [u8],
    /// Execution environment as raw octets.
    pub execution_environment: &'a [u8],
    radios: Vec<InventoryRadio<'a>>,
}
impl<'a> DeviceInventory<'a> {
    /// Creates an inventory if at least one radio is present.
    #[must_use]
    pub fn new(
        serial_number: &'a [u8],
        software_version: &'a [u8],
        execution_environment: &'a [u8],
        radios: Vec<InventoryRadio<'a>>,
    ) -> Option<Self> {
        if radios.is_empty() {
            None
        } else {
            Some(Self {
                serial_number,
                software_version,
                execution_environment,
                radios,
            })
        }
    }
    /// Returns the radios; a leniently decoded upstream value may have none.
    #[must_use]
    pub fn radios(&self) -> &[InventoryRadio<'a>] {
        &self.radios
    }
    /// Mutates existing radios without changing their count.
    pub fn radios_mut(&mut self) -> &mut [InventoryRadio<'a>] {
        &mut self.radios
    }
    /// Appends a radio.
    pub fn push_radio(&mut self, radio: InventoryRadio<'a>) {
        self.radios.push(radio);
    }
    fn read(value: &'a [u8], lenient: bool) -> Result<Self, DecodeError> {
        fn string<'a>(reader: &mut Reader<'a>) -> Result<&'a [u8], DecodeError> {
            let len = usize::from(reader.u8()?);
            if len > 64 {
                return Err(DecodeError::InvalidLength { len });
            }
            reader.bytes(len)
        }
        let mut reader = Reader::new(value);
        let serial_number = string(&mut reader)?;
        let software_version = string(&mut reader)?;
        let execution_environment = string(&mut reader)?;
        let count = usize::from(reader.u8()?);
        if count == 0 && !lenient {
            return Err(DecodeError::InvalidLength { len: value.len() });
        }
        let mut radios = bounded_vec(count, reader.remaining(), 7);
        for _ in 0..count {
            radios.push(InventoryRadio {
                ruid: reader.mac()?,
                chipset_vendor: string(&mut reader)?,
            });
        }
        reader.finish()?;
        Ok(Self {
            serial_number,
            software_version,
            execution_environment,
            radios,
        })
    }
}
impl<'a> TlvBody<'a> for DeviceInventory<'a> {
    const TLV_TYPE: TlvType = TlvType::DEVICE_INVENTORY;
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        Self::read(value, false)
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        fn string(writer: &mut Writer, bytes: &[u8]) -> Result<(), EncodeError> {
            if bytes.len() > 64 {
                return Err(EncodeError::CountTooLarge {
                    len: bytes.len(),
                    max: 64,
                });
            }
            writer.count_u8(bytes.len())?;
            writer.bytes(bytes);
            Ok(())
        }
        let mut writer = Writer::new();
        string(&mut writer, self.serial_number)?;
        string(&mut writer, self.software_version)?;
        string(&mut writer, self.execution_environment)?;
        // A lenient decode can give no radios; the strict decoder rejects that, so do not write it.
        if self.radios.is_empty() {
            return Err(EncodeError::CountTooSmall { len: 0, min: 1 });
        }
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            string(&mut writer, radio.chipset_vendor)?;
        }
        Ok(writer.into_bytes())
    }
}
impl<'a> TlvBodyLenient<'a> for DeviceInventory<'a> {
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        Self::read(value, true).map(Some)
    }
}

/// A Wi-Fi 7 frequency-separation record, `EasyMesh` §17.2.95 Table 117;
/// audit rows 274–277. Upstream `em_wifi7_freq_record_tlv_t` (`em_base.h:1970-1974`),
/// sender `em.cpp:1556-1701`, receiver `:2747-2895`. Counts are not capped at
/// the upstream limit of eight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wifi7FrequencyRecord {
    /// Other radio unique identifier.
    pub ruid: MacAddr,
    /// Frequency separation code in the range 0–31.
    pub frequency_separation: u8,
}
/// Wi-Fi 7 mode support and corresponding records for one role, `EasyMesh`
/// §17.2.95 Table 117; audit rows 274–277. Upstream
/// `em_wifi7_mlo_cap_records_t` (`em_base.h:2005-2014`), sender `em.cpp:1556-1701`,
/// receiver `:2747-2895`.
#[derive(Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent wire capability bits.
pub struct Wifi7ModeCapabilities {
    /// STR support.
    pub str_supported: bool,
    /// NSTR support.
    pub nstr_supported: bool,
    /// EMLSR support.
    pub emlsr_supported: bool,
    /// EMLMR support.
    pub emlmr_supported: bool,
    /// STR frequency records.
    pub str_records: Vec<Wifi7FrequencyRecord>,
    /// NSTR frequency records.
    pub nstr_records: Vec<Wifi7FrequencyRecord>,
    /// EMLSR frequency records.
    pub emlsr_records: Vec<Wifi7FrequencyRecord>,
    /// EMLMR frequency records.
    pub emlmr_records: Vec<Wifi7FrequencyRecord>,
}
impl Wifi7ModeCapabilities {
    fn flags(&self) -> u8 {
        (u8::from(self.str_supported) << 7)
            | (u8::from(self.nstr_supported) << 6)
            | (u8::from(self.emlsr_supported) << 5)
            | (u8::from(self.emlmr_supported) << 4)
    }
    fn read_records(reader: &mut Reader<'_>, flags: u8) -> Result<Self, DecodeError> {
        fn group(reader: &mut Reader<'_>) -> Result<Vec<Wifi7FrequencyRecord>, DecodeError> {
            let count = usize::from(reader.u8()?);
            let mut records = bounded_vec(count, reader.remaining(), 7);
            for _ in 0..count {
                records.push(Wifi7FrequencyRecord {
                    ruid: reader.mac()?,
                    frequency_separation: reader.u8()? >> 3,
                });
            }
            Ok(records)
        }
        Ok(Self {
            str_supported: bit(flags, 7),
            nstr_supported: bit(flags, 6),
            emlsr_supported: bit(flags, 5),
            emlmr_supported: bit(flags, 4),
            str_records: group(reader)?,
            nstr_records: group(reader)?,
            emlsr_records: group(reader)?,
            emlmr_records: group(reader)?,
        })
    }
    fn write_records(&self, writer: &mut Writer) -> Result<(), EncodeError> {
        for records in [
            &self.str_records,
            &self.nstr_records,
            &self.emlsr_records,
            &self.emlmr_records,
        ] {
            writer.count_u8(records.len())?;
            for record in records {
                writer.mac(record.ruid);
                writer.u8(field(record.frequency_separation, 31, 3)?);
            }
        }
        Ok(())
    }
}
/// One radio in Wi-Fi 7 Agent Capabilities, `EasyMesh` §17.2.95 Table 117;
/// audit rows 274–277. Upstream `em_wifi7_agent_cap_t` (`em_base.h:2016-2019`),
/// sender `em.cpp:1556-1701`, receiver `:2747-2895`. Reserved bytes and bits are
/// ignored and written zero.
#[derive(Debug, PartialEq, Eq)]
pub struct Wifi7Radio {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// AP mode support and records.
    pub ap: Wifi7ModeCapabilities,
    /// Backhaul STA mode support and records.
    pub backhaul_sta: Wifi7ModeCapabilities,
}
/// Wi-Fi 7 Agent Capabilities, `EasyMesh` §17.2.95 Table 117; audit rows 274–277.
/// Upstream `em_wifi7_cap_tlv_t` (`em_base.h:2021-2025`), sender `em.cpp:1556-1701`;
/// receiver `em.cpp:2747-2895`. The spec's packed radio counts and
/// record counts replace upstream's fixed-size arrays (audit 274–276). Link code 15
/// remains raw instead of being mapped to zero links as in upstream (audit 277).
#[derive(Debug, PartialEq, Eq)]
pub struct Wifi7AgentCapabilities {
    /// Maximum number of MLDs.
    pub max_mlds: u8,
    /// AP maximum-links wire code, including reserved 15.
    pub ap_maximum_links: u8,
    /// Backhaul STA maximum-links wire code, including reserved 15.
    pub backhaul_sta_maximum_links: u8,
    /// TID-to-link mapping code, including reserved 2.
    pub tid_to_link_mapping: u8,
    /// Radio records in wire order.
    pub radios: Vec<Wifi7Radio>,
}
impl TlvBody<'_> for Wifi7AgentCapabilities {
    const TLV_TYPE: TlvType = TlvType::WIFI_7_AGENT_CAPABILITIES;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let max_mlds = reader.u8()?;
        let links = reader.u8()?;
        let mapping = reader.u8()? >> 6;
        reader.skip(13)?;
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 40);
        for _ in 0..count {
            let ruid = reader.mac()?;
            reader.skip(24)?;
            let ap_flags = reader.u8()?;
            let backhaul_flags = reader.u8()?;
            let ap = Wifi7ModeCapabilities::read_records(&mut reader, ap_flags)?;
            let backhaul_sta = Wifi7ModeCapabilities::read_records(&mut reader, backhaul_flags)?;
            radios.push(Wifi7Radio {
                ruid,
                ap,
                backhaul_sta,
            });
        }
        reader.finish()?;
        Ok(Self {
            max_mlds,
            ap_maximum_links: links >> 4,
            backhaul_sta_maximum_links: links & 15,
            tid_to_link_mapping: mapping,
            radios,
        })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.max_mlds);
        writer
            .u8(field(self.ap_maximum_links, 15, 4)?
                | field(self.backhaul_sta_maximum_links, 15, 0)?);
        writer.u8(field(self.tid_to_link_mapping, 3, 6)?);
        writer.zeros(13);
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.zeros(24);
            writer.u8(radio.ap.flags());
            writer.u8(radio.backhaul_sta.flags());
            radio.ap.write_records(&mut writer)?;
            radio.backhaul_sta.write_records(&mut writer)?;
        }
        Ok(writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{sweep_row, sweep_row_lenient};
    use core::mem::size_of;

    const GROUP: &str = "capability";
    const RUID: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]);
    const RUID2: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf1]);
    const STATION: MacAddr = MacAddr::new([0x02, 0x66, 0x77, 0x88, 0x99, 0xaa]);
    const LENIENT: &[Key] = &[
        ("0x86", "empty"),
        ("0x87", "empty"),
        ("0x88", "empty"),
        ("0xaa", "empty"),
        ("0xd4", "zero-radios"),
    ];
    const CANONICAL: &[(Key, Key)] = &[
        (
            ("0xcb", "reserved-bits-set-not-included"),
            ("0xcb", "mac-not-included"),
        ),
        (("0xdd", "reserved-ext-octets"), ("0xdd", "both-set")),
        (
            ("0xdd", "reserved-ext-zero-octets"),
            ("0xdd", "upstream-early-ap"),
        ),
        (
            ("0xdf", "reserved-bits-set"),
            ("0xdf", "reserved-bits-clear"),
        ),
    ];
    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }
    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0x85" => sweep_row::<ApRadioBasicCapabilities<'_>>(row, bytes, canonical),
            "0x86" => sweep_row_lenient::<ApHtCapabilities>(row, bytes, canonical, LENIENT),
            "0x87" => sweep_row_lenient::<ApVhtCapabilities>(row, bytes, canonical, LENIENT),
            "0x88" => sweep_row_lenient::<ApHeCapabilities>(row, bytes, canonical, LENIENT),
            "0x90" => sweep_row::<ClientInfo>(row, bytes, canonical),
            "0x91" => sweep_row::<ClientCapabilityReport<'_>>(row, bytes, canonical),
            "0xa1" => sweep_row::<ApCapability>(row, bytes, canonical),
            "0xaa" => sweep_row_lenient::<ApWifi6Capabilities>(row, bytes, canonical, LENIENT),
            "0xb2" => sweep_row::<CacCapabilities<'_>>(row, bytes, canonical),
            "0xb4" => sweep_row::<Profile2ApCapability>(row, bytes, canonical),
            "0xbe" => sweep_row::<ApRadioAdvancedCapabilities>(row, bytes, canonical),
            "0xcb" => {
                sweep_row_lenient::<BackhaulStaRadioCapabilities>(row, bytes, canonical, LENIENT)
            }
            "0xd4" => sweep_row_lenient::<DeviceInventory<'_>>(row, bytes, canonical, LENIENT),
            "0xdd" => sweep_row::<ControllerCapability>(row, bytes, canonical),
            "0xdf" => sweep_row::<Wifi7AgentCapabilities>(row, bytes, canonical),
            other => Err(format!("no body for TLV {other}")),
        }
    }
    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, CANONICAL, dispatch_row);
    }
    fn backhaul() -> BackhaulStaRadioCapabilities {
        BackhaulStaRadioCapabilities {
            ruid: RUID,
            backhaul_sta: Some(STATION),
        }
    }
    fn basic() -> ApRadioBasicCapabilities<'static> {
        ApRadioBasicCapabilities {
            ruid: RUID,
            max_bss: 16,
            operating_classes: vec![
                BasicOperatingClass {
                    operating_class: 115,
                    max_tx_power: 23,
                    non_operable_channels: &[48],
                },
                BasicOperatingClass {
                    operating_class: 121,
                    max_tx_power: -3,
                    non_operable_channels: &[120, 124, 128],
                },
            ],
        }
    }
    fn ht() -> ApHtCapabilities {
        ApHtCapabilities {
            ruid: RUID,
            max_tx_streams_code: 1,
            max_rx_streams_code: 2,
            short_gi_20: true,
            short_gi_40: false,
            ht_40: true,
        }
    }
    fn vht() -> ApVhtCapabilities {
        ApVhtCapabilities {
            ruid: RUID,
            tx_mcs: 0xfffa,
            rx_mcs: 0xfff5,
            max_tx_streams_code: 3,
            max_rx_streams_code: 1,
            short_gi_80: true,
            short_gi_160: false,
            vht_80p80: false,
            vht_160: true,
            su_beamformer: true,
            mu_beamformer: false,
        }
    }
    fn he() -> ApHeCapabilities {
        ApHeCapabilities {
            ruid: RUID,
            mcs: HeMcsSets::Two([
                HeMcsSet {
                    rx: 0xfffa,
                    tx: 0xfff5,
                },
                HeMcsSet {
                    rx: 0xfffc,
                    tx: 0xfff3,
                },
            ]),
            max_tx_streams_code: 3,
            max_rx_streams_code: 1,
            he_80p80: false,
            he_160: true,
            su_beamformer: true,
            mu_beamformer: false,
            ul_mu_mimo: true,
            ul_mu_mimo_ofdma: false,
            dl_mu_mimo_ofdma: false,
            ul_ofdma: true,
            dl_ofdma: true,
        }
    }
    fn ap() -> ApCapability {
        ApCapability {
            on_channel_metrics: true,
            off_channel_metrics: false,
            rcpi_steering: true,
            m8_backhaul_reconfiguration: false,
        }
    }
    fn client() -> ClientInfo {
        ClientInfo {
            bssid: MacAddr::new([2, 0x11, 0x22, 0x33, 0x44, 0x55]),
            station: STATION,
        }
    }
    fn report() -> ClientCapabilityReport<'static> {
        ClientCapabilityReport::new(
            0,
            Some(&[
                0x31, 0x04, 0x0a, 0x00, 0x00, 0x04, b't', b'e', b's', b't', 0x01, 0x04, 0x02, 0x04,
                0x0b, 0x16,
            ]),
        )
        .unwrap()
    }
    fn cac() -> CacCapabilities<'static> {
        CacCapabilities {
            country_code: *b"US",
            radios: vec![
                CacRadio {
                    ruid: RUID,
                    methods: vec![
                        CacMethod {
                            method: 0,
                            duration_seconds: 123_456,
                            operating_classes: vec![
                                CacOperatingClass {
                                    operating_class: 121,
                                    channels: &[100, 104],
                                },
                                CacOperatingClass {
                                    operating_class: 122,
                                    channels: &[106],
                                },
                            ],
                        },
                        CacMethod {
                            method: 1,
                            duration_seconds: 600,
                            operating_classes: vec![CacOperatingClass {
                                operating_class: 124,
                                channels: &[149],
                            }],
                        },
                    ],
                },
                CacRadio {
                    ruid: RUID2,
                    methods: vec![CacMethod {
                        method: 3,
                        duration_seconds: 70_000,
                        operating_classes: vec![CacOperatingClass {
                            operating_class: 128,
                            channels: &[42, 58, 106],
                        }],
                    }],
                },
            ],
        }
    }
    fn profile2() -> Profile2ApCapability {
        Profile2ApCapability {
            max_prioritization_rules: 8,
            byte_counter_units: 1,
            prioritization: true,
            dpp_onboarding: false,
            traffic_separation: true,
            max_vids: 48,
        }
    }
    fn advanced() -> ApRadioAdvancedCapabilities {
        ApRadioAdvancedCapabilities {
            ruid: RUID,
            combined_front_back: true,
            combined_profile_1_2: false,
            mscs: true,
            scs: false,
            qos_map: true,
            dscp_policy: true,
            qm_scs_traffic_description: false,
        }
    }
    fn inventory() -> DeviceInventory<'static> {
        DeviceInventory::new(
            b"SN-0042",
            b"2.1.7",
            b"Linux",
            vec![
                InventoryRadio {
                    ruid: RUID,
                    chipset_vendor: b"Broadcom",
                },
                InventoryRadio {
                    ruid: RUID2,
                    chipset_vendor: b"MTK",
                },
            ],
        )
        .unwrap()
    }
    fn inventory_empty() -> DeviceInventory<'static> {
        DeviceInventory {
            serial_number: b"SN-0042",
            software_version: b"2.1.7",
            execution_environment: b"Linux",
            radios: vec![],
        }
    }
    fn controller() -> ControllerCapability {
        ControllerCapability {
            kib_mib_counter: true,
            early_ap_capability: false,
        }
    }
    fn wifi6() -> ApWifi6Capabilities {
        let one = HeMcsSet {
            rx: 0xfffa,
            tx: 0xfff5,
        };
        ApWifi6Capabilities {
            ruid: RUID,
            roles: vec![
                Wifi6Role {
                    role: 0,
                    he_160: true,
                    he_80p80: true,
                    mcs: HeMcsSets::Three([
                        one,
                        HeMcsSet {
                            rx: 0xfffc,
                            tx: 0xfff3,
                        },
                        HeMcsSet {
                            rx: 0xfffe,
                            tx: 0xfff9,
                        },
                    ]),
                    su_beamformer: true,
                    su_beamformee: false,
                    mu_beamformer: true,
                    beamformee_sts_le_80: true,
                    beamformee_sts_gt_80: false,
                    ul_mu_mimo: true,
                    ul_ofdma: true,
                    dl_ofdma: true,
                    max_dl_mu_mimo_tx: 8,
                    max_ul_mu_mimo_rx: 4,
                    max_dl_ofdma_tx: 37,
                    max_ul_ofdma_rx: 18,
                    rts: true,
                    mu_rts: false,
                    multi_bssid: true,
                    mu_edca: true,
                    twt_requester: false,
                    twt_responder: true,
                    spatial_reuse: true,
                    anticipated_channel_usage: false,
                },
                Wifi6Role {
                    role: 1,
                    he_160: false,
                    he_80p80: false,
                    mcs: HeMcsSets::One([one]),
                    su_beamformer: false,
                    su_beamformee: true,
                    mu_beamformer: false,
                    beamformee_sts_le_80: false,
                    beamformee_sts_gt_80: true,
                    ul_mu_mimo: false,
                    ul_ofdma: true,
                    dl_ofdma: false,
                    max_dl_mu_mimo_tx: 0,
                    max_ul_mu_mimo_rx: 0,
                    max_dl_ofdma_tx: 0,
                    max_ul_ofdma_rx: 0,
                    rts: true,
                    mu_rts: true,
                    multi_bssid: false,
                    mu_edca: false,
                    twt_requester: true,
                    twt_responder: false,
                    spatial_reuse: false,
                    anticipated_channel_usage: true,
                },
            ],
        }
    }
    fn record(last: u8, frequency_separation: u8) -> Wifi7FrequencyRecord {
        Wifi7FrequencyRecord {
            ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, last]),
            frequency_separation,
        }
    }
    fn wifi7() -> Wifi7AgentCapabilities {
        Wifi7AgentCapabilities {
            max_mlds: 3,
            ap_maximum_links: 2,
            backhaul_sta_maximum_links: 1,
            tid_to_link_mapping: 1,
            radios: vec![
                Wifi7Radio {
                    ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]),
                    ap: Wifi7ModeCapabilities {
                        str_supported: true,
                        str_records: vec![record(0xf1, 2)],
                        nstr_supported: false,
                        nstr_records: vec![record(0x02, 5), record(0x13, 8)],
                        emlsr_supported: true,
                        emlsr_records: vec![record(0x13, 11), record(0x24, 14), record(0x35, 17)],
                        emlmr_supported: false,
                        emlmr_records: vec![
                            record(0x24, 20),
                            record(0x35, 23),
                            record(0x46, 26),
                            record(0x57, 29),
                        ],
                    },
                    backhaul_sta: Wifi7ModeCapabilities {
                        str_supported: false,
                        str_records: vec![
                            record(0x35, 1),
                            record(0x46, 4),
                            record(0x57, 7),
                            record(0x68, 10),
                            record(0x79, 13),
                        ],
                        nstr_supported: true,
                        nstr_records: vec![
                            record(0x46, 16),
                            record(0x57, 19),
                            record(0x68, 22),
                            record(0x79, 25),
                            record(0xf1, 28),
                            record(0x02, 31),
                        ],
                        emlsr_supported: false,
                        emlsr_records: vec![
                            record(0x57, 3),
                            record(0x68, 6),
                            record(0x79, 9),
                            record(0xf1, 12),
                            record(0x02, 15),
                            record(0x13, 18),
                            record(0x24, 21),
                        ],
                        emlmr_supported: true,
                        emlmr_records: vec![
                            record(0x68, 24),
                            record(0x79, 27),
                            record(0xf1, 30),
                            record(0x02, 2),
                            record(0x13, 5),
                            record(0x24, 8),
                            record(0x35, 11),
                            record(0x46, 14),
                        ],
                    },
                },
                Wifi7Radio {
                    ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf1]),
                    ap: Wifi7ModeCapabilities {
                        str_supported: false,
                        str_records: vec![record(0xf1, 2)],
                        nstr_supported: true,
                        nstr_records: vec![record(0x02, 5)],
                        emlsr_supported: false,
                        emlsr_records: vec![record(0x13, 8)],
                        emlmr_supported: true,
                        emlmr_records: vec![record(0x24, 11)],
                    },
                    backhaul_sta: Wifi7ModeCapabilities {
                        str_supported: true,
                        str_records: vec![record(0x35, 14)],
                        nstr_supported: false,
                        nstr_records: vec![record(0x46, 17)],
                        emlsr_supported: true,
                        emlsr_records: vec![record(0x57, 20)],
                        emlmr_supported: false,
                        emlmr_records: vec![record(0x68, 23)],
                    },
                },
            ],
        }
    }
    const TOO_MANY: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    fn many_basic() -> ApRadioBasicCapabilities<'static> {
        ApRadioBasicCapabilities {
            ruid: RUID,
            max_bss: 0,
            operating_classes: vec![
                BasicOperatingClass {
                    operating_class: 0,
                    max_tx_power: 0,
                    non_operable_channels: &[]
                };
                256
            ],
        }
    }
    fn many_wifi6() -> ApWifi6Capabilities {
        let mut body = wifi6();
        body.roles = vec![body.roles[0]; 256];
        body
    }
    fn many_cac() -> CacCapabilities<'static> {
        CacCapabilities {
            country_code: *b"US",
            radios: (0..256)
                .map(|_| CacRadio {
                    ruid: RUID,
                    methods: vec![],
                })
                .collect(),
        }
    }
    fn many_inventory() -> DeviceInventory<'static> {
        DeviceInventory::new(
            b"",
            b"",
            b"",
            vec![
                InventoryRadio {
                    ruid: RUID,
                    chipset_vendor: b""
                };
                256
            ],
        )
        .unwrap()
    }
    fn many_wifi7() -> Wifi7AgentCapabilities {
        Wifi7AgentCapabilities {
            max_mlds: 0,
            ap_maximum_links: 0,
            backhaul_sta_maximum_links: 0,
            tid_to_link_mapping: 0,
            radios: (0..256)
                .map(|_| Wifi7Radio {
                    ruid: RUID,
                    ap: Wifi7ModeCapabilities::default(),
                    backhaul_sta: Wifi7ModeCapabilities::default(),
                })
                .collect(),
        }
    }
    use crate::body_testkit::{Field, truncated_in};
    body_checks! {
        mod basic_checks;
        body: ApRadioBasicCapabilities<'a>;
        golden: &fixture(("0x85", "golden")), basic();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 1 }, Field { offset: 7, width: 1 }, Field { offset: 8, width: 1 }, Field { offset: 9, width: 1 }, Field { offset: 10, width: 1 }, Field { offset: 11, width: 1 }, Field { offset: 12, width: 1 }, Field { offset: 13, width: 1 }, Field { offset: 14, width: 1 }, Field { offset: 15, width: 3 }]);
        trailing_byte: &fixture(("0x85", "golden"));
        count_too_large: many_basic(), TOO_MANY;
    }
    body_checks! {
        mod ht_checks;
        body: ApHtCapabilities;
        golden: &fixture(("0x86", "golden")), ht();
        lenient;
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 1 }]);
        trailing_byte: &fixture(("0x86", "golden"));
        reserved_bits: [(&fixture(("0x86", "reserved-bits-set")), ht(), &fixture(("0x86", "golden")))];
        lenient_absent: [&fixture(("0x86", "empty"))];
    }
    body_checks! {
        mod vht_checks;
        body: ApVhtCapabilities;
        golden: &fixture(("0x87", "golden")), vht();
        lenient;
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 2 }, Field { offset: 8, width: 2 }, Field { offset: 10, width: 1 }, Field { offset: 11, width: 1 }]);
        trailing_byte: &fixture(("0x87", "golden"));
        reserved_bits: [(&fixture(("0x87", "reserved-bits-set")), vht(), &fixture(("0x87", "golden")))];
        lenient_absent: [&fixture(("0x87", "empty"))];
    }
    body_checks! {
        mod he_checks;
        body: ApHeCapabilities;
        golden: &fixture(("0x88", "golden")), he();
        lenient;
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 1 }, Field { offset: 7, width: 2 }, Field { offset: 9, width: 2 }, Field { offset: 11, width: 2 }, Field { offset: 13, width: 2 }, Field { offset: 15, width: 1 }, Field { offset: 16, width: 1 }]);
        trailing_byte: &fixture(("0x88", "golden"));
        reserved_bits: [(&fixture(("0x88", "reserved-bits-set")), he(), &fixture(("0x88", "golden")))];
        lenient_absent: [&fixture(("0x88", "empty"))];
    }
    body_checks! {
        mod client_checks;
        body: ClientInfo;
        golden: &fixture(("0x90", "golden")), client();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 6 }]);
        trailing_byte: &fixture(("0x90", "golden"));
    }
    body_checks! {
        mod report_checks;
        body: ClientCapabilityReport<'a>;
        golden: &fixture(("0x91", "golden")), report();
        prefixes: |len| len >= 1, truncated_in(&[Field { offset: 0, width: 1 }]);
    }
    body_checks! {
        mod ap_checks;
        body: ApCapability;
        golden: &fixture(("0xa1", "golden")), ap();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 1 }]);
        trailing_byte: &fixture(("0xa1", "golden"));
        reserved_bits: [(&fixture(("0xa1", "reserved-bits-set")), ap(), &fixture(("0xa1", "golden")))];
    }
    body_checks! {
        mod wifi6_checks;
        body: ApWifi6Capabilities;
        golden: &fixture(("0xaa", "golden")), wifi6();
        lenient;
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 1 }, Field { offset: 7, width: 1 }, Field { offset: 8, width: 2 }, Field { offset: 10, width: 2 }, Field { offset: 12, width: 2 }, Field { offset: 14, width: 2 }, Field { offset: 16, width: 2 }, Field { offset: 18, width: 2 }, Field { offset: 20, width: 1 }, Field { offset: 21, width: 1 }, Field { offset: 22, width: 1 }, Field { offset: 23, width: 1 }, Field { offset: 24, width: 1 }, Field { offset: 25, width: 1 }, Field { offset: 26, width: 2 }, Field { offset: 28, width: 2 }, Field { offset: 30, width: 1 }, Field { offset: 31, width: 1 }, Field { offset: 32, width: 1 }, Field { offset: 33, width: 1 }, Field { offset: 34, width: 1 }]);
        trailing_byte: &fixture(("0xaa", "golden"));
        count_too_large: many_wifi6(), TOO_MANY;
        lenient_absent: [&fixture(("0xaa", "empty"))];
    }
    body_checks! {
        mod cac_checks;
        body: CacCapabilities<'a>;
        golden: &fixture(("0xb2", "golden")), cac();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 2 }, Field { offset: 2, width: 1 }, Field { offset: 3, width: 6 }, Field { offset: 9, width: 1 }, Field { offset: 10, width: 1 }, Field { offset: 11, width: 3 }, Field { offset: 14, width: 1 }, Field { offset: 15, width: 1 }, Field { offset: 16, width: 1 }, Field { offset: 17, width: 2 }, Field { offset: 19, width: 1 }, Field { offset: 20, width: 1 }, Field { offset: 21, width: 1 }, Field { offset: 22, width: 1 }, Field { offset: 23, width: 3 }, Field { offset: 26, width: 1 }, Field { offset: 27, width: 1 }, Field { offset: 28, width: 1 }, Field { offset: 29, width: 1 }, Field { offset: 30, width: 6 }, Field { offset: 36, width: 1 }, Field { offset: 37, width: 1 }, Field { offset: 38, width: 3 }, Field { offset: 41, width: 1 }, Field { offset: 42, width: 1 }, Field { offset: 43, width: 1 }, Field { offset: 44, width: 3 }]);
        trailing_byte: &fixture(("0xb2", "golden"));
        count_too_large: many_cac(), TOO_MANY;
    }
    body_checks! {
        mod profile_checks;
        body: Profile2ApCapability;
        golden: &fixture(("0xb4", "golden")), profile2();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 1 }, Field { offset: 1, width: 1 }, Field { offset: 2, width: 1 }, Field { offset: 3, width: 1 }]);
        trailing_byte: &fixture(("0xb4", "golden"));
        reserved_bits: [(&fixture(("0xb4", "reserved-bits-set")), profile2(), &fixture(("0xb4", "golden")))];
    }
    body_checks! {
        mod advanced_checks;
        body: ApRadioAdvancedCapabilities;
        golden: &fixture(("0xbe", "golden")), advanced();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 1 }]);
        trailing_byte: &fixture(("0xbe", "golden"));
        reserved_bits: [(&fixture(("0xbe", "reserved-bits-set")), advanced(), &fixture(("0xbe", "golden")))];
    }
    body_checks! {
        mod backhaul_checks;
        body: BackhaulStaRadioCapabilities;
        golden: &fixture(("0xcb", "golden")), backhaul();
        lenient;
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 6 }, Field { offset: 6, width: 1 }, Field { offset: 7, width: 6 }]);
        trailing_byte: &fixture(("0xcb", "golden"));
        reserved_bits: [(&fixture(("0xcb", "reserved-bits-set")), backhaul(), &fixture(("0xcb", "golden")))];
        lenient_absent: [&fixture(("0xcb", "empty-upstream"))];
    }
    body_checks! {
        mod inventory_checks;
        body: DeviceInventory<'a>;
        golden: &fixture(("0xd4", "golden")), inventory();
        lenient;
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 1 }, Field { offset: 1, width: 7 }, Field { offset: 8, width: 1 }, Field { offset: 9, width: 5 }, Field { offset: 14, width: 1 }, Field { offset: 15, width: 5 }, Field { offset: 20, width: 1 }, Field { offset: 21, width: 6 }, Field { offset: 27, width: 1 }, Field { offset: 28, width: 8 }, Field { offset: 36, width: 6 }, Field { offset: 42, width: 1 }, Field { offset: 43, width: 3 }]);
        trailing_byte: &fixture(("0xd4", "golden"));
        count_too_large: many_inventory(), TOO_MANY;
    }
    body_checks! {
        mod controller_checks;
        body: ControllerCapability;
        golden: &fixture(("0xdd", "golden")), controller();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 1 }]);
        reserved_bits: [(&fixture(("0xdd", "reserved-bits-set")), controller(), &fixture(("0xdd", "golden")))];
    }
    body_checks! {
        mod wifi7_checks;
        body: Wifi7AgentCapabilities;
        golden: &fixture(("0xdf", "golden")), wifi7();
        prefixes: |_| false, truncated_in(&[Field { offset: 0, width: 1 }, Field { offset: 1, width: 1 }, Field { offset: 2, width: 1 }, Field { offset: 3, width: 13 }, Field { offset: 16, width: 1 }, Field { offset: 17, width: 6 }, Field { offset: 23, width: 24 }, Field { offset: 47, width: 1 }, Field { offset: 48, width: 1 }, Field { offset: 49, width: 1 }, Field { offset: 50, width: 6 }, Field { offset: 56, width: 1 }, Field { offset: 57, width: 1 }, Field { offset: 58, width: 6 }, Field { offset: 64, width: 1 }, Field { offset: 65, width: 6 }, Field { offset: 71, width: 1 }, Field { offset: 72, width: 1 }, Field { offset: 73, width: 6 }, Field { offset: 79, width: 1 }, Field { offset: 80, width: 6 }, Field { offset: 86, width: 1 }, Field { offset: 87, width: 6 }, Field { offset: 93, width: 1 }, Field { offset: 94, width: 1 }, Field { offset: 95, width: 6 }, Field { offset: 101, width: 1 }, Field { offset: 102, width: 6 }, Field { offset: 108, width: 1 }, Field { offset: 109, width: 6 }, Field { offset: 115, width: 1 }, Field { offset: 116, width: 6 }, Field { offset: 122, width: 1 }, Field { offset: 123, width: 1 }, Field { offset: 124, width: 6 }, Field { offset: 130, width: 1 }, Field { offset: 131, width: 6 }, Field { offset: 137, width: 1 }, Field { offset: 138, width: 6 }, Field { offset: 144, width: 1 }, Field { offset: 145, width: 6 }, Field { offset: 151, width: 1 }, Field { offset: 152, width: 6 }, Field { offset: 158, width: 1 }, Field { offset: 159, width: 1 }, Field { offset: 160, width: 6 }, Field { offset: 166, width: 1 }, Field { offset: 167, width: 6 }, Field { offset: 173, width: 1 }, Field { offset: 174, width: 6 }, Field { offset: 180, width: 1 }, Field { offset: 181, width: 6 }, Field { offset: 187, width: 1 }, Field { offset: 188, width: 6 }, Field { offset: 194, width: 1 }, Field { offset: 195, width: 6 }, Field { offset: 201, width: 1 }, Field { offset: 202, width: 1 }, Field { offset: 203, width: 6 }, Field { offset: 209, width: 1 }, Field { offset: 210, width: 6 }, Field { offset: 216, width: 1 }, Field { offset: 217, width: 6 }, Field { offset: 223, width: 1 }, Field { offset: 224, width: 6 }, Field { offset: 230, width: 1 }, Field { offset: 231, width: 6 }, Field { offset: 237, width: 1 }, Field { offset: 238, width: 6 }, Field { offset: 244, width: 1 }, Field { offset: 245, width: 6 }, Field { offset: 251, width: 1 }, Field { offset: 252, width: 1 }, Field { offset: 253, width: 6 }, Field { offset: 259, width: 1 }, Field { offset: 260, width: 6 }, Field { offset: 266, width: 1 }, Field { offset: 267, width: 6 }, Field { offset: 273, width: 1 }, Field { offset: 274, width: 6 }, Field { offset: 280, width: 1 }, Field { offset: 281, width: 6 }, Field { offset: 287, width: 1 }, Field { offset: 288, width: 6 }, Field { offset: 294, width: 1 }, Field { offset: 295, width: 6 }, Field { offset: 301, width: 1 }, Field { offset: 302, width: 6 }, Field { offset: 308, width: 1 }, Field { offset: 309, width: 6 }, Field { offset: 315, width: 24 }, Field { offset: 339, width: 1 }, Field { offset: 340, width: 1 }, Field { offset: 341, width: 1 }, Field { offset: 342, width: 6 }, Field { offset: 348, width: 1 }, Field { offset: 349, width: 1 }, Field { offset: 350, width: 6 }, Field { offset: 356, width: 1 }, Field { offset: 357, width: 1 }, Field { offset: 358, width: 6 }, Field { offset: 364, width: 1 }, Field { offset: 365, width: 1 }, Field { offset: 366, width: 6 }, Field { offset: 372, width: 1 }, Field { offset: 373, width: 1 }, Field { offset: 374, width: 6 }, Field { offset: 380, width: 1 }, Field { offset: 381, width: 1 }, Field { offset: 382, width: 6 }, Field { offset: 388, width: 1 }, Field { offset: 389, width: 1 }, Field { offset: 390, width: 6 }, Field { offset: 396, width: 1 }, Field { offset: 397, width: 1 }, Field { offset: 398, width: 6 }, Field { offset: 404, width: 1 }]);
        trailing_byte: &fixture(("0xdf", "golden"));
        count_too_large: many_wifi7(), TOO_MANY;
        reserved_bits: [(&fixture(("0xdf", "reserved-bits-set")), Wifi7AgentCapabilities::decode_value(&fixture(("0xdf", "reserved-bits-clear"))).unwrap(), &fixture(("0xdf", "reserved-bits-clear")))];
    }

    #[test]
    fn upstream_create_prof_2_vector_decodes_to_dpp_only() {
        let source = include_str!("../../tests/data/upstream_tlv_vectors.tsv");
        let row = source
            .lines()
            .find(|line| line.starts_with("default\tem_t::create_prof_2_tlv\t"))
            .unwrap();
        let hex = row.split('\t').nth(4).unwrap();
        let value: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(value, [0, 0, 0x10, 0]);
        let expected = Profile2ApCapability {
            max_prioritization_rules: 0,
            byte_counter_units: 0,
            prioritization: false,
            dpp_onboarding: true,
            traffic_separation: false,
            max_vids: 0,
        };
        assert_eq!(Profile2ApCapability::decode_value(&value), Ok(expected));
        assert_eq!(expected.encode_value(), Ok(value));
    }

    #[test]
    fn he_spec_bits_differ_from_upstream_one_bit_lower() {
        let flags: [fn(&ApHeCapabilities) -> bool; 7] = [
            |cap| cap.su_beamformer,
            |cap| cap.mu_beamformer,
            |cap| cap.ul_mu_mimo,
            |cap| cap.ul_mu_mimo_ofdma,
            |cap| cap.dl_mu_mimo_ofdma,
            |cap| cap.ul_ofdma,
            |cap| cap.dl_ofdma,
        ];
        for (index, getter) in flags.iter().enumerate() {
            let mut bytes = [
                0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0, 4, 0x12, 0x34, 0xab, 0xcd, 0, 0,
            ];
            bytes[12] = 0x80 >> index;
            let decoded = ApHeCapabilities::decode_value(&bytes).unwrap();
            assert_eq!(
                decoded.mcs,
                HeMcsSets::One([HeMcsSet {
                    rx: 0x1234,
                    tx: 0xabcd
                }])
            );
            for (other_index, other) in flags.iter().enumerate() {
                assert_eq!(
                    other(&decoded),
                    other_index == index,
                    "flag {index}, compared with {other_index}"
                );
            }
            assert!(getter(&decoded));
            assert_eq!(decoded.encode_value().unwrap(), bytes);
        }
    }

    #[test]
    fn spec_mcs_maps_are_receive_first_and_use_each_tlvs_byte_order() {
        let he = he();
        assert_eq!(
            &he.encode_value().unwrap()[7..15],
            &[0xff, 0xfa, 0xff, 0xf5, 0xff, 0xfc, 0xff, 0xf3]
        );
        let wifi6 = wifi6();
        assert_eq!(
            &wifi6.encode_value().unwrap()[8..20],
            &[
                0xfa, 0xff, 0xf5, 0xff, 0xfc, 0xff, 0xf3, 0xff, 0xfe, 0xff, 0xf9, 0xff
            ]
        );
        let vht = vht();
        assert_eq!(
            &vht.encode_value().unwrap()[6..10],
            &[0xff, 0xfa, 0xff, 0xf5]
        );
    }

    #[test]
    fn basic_opclass_groups_are_contiguous_unlike_upstream_holed_sender() {
        let encoded = basic().encode_value().unwrap();
        assert_eq!(
            &encoded[8..],
            &[115, 23, 1, 48, 121, 0xfd, 3, 120, 124, 128]
        );
    }

    #[test]
    fn small_integer_fields_and_cac_duration_cannot_be_truncated() {
        let mut ht = ht();
        ht.max_tx_streams_code = 4;
        assert_eq!(
            ht.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 4, max: 3 })
        );
        let mut he = he();
        he.max_rx_streams_code = 8;
        assert_eq!(
            he.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 8, max: 7 })
        );
        let mut bad_role = wifi6();
        bad_role.roles[0].role = 4;
        assert_eq!(
            bad_role.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 4, max: 3 })
        );
        let mut bad_users = wifi6();
        bad_users.roles[0].max_dl_mu_mimo_tx = 16;
        assert_eq!(
            bad_users.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 16, max: 15 })
        );
        let mut wifi7 = wifi7();
        wifi7.radios[0].ap.str_records[0].frequency_separation = 32;
        assert_eq!(
            wifi7.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 32, max: 31 })
        );
        let mut cac = cac();
        cac.radios[0].methods[0].duration_seconds = 0x100_0000;
        assert_eq!(
            cac.encode_value(),
            Err(EncodeError::ValueTooLarge {
                value: 0x100_0000,
                max: 0xff_ffff
            })
        );
    }

    #[test]
    fn nested_counts_and_inventory_string_limit_are_checked() {
        let too_many = EncodeError::CountTooLarge { len: 256, max: 255 };
        let mut basic = basic();
        basic.operating_classes[0].non_operable_channels = &[0; 256];
        assert_eq!(basic.encode_value(), Err(too_many));
        let mut cac = cac();
        cac.radios[0].methods[0].operating_classes[0].channels = &[0; 256];
        assert_eq!(cac.encode_value(), Err(too_many));
        let mut wifi6 = wifi6();
        wifi6.roles = vec![wifi6.roles[0]; 256];
        assert_eq!(wifi6.encode_value(), Err(too_many));
        let mut wifi7 = wifi7();
        wifi7.radios[0].ap.str_records = vec![
            Wifi7FrequencyRecord {
                ruid: RUID,
                frequency_separation: 0
            };
            256
        ];
        assert_eq!(wifi7.encode_value(), Err(too_many));
        let mut inventory = inventory();
        inventory.serial_number = &[0; 65];
        assert_eq!(
            inventory.encode_value(),
            Err(EncodeError::CountTooLarge { len: 65, max: 64 })
        );
        inventory.serial_number = &[0; 64];
        inventory.radios_mut()[0].chipset_vendor = &[0; 65];
        assert_eq!(
            inventory.encode_value(),
            Err(EncodeError::CountTooLarge { len: 65, max: 64 })
        );
    }

    #[test]
    fn largest_one_octet_counts_round_trip_without_truncation() {
        let mut basic = many_basic();
        basic.operating_classes.truncate(255);
        let encoded = basic.encode_value().unwrap();
        assert_eq!(ApRadioBasicCapabilities::decode_value(&encoded), Ok(basic));

        let mut wifi6 = many_wifi6();
        wifi6.roles.truncate(255);
        let encoded = wifi6.encode_value().unwrap();
        assert_eq!(ApWifi6Capabilities::decode_value(&encoded), Ok(wifi6));

        let mut cac = many_cac();
        cac.radios.truncate(255);
        let encoded = cac.encode_value().unwrap();
        assert_eq!(CacCapabilities::decode_value(&encoded), Ok(cac));

        let mut inventory = many_inventory();
        inventory.radios.truncate(255);
        let encoded = inventory.encode_value().unwrap();
        assert_eq!(DeviceInventory::decode_value(&encoded), Ok(inventory));

        let mut wifi7 = many_wifi7();
        wifi7.radios.truncate(255);
        wifi7.radios[0].ap.str_records = vec![
            Wifi7FrequencyRecord {
                ruid: RUID,
                frequency_separation: 31,
            };
            255
        ];
        let encoded = wifi7.encode_value().unwrap();
        assert_eq!(Wifi7AgentCapabilities::decode_value(&encoded), Ok(wifi7));
    }

    #[test]
    fn report_frame_is_borrowed_and_fills_maximum_tlv_value() {
        let frame = vec![0xa5; 65_534];
        let body = ClientCapabilityReport::new(0, Some(&frame)).unwrap();
        let message = crate::body_testkit::build_message(&body).unwrap();
        let tlv = crate::body_testkit::single_tlv(&message).unwrap();
        let decoded = tlv.decode::<ClientCapabilityReport<'_>>().unwrap();
        assert_eq!(decoded.frame_body(), Some(frame.as_slice()));
        assert!(core::ptr::eq(
            decoded.frame_body().unwrap().as_ptr(),
            tlv.value[1..].as_ptr()
        ));
        assert!(ClientCapabilityReport::new(0, None).is_none());
        assert!(ClientCapabilityReport::new(1, Some(&[])).is_none());
        assert!(ClientCapabilityReport::new(2, None).is_some());
    }

    #[test]
    fn strict_inventory_requires_a_radio_while_lenient_zero_radios_does_not_absorb_trailing_bytes()
    {
        assert!(DeviceInventory::new(b"", b"", b"", vec![]).is_none());
        let mut value = fixture(("0xd4", "zero-radios"));
        assert!(matches!(
            DeviceInventory::decode_value(&value),
            Err(DecodeError::InvalidLength { .. })
        ));
        assert_eq!(
            DeviceInventory::decode_value_lenient(&value),
            Ok(Some(inventory_empty()))
        );
        value.push(0xaa);
        assert_eq!(
            DeviceInventory::decode_value_lenient(&value),
            Err(DecodeError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn lenient_zero_radio_inventory_does_not_encode() {
        // The encoder never writes bytes the strict decoder rejects (note 0009 D4).
        let value = fixture(("0xd4", "zero-radios"));
        let lenient = DeviceInventory::decode_value_lenient(&value)
            .unwrap()
            .unwrap();
        assert_eq!(
            lenient.encode_value(),
            Err(EncodeError::CountTooSmall { len: 0, min: 1 })
        );
        let tlv = crate::Tlv {
            tlv_type: TlvType::DEVICE_INVENTORY,
            value: &value,
        };
        assert_eq!(
            tlv.decode_lenient::<DeviceInventory<'_>>(),
            Ok(Some(lenient))
        );
        assert!(tlv.decode::<DeviceInventory<'_>>().is_err());
    }

    /// A wire flag: octet offset in the value, bit mask, and a setter for the matching field.
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

    fn zero_mcs() -> HeMcsSets {
        HeMcsSets::One([HeMcsSet { rx: 0, tx: 0 }])
    }

    #[test]
    fn ap_capability_flags_each_use_their_own_bit() {
        // AP Capability: bit 7 on-channel, 6 off-channel, 5 RCPI steering, 4 M8.
        let flags: &[Flag<ApCapability>] = &[
            (0, 0x80, |b| b.on_channel_metrics = true),
            (0, 0x40, |b| b.off_channel_metrics = true),
            (0, 0x20, |b| b.rcpi_steering = true),
            (0, 0x10, |b| b.m8_backhaul_reconfiguration = true),
        ];
        assert_each_flag_alone(
            &[0],
            || ApCapability {
                on_channel_metrics: false,
                off_channel_metrics: false,
                rcpi_steering: false,
                m8_backhaul_reconfiguration: false,
            },
            flags,
        );
    }

    #[test]
    fn ht_fields_each_use_their_own_bits() {
        // HT Capabilities: bits 7-6 Tx streams, 5-4 Rx streams, 3 SGI 20, 2 SGI 40, 1 HT 40.
        let flags: &[Flag<ApHtCapabilities>] = &[
            (6, 0x40, |b| b.max_tx_streams_code = 1),
            (6, 0x10, |b| b.max_rx_streams_code = 1),
            (6, 0x08, |b| b.short_gi_20 = true),
            (6, 0x04, |b| b.short_gi_40 = true),
            (6, 0x02, |b| b.ht_40 = true),
        ];
        assert_each_flag_alone(
            &[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0, 0],
            || ApHtCapabilities {
                ruid: RUID,
                max_tx_streams_code: 0,
                max_rx_streams_code: 0,
                short_gi_20: false,
                short_gi_40: false,
                ht_40: false,
            },
            flags,
        );
    }

    #[test]
    fn vht_fields_each_use_their_own_bits() {
        // VHT Capabilities: octet 10 bits 7-5 Tx streams, 4-2 Rx streams, 1 SGI 80, 0 SGI 160;
        // octet 11 bit 7 80+80, 6 160, 5 SU beamformer, 4 MU beamformer.
        let flags: &[Flag<ApVhtCapabilities>] = &[
            (10, 0x20, |b| b.max_tx_streams_code = 1),
            (10, 0x04, |b| b.max_rx_streams_code = 1),
            (10, 0x02, |b| b.short_gi_80 = true),
            (10, 0x01, |b| b.short_gi_160 = true),
            (11, 0x80, |b| b.vht_80p80 = true),
            (11, 0x40, |b| b.vht_160 = true),
            (11, 0x20, |b| b.su_beamformer = true),
            (11, 0x10, |b| b.mu_beamformer = true),
        ];
        assert_each_flag_alone(
            &[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0, 0, 0, 0, 0, 0, 0],
            || ApVhtCapabilities {
                ruid: RUID,
                tx_mcs: 0,
                rx_mcs: 0,
                max_tx_streams_code: 0,
                max_rx_streams_code: 0,
                short_gi_80: false,
                short_gi_160: false,
                vht_80p80: false,
                vht_160: false,
                su_beamformer: false,
                mu_beamformer: false,
            },
            flags,
        );
    }

    #[test]
    fn profile2_flags_each_use_their_own_bit() {
        // Profile-2 AP Capability octet 3: bits 7-6 byte counter units, 5 prioritization,
        // 4 DPP onboarding, 3 traffic separation.
        let flags: &[Flag<Profile2ApCapability>] = &[
            (2, 0x40, |b| b.byte_counter_units = 1),
            (2, 0x20, |b| b.prioritization = true),
            (2, 0x10, |b| b.dpp_onboarding = true),
            (2, 0x08, |b| b.traffic_separation = true),
        ];
        assert_each_flag_alone(
            &[0, 0, 0, 0],
            || Profile2ApCapability {
                max_prioritization_rules: 0,
                byte_counter_units: 0,
                prioritization: false,
                dpp_onboarding: false,
                traffic_separation: false,
                max_vids: 0,
            },
            flags,
        );
    }

    #[test]
    fn advanced_capability_flags_each_use_their_own_bit() {
        // Table 74: bit 7 combined front/back, 6 combined Profile-1/2, 5 MSCS, 4 SCS,
        // 3 QoS map, 2 DSCP policy, 1 QM SCS traffic description.
        let flags: &[Flag<ApRadioAdvancedCapabilities>] = &[
            (6, 0x80, |b| b.combined_front_back = true),
            (6, 0x40, |b| b.combined_profile_1_2 = true),
            (6, 0x20, |b| b.mscs = true),
            (6, 0x10, |b| b.scs = true),
            (6, 0x08, |b| b.qos_map = true),
            (6, 0x04, |b| b.dscp_policy = true),
            (6, 0x02, |b| b.qm_scs_traffic_description = true),
        ];
        assert_each_flag_alone(
            &[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0, 0],
            || ApRadioAdvancedCapabilities {
                ruid: RUID,
                combined_front_back: false,
                combined_profile_1_2: false,
                mscs: false,
                scs: false,
                qos_map: false,
                dscp_policy: false,
                qm_scs_traffic_description: false,
            },
            flags,
        );
    }

    fn zero_wifi6_role() -> Wifi6Role {
        Wifi6Role {
            role: 0,
            he_160: false,
            he_80p80: false,
            mcs: zero_mcs(),
            su_beamformer: false,
            su_beamformee: false,
            mu_beamformer: false,
            beamformee_sts_le_80: false,
            beamformee_sts_gt_80: false,
            ul_mu_mimo: false,
            ul_ofdma: false,
            dl_ofdma: false,
            max_dl_mu_mimo_tx: 0,
            max_ul_mu_mimo_rx: 0,
            max_dl_ofdma_tx: 0,
            max_ul_ofdma_rx: 0,
            rts: false,
            mu_rts: false,
            multi_bssid: false,
            mu_edca: false,
            twt_requester: false,
            twt_responder: false,
            spatial_reuse: false,
            anticipated_channel_usage: false,
        }
    }

    #[test]
    fn wifi6_role_fields_each_use_their_own_bits() {
        // Table 94 role record, octets counted in the whole value (RUID 0-5, role count 6).
        // Octet 7: bits 7-6 role, 5 HE 160, 4 HE 80+80, 3-0 MCS length.
        // Octet 12: bit 7 SU beamformer, 6 SU beamformee, 5 MU beamformer, 4 beamformee
        // STS <= 80, 3 beamformee STS > 80, 2 UL MU-MIMO, 1 UL OFDMA, 0 DL OFDMA.
        // Octet 13: bits 7-4 max DL MU-MIMO Tx, 3-0 max UL MU-MIMO Rx. Octet 14: max DL OFDMA
        // Tx. Octet 15: max UL OFDMA Rx. Octet 16: bit 7 RTS, 6 MU RTS, 5 multi-BSSID,
        // 4 MU EDCA, 3 TWT requester, 2 TWT responder, 1 spatial reuse,
        // 0 anticipated channel usage.
        let flags: &[Flag<ApWifi6Capabilities>] = &[
            (7, 0x40, |b| b.roles[0].role = 1),
            (7, 0x20, |b| b.roles[0].he_160 = true),
            (7, 0x10, |b| b.roles[0].he_80p80 = true),
            (12, 0x80, |b| b.roles[0].su_beamformer = true),
            (12, 0x40, |b| b.roles[0].su_beamformee = true),
            (12, 0x20, |b| b.roles[0].mu_beamformer = true),
            (12, 0x10, |b| b.roles[0].beamformee_sts_le_80 = true),
            (12, 0x08, |b| b.roles[0].beamformee_sts_gt_80 = true),
            (12, 0x04, |b| b.roles[0].ul_mu_mimo = true),
            (12, 0x02, |b| b.roles[0].ul_ofdma = true),
            (12, 0x01, |b| b.roles[0].dl_ofdma = true),
            (13, 0x10, |b| b.roles[0].max_dl_mu_mimo_tx = 1),
            (13, 0x01, |b| b.roles[0].max_ul_mu_mimo_rx = 1),
            (14, 0x01, |b| b.roles[0].max_dl_ofdma_tx = 1),
            (15, 0x01, |b| b.roles[0].max_ul_ofdma_rx = 1),
            (16, 0x80, |b| b.roles[0].rts = true),
            (16, 0x40, |b| b.roles[0].mu_rts = true),
            (16, 0x20, |b| b.roles[0].multi_bssid = true),
            (16, 0x10, |b| b.roles[0].mu_edca = true),
            (16, 0x08, |b| b.roles[0].twt_requester = true),
            (16, 0x04, |b| b.roles[0].twt_responder = true),
            (16, 0x02, |b| b.roles[0].spatial_reuse = true),
            (16, 0x01, |b| b.roles[0].anticipated_channel_usage = true),
        ];
        let mut zero_bytes = vec![0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0, 1];
        // Role head with MCS length 4, one zero MCS set, then the five zero flag octets.
        zero_bytes.extend_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_each_flag_alone(
            &zero_bytes,
            || ApWifi6Capabilities {
                ruid: RUID,
                roles: vec![zero_wifi6_role()],
            },
            flags,
        );
    }

    fn zero_wifi7_mode() -> Wifi7ModeCapabilities {
        Wifi7ModeCapabilities {
            str_supported: false,
            nstr_supported: false,
            emlsr_supported: false,
            emlmr_supported: false,
            str_records: vec![],
            nstr_records: vec![],
            emlsr_records: vec![],
            emlmr_records: vec![],
        }
    }

    #[test]
    fn wifi7_mode_flags_each_use_their_own_bit() {
        // Table 117 radio record: octet 47 is the AP mode flags and octet 48 the backhaul STA
        // mode flags (header 17, RUID 6, reserved 24). In each: bit 7 STR, 6 NSTR, 5 EMLSR,
        // 4 EMLMR.
        let flags: &[Flag<Wifi7AgentCapabilities>] = &[
            (47, 0x80, |b| b.radios[0].ap.str_supported = true),
            (47, 0x40, |b| b.radios[0].ap.nstr_supported = true),
            (47, 0x20, |b| b.radios[0].ap.emlsr_supported = true),
            (47, 0x10, |b| b.radios[0].ap.emlmr_supported = true),
            (48, 0x80, |b| b.radios[0].backhaul_sta.str_supported = true),
            (48, 0x40, |b| b.radios[0].backhaul_sta.nstr_supported = true),
            (48, 0x20, |b| {
                b.radios[0].backhaul_sta.emlsr_supported = true;
            }),
            (48, 0x10, |b| {
                b.radios[0].backhaul_sta.emlmr_supported = true;
            }),
        ];
        let mut zero_bytes = vec![0; 16];
        zero_bytes.push(1);
        zero_bytes.extend_from_slice(&[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]);
        // 24 reserved octets, two flag octets and eight empty record counts.
        zero_bytes.extend_from_slice(&[0; 34]);
        assert_each_flag_alone(
            &zero_bytes,
            || Wifi7AgentCapabilities {
                max_mlds: 0,
                ap_maximum_links: 0,
                backhaul_sta_maximum_links: 0,
                tid_to_link_mapping: 0,
                radios: vec![Wifi7Radio {
                    ruid: RUID,
                    ap: zero_wifi7_mode(),
                    backhaul_sta: zero_wifi7_mode(),
                }],
            },
            flags,
        );
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<BasicOperatingClass<'_>>() <= 4 * word);
        assert!(size_of::<ApRadioBasicCapabilities<'_>>() <= 5 * word);
        assert!(size_of::<ApCapability>() <= 4);
        assert!(size_of::<ApHtCapabilities>() <= 16);
        assert!(size_of::<ApVhtCapabilities>() <= 24);
        assert!(size_of::<HeMcsSet>() <= 4);
        assert!(size_of::<HeMcsSets>() <= 16);
        assert!(size_of::<ApHeCapabilities>() <= 40);
        assert!(size_of::<ClientInfo>() <= 12);
        assert!(size_of::<ClientCapabilityReport<'_>>() <= 4 * word);
        assert!(size_of::<CacOperatingClass<'_>>() <= 3 * word);
        assert!(size_of::<CacMethod<'_>>() <= 5 * word);
        assert!(size_of::<CacRadio<'_>>() <= 5 * word);
        assert!(size_of::<CacCapabilities<'_>>() <= 4 * word);
        assert!(size_of::<Profile2ApCapability>() <= 8);
        assert!(size_of::<ApRadioAdvancedCapabilities>() <= 16);
        assert!(size_of::<BackhaulStaRadioCapabilities>() <= 16);
        assert!(size_of::<Wifi6Role>() <= 48);
        assert!(size_of::<ApWifi6Capabilities>() <= 5 * word);
        assert!(size_of::<InventoryRadio<'_>>() <= 4 * word);
        assert!(size_of::<DeviceInventory<'_>>() <= 9 * word);
        assert!(size_of::<ControllerCapability>() <= 2);
        assert!(size_of::<Wifi7FrequencyRecord>() <= 8);
        assert!(size_of::<Wifi7ModeCapabilities>() <= 13 * word);
        assert!(size_of::<Wifi7Radio>() <= 28 * word);
        assert!(size_of::<Wifi7AgentCapabilities>() <= 4 * word);
    }
}
