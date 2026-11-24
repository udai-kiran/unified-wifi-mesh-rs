// SPDX-License-Identifier: Apache-2.0

//! Autoconfiguration, search, topology, profile, WSC, and operational BSS bodies.

use em_types::{FreqBand, MacAddr, ProfileType, ServiceType, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};
use crate::wsc::WscAttrIter;

open_code! {
    /// IEEE 1905.1 autoconfiguration role (§6.4.14/16, Tables 6-22/24);
    /// audit rows 329–330 and 334–335. Upstream uses the TLV header at
    /// `em_base.h:458-462`; senders `em_configuration.cpp:4555-4557`,
    /// `:4399-4405`, receivers validate only (`em_msg.cpp:715`, `:725`).
    /// Reserved values are retained.
    pub struct Role(u8);
    const HEX_WIDTH: usize = 2;
    entries: [(REGISTRAR, 0x00, "Registrar.")]
}

open_code! {
    /// Agent List security code, `EasyMesh` §17.2.77 Table 99; audit rows 237–238.
    /// Upstream `em_agent_list_agent_t` (`em_base.h:967-971`), sender
    /// `em_configuration.cpp:3981-4006`, receiver `:5801-5826`.
    /// Reserved values are retained.
    pub struct AgentSecurity(u8);
    const HEX_WIDTH: usize = 2;
    entries: [
        (DISABLED, 0x00, "Security is not enabled."),
        (ENABLED, 0x01, "Security is enabled."),
    ]
}

open_code! {
    /// Profile-2 Error Code reason, `EasyMesh` §17.2.51 Table 73; audit rows 181–182.
    /// Upstream enum `em_base.h:1428-1444`; there is no production sender or
    /// receiver (`em_msg.cpp:1121-1124` validates presence only).
    /// Reserved values are retained.
    pub struct Profile2ErrorReason(u8);
    const HEX_WIDTH: usize = 2;
    entries: [
        (RULE_NOT_FOUND, 0x01, "Service prioritization rule not found."),
        (RULES_EXCEEDED, 0x02, "Maximum number of service prioritization rules exceeded."),
        (PCP_VLAN_NOT_PROVIDED, 0x03, "PCP or VLAN ID not provided."),
        (VLAN_IDS_EXCEEDED, 0x05, "Maximum number of VLAN IDs exceeded."),
        (COMBINED_TRAFFIC_SEPARATION_UNSUPPORTED, 0x07, "Combined traffic separation unsupported."),
        (MIXED_TRAFFIC_SEPARATION_UNSUPPORTED, 0x08, "Mixed traffic separation unsupported."),
        (TRAFFIC_SEPARATION_UNSUPPORTED, 0x0A, "Traffic separation unsupported."),
        (QOS_POLICY_NOT_CONFIGURED, 0x0B, "`QoS` management policy not configured."),
        (DSCP_POLICY_REJECTED, 0x0C, "`QoS` management DSCP policy rejected."),
        (CANNOT_ONBOARD_OTHER_AGENTS, 0x0D, "Agent cannot onboard other Agents via DPP over Wi-Fi."),
        (EHT_SUBCHANNEL_BITMAP_ERROR, 0x0E, "EHT subchannel bitmap error."),
    ]
}

/// Shared count and service list for `EasyMesh` §17.2.1 Table 23 and §17.2.2 Table 24.
/// Upstream structs `em_base.h:2199-2207`; senders `em_configuration.cpp:1758-1775`,
/// `:4574-4577`; supported-service receiver `em_msg.cpp:89-129`, no searched-service receiver.
/// Audit rows 25–29; reserved codes and zero counts are preserved.
#[derive(Debug, PartialEq, Eq)]
pub struct ServiceList {
    /// Services in wire order, including reserved codes.
    pub services: Vec<ServiceType>,
}

impl ServiceList {
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut services = bounded_vec(count, reader.remaining(), 1);
        for _ in 0..count {
            services.push(ServiceType::from(reader.u8()?));
        }
        reader.finish()?;
        Ok(Self { services })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.services.len())?;
        for &service in &self.services {
            writer.u8(u8::from(service));
        }
        Ok(writer.into_bytes())
    }
}

tlv_newtype! {
    /// `SupportedService`, `EasyMesh` §17.2.1 Table 23; audit rows 25–28.
    /// Upstream `em_supported_service_t` (`em_base.h:2199-2202`), sender
    /// `em_configuration.cpp:1758-1775`, receiver `em_msg.cpp:89-129`.
    #[derive(Debug, PartialEq, Eq)]
    pub struct SupportedService(pub ServiceList) => TlvType::SUPPORTED_SERVICE;
}

tlv_newtype! {
    /// `SearchedService`, `EasyMesh` §17.2.2 Table 24; audit row 29.
    /// Upstream `em_searched_service_t` (`em_base.h:2204-2207`), sender
    /// `em_configuration.cpp:4574-4577`; no receiver at the pin.
    #[derive(Debug, PartialEq, Eq)]
    pub struct SearchedService(pub ServiceList) => TlvType::SEARCHED_SERVICE;
}

/// Shared six-octet address layout for `EasyMesh` §17.2.3 Table 25 and
/// §17.2.74 Table 96; audit rows 30–31 and 226–227. Upstream TLV header
/// `em_base.h:458-462`; senders `em_configuration.cpp:4187-4194`, `:608-614`,
/// receivers `em_msg.cpp:270-282`, `em_ctrl.cpp:668-672`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacValue {
    /// MAC address in the value.
    pub mac: MacAddr,
}

impl MacValue {
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let mac = reader.mac()?;
        reader.finish()?;
        Ok(Self { mac })
    }

    #[allow(clippy::unnecessary_wraps)]
    fn encode_value(self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.mac);
        Ok(writer.into_bytes())
    }
}

tlv_newtype! {
    /// AP Radio Identifier, `EasyMesh` §17.2.3 Table 25; audit rows 30–31.
    /// Upstream uses the address directly: sender `em_configuration.cpp:4187-4194`,
    /// receiver `em_msg.cpp:270-282`; TLV header `em_base.h:458-462`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ApRadioIdentifier(pub MacValue) => TlvType::AP_RADIO_IDENTIFIER;
}

/// One BSS in AP Operational BSS, `EasyMesh` §17.2.4 Table 26; audit rows 32–36.
/// Upstream `em_ap_operational_bss_t` (`em_base.h:1634-1638`), sender
/// `em_configuration.cpp:1083-1107`, receiver `:2244-2294`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationalBss<'a> {
    /// AP MAC address.
    pub bssid: MacAddr,
    /// Raw SSID octets, borrowed from the TLV; a final NUL is preserved.
    pub ssid: &'a [u8],
}

/// One radio in AP Operational BSS, `EasyMesh` §17.2.4 Table 26; audit rows 32–36.
/// Upstream `em_ap_op_bss_radio_t` (`em_base.h:1640-1644`), sender
/// `em_configuration.cpp:1083-1107`, receiver `:2244-2294`.
#[derive(Debug, PartialEq, Eq)]
pub struct OperationalRadio<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// BSS entries in wire order.
    pub bsses: Vec<OperationalBss<'a>>,
}

/// AP Operational BSS, `EasyMesh` §17.2.4 Table 26; audit rows 32–36.
/// Upstream `em_ap_op_bss_t` (`em_base.h:1646-1649`), sender
/// `em_configuration.cpp:1064-1113`, receiver `:2229-2303`. SSIDs are raw bytes,
/// including a final NUL when the length includes it (note 0009 D4).
#[derive(Debug, PartialEq, Eq)]
pub struct ApOperationalBss<'a> {
    /// Radios in wire order.
    pub radios: Vec<OperationalRadio<'a>>,
}

impl<'a> TlvBody<'a> for ApOperationalBss<'a> {
    const TLV_TYPE: TlvType = TlvType::AP_OPERATIONAL_BSS;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 7);
        for _ in 0..count {
            let ruid = reader.mac()?;
            let bss_count = usize::from(reader.u8()?);
            let mut bsses = bounded_vec(bss_count, reader.remaining(), 7);
            for _ in 0..bss_count {
                let bssid = reader.mac()?;
                let len = usize::from(reader.u8()?);
                let ssid = reader.bytes(len)?;
                bsses.push(OperationalBss { bssid, ssid });
            }
            radios.push(OperationalRadio { ruid, bsses });
        }
        reader.finish()?;
        Ok(Self { radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.count_u8(radio.bsses.len())?;
            for bss in &radio.bsses {
                writer.mac(bss.bssid);
                writer.count_u8(bss.ssid.len())?;
                writer.bytes(bss.ssid);
            }
        }
        Ok(writer.into_bytes())
    }
}

/// One associated station, `EasyMesh` §17.2.5 Table 27; audit row 37.
/// Upstream `em_assoc_clients_sta_t` (`em_base.h:1651-1654`); no production
/// sender or receiver for this TLV at the pin (`em_msg.cpp:778` validates presence only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssociatedClient {
    /// Station MAC address.
    pub mac: MacAddr,
    /// Association time in seconds; `0xffff` means 65,535 seconds or more.
    pub association_time: u16,
}

/// One BSS and its stations, `EasyMesh` §17.2.5 Table 27; audit row 37.
/// Upstream `em_assoc_clients_bss_t` (`em_base.h:1656-1660`) contains an
/// in-memory pointer; there is no production sender or receiver at the pin.
#[derive(Debug, PartialEq, Eq)]
pub struct AssociatedBss {
    /// AP MAC address.
    pub bssid: MacAddr,
    /// Associated stations in wire order.
    pub clients: Vec<AssociatedClient>,
}

/// Associated Clients, `EasyMesh` §17.2.5 Table 27; audit row 37.
/// Upstream `em_assoc_clients_t` (`em_base.h:1662-1665`) is an in-memory layout;
/// no production sender or receiver exists at the pin (`em_msg.cpp:778`).
#[derive(Debug, PartialEq, Eq)]
pub struct AssociatedClients {
    /// BSS entries in wire order.
    pub bsses: Vec<AssociatedBss>,
}

impl TlvBody<'_> for AssociatedClients {
    const TLV_TYPE: TlvType = TlvType::ASSOCIATED_CLIENTS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut bsses = bounded_vec(count, reader.remaining(), 8);
        for _ in 0..count {
            let bssid = reader.mac()?;
            let client_count = usize::from(reader.be_u16()?);
            let mut clients = bounded_vec(client_count, reader.remaining(), 8);
            for _ in 0..client_count {
                clients.push(AssociatedClient {
                    mac: reader.mac()?,
                    association_time: reader.be_u16()?,
                });
            }
            bsses.push(AssociatedBss { bssid, clients });
        }
        reader.finish()?;
        Ok(Self { bsses })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.bsses.len())?;
        for bss in &self.bsses {
            writer.mac(bss.bssid);
            writer.count_u16(bss.clients.len())?;
            for client in &bss.clients {
                writer.mac(client.mac);
                writer.be_u16(client.association_time);
            }
        }
        Ok(writer.into_bytes())
    }
}

/// Multi-AP Profile, `EasyMesh` §17.2.47 Table 69; audit rows 163–168.
/// Upstream enum `em_base.h:493-499`, senders `em_configuration.cpp:984-990`,
/// `:3614`, receiver `em_msg.cpp:131-150`. Strict form is one octet; the lenient
/// four-octet form preserves its first octet and discards the remaining three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiApProfile {
    /// Profile code, including reserved values.
    pub profile: ProfileType,
}

impl TlvBody<'_> for MultiApProfile {
    const TLV_TYPE: TlvType = TlvType::MULTI_AP_PROFILE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let profile = ProfileType::from(reader.u8()?);
        reader.finish()?;
        Ok(Self { profile })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        Ok(vec![u8::from(self.profile)])
    }
}

impl TlvBodyLenient<'_> for MultiApProfile {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if let [profile, _, _, _] = value {
            Ok(Some(Self {
                profile: ProfileType::from(*profile),
            }))
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// Conditional data in a Profile-2 Error Code, `EasyMesh` §17.2.51 Table 73;
/// audit rows 181–182. The upstream enum is `em_base.h:1428-1444`; no production
/// sender or receiver at the pin (`em_msg.cpp:1121-1124` only validates presence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile2ErrorDetail {
    /// The reason has no additional fields.
    None,
    /// BSSID, for reason `0x07`, `0x08`, or `0x0e`.
    Bssid(MacAddr),
    /// Service prioritization rule ID, for reason `0x01` or `0x02`.
    RuleId(u32),
    /// `QoS` management identifier, for reason `0x0b`.
    Qmid(u16),
}

/// Profile-2 Error Code, `EasyMesh` §17.2.51 Table 73; audit rows 181–182.
/// Upstream reason enum `em_base.h:1428-1444`; upstream's fixed layout
/// `em_prof2_error_t` (`em_base.h:1028-1033`) carries every conditional field, which
/// this body does not. No production sender or receiver
/// (`em_msg.cpp:1121-1124` validates presence only). The constructor makes a
/// reason and conditional field mismatch unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile2ErrorCode {
    reason: Profile2ErrorReason,
    detail: Profile2ErrorDetail,
}

impl Profile2ErrorCode {
    /// Creates a body if the conditional field agrees with the reason.
    #[must_use]
    pub const fn new(reason: Profile2ErrorReason, detail: Profile2ErrorDetail) -> Option<Self> {
        let valid = matches!(
            (reason.0, detail),
            (0x07 | 0x08 | 0x0e, Profile2ErrorDetail::Bssid(_))
                | (0x01 | 0x02, Profile2ErrorDetail::RuleId(_))
                | (0x0b, Profile2ErrorDetail::Qmid(_))
        ) || (!matches!(reason.0, 0x01 | 0x02 | 0x07 | 0x08 | 0x0b | 0x0e)
            && matches!(detail, Profile2ErrorDetail::None));
        if valid {
            Some(Self { reason, detail })
        } else {
            None
        }
    }

    /// Returns the reason, including reserved values.
    #[must_use]
    pub const fn reason(&self) -> Profile2ErrorReason {
        self.reason
    }

    /// Returns the reason-dependent field.
    #[must_use]
    pub const fn detail(&self) -> Profile2ErrorDetail {
        self.detail
    }
}

impl TlvBody<'_> for Profile2ErrorCode {
    const TLV_TYPE: TlvType = TlvType::PROFILE_2_ERROR_CODE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let reason = Profile2ErrorReason::from(reader.u8()?);
        let detail = match reason.0 {
            0x07 | 0x08 | 0x0e => Profile2ErrorDetail::Bssid(reader.mac()?),
            0x01 | 0x02 => Profile2ErrorDetail::RuleId(reader.be_u32()?),
            0x0b => Profile2ErrorDetail::Qmid(reader.be_u16()?),
            _ => Profile2ErrorDetail::None,
        };
        reader.finish()?;
        Ok(Self { reason, detail })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.reason.0);
        match self.detail {
            Profile2ErrorDetail::None => {}
            Profile2ErrorDetail::Bssid(mac) => writer.mac(mac),
            Profile2ErrorDetail::RuleId(id) => writer.be_u32(id),
            Profile2ErrorDetail::Qmid(id) => writer.be_u16(id),
        }
        Ok(writer.into_bytes())
    }
}

tlv_newtype! {
    /// BSSID, `EasyMesh` §17.2.74 Table 96; audit rows 226–227.
    /// Upstream `em_bssid_t` (`em_base.h:1667-1669`); the TLV header is
    /// `em_base.h:458-462`; sender `em_configuration.cpp:608-614`, receiver
    /// `em_ctrl.cpp:668-672`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Bssid(pub MacValue) => TlvType::BSSID;
}

/// One Agent List entry, `EasyMesh` §17.2.77 Table 99; audit rows 237–238.
/// Upstream `em_agent_list_agent_t` (`em_base.h:967-971`), sender
/// `em_configuration.cpp:3981-4006`, receiver `:5801-5826`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Agent {
    /// Agent AL MAC address.
    pub mac: MacAddr,
    /// Profile, including reserved values.
    pub profile: ProfileType,
    /// Security code, including reserved values.
    pub security: AgentSecurity,
}

/// Agent List, `EasyMesh` §17.2.77 Table 99; audit rows 237–238.
/// Upstream `em_agent_list_t` (`em_base.h:973-976`), sender
/// `em_configuration.cpp:3966-4009`, receiver `:5801-5840`.
/// Octets after the `Num_Agent` entries are trailing bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct AgentList {
    /// Agents in wire order.
    pub agents: Vec<Agent>,
}

impl TlvBody<'_> for AgentList {
    const TLV_TYPE: TlvType = TlvType::AGENT_LIST;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut agents = bounded_vec(count, reader.remaining(), 8);
        for _ in 0..count {
            agents.push(Agent {
                mac: reader.mac()?,
                profile: ProfileType::from(reader.u8()?),
                security: AgentSecurity::from(reader.u8()?),
            });
        }
        reader.finish()?;
        Ok(Self { agents })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.agents.len())?;
        for agent in &self.agents {
            writer.mac(agent.mac);
            writer.u8(u8::from(agent.profile));
            writer.u8(agent.security.0);
        }
        Ok(writer.into_bytes())
    }
}

/// Shared one-octet role layout, IEEE 1905.1 §6.4.14/16 Tables 6-22/24;
/// audit rows 329–330, 334–335. Upstream TLV header `em_base.h:458-462`, senders
/// `em_configuration.cpp:4555-4557`, `:4399-4405`; receivers only validate
/// presence and length (`em_msg.cpp:715`, `:725`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleValue {
    /// Role, including reserved values.
    pub role: Role,
}

impl RoleValue {
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let role = Role::from(reader.u8()?);
        reader.finish()?;
        Ok(Self { role })
    }
    #[allow(clippy::unnecessary_wraps)]
    fn encode_value(self) -> Result<Vec<u8>, EncodeError> {
        Ok(vec![self.role.0])
    }
}

tlv_newtype! {
    /// `SearchedRole`, IEEE 1905.1 §6.4.14 Table 6-22; audit rows 329–330.
    /// TLV header `em_base.h:458-462`, sender `em_configuration.cpp:4555-4557`;
    /// receiver `em_msg.cpp:715` validates presence and length only.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SearchedRole(pub RoleValue) => TlvType::SEARCHED_ROLE;
}

tlv_newtype! {
    /// `SupportedRole`, IEEE 1905.1 §6.4.16 Table 6-24; audit rows 334–335.
    /// TLV header `em_base.h:458-462`, sender `em_configuration.cpp:4399-4405`;
    /// receiver `em_msg.cpp:725` validates presence and length only.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SupportedRole(pub RoleValue) => TlvType::SUPPORTED_ROLE;
}

/// Shared one-octet band layout, IEEE 1905.1 §6.4.15/17 Tables 6-23/25;
/// audit rows 331–333, 336–338. Upstream enum `em_base.h:501-507`, senders
/// `em_configuration.cpp:4559-4562`, `:4407-4414`, receiver `em_msg.cpp:346-363`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrequencyBand {
    /// Band, including reserved values.
    pub band: FreqBand,
}

impl FrequencyBand {
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let band = FreqBand::from(reader.u8()?);
        reader.finish()?;
        Ok(Self { band })
    }
    #[allow(clippy::unnecessary_wraps)]
    fn encode_value(self) -> Result<Vec<u8>, EncodeError> {
        Ok(vec![u8::from(self.band)])
    }
}

tlv_newtype! {
    /// `AutoconfigFreqBand`, IEEE 1905.1 §6.4.15 Table 6-23; audit rows 331–333.
    /// Upstream enum `em_base.h:501-507`, sender `em_configuration.cpp:4559-4562`,
    /// receiver `em_msg.cpp:346-363`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AutoconfigFreqBand(pub FrequencyBand) => TlvType::AUTOCONFIG_FREQ_BAND;
}

tlv_newtype! {
    /// `SupportedFreqBand`, IEEE 1905.1 §6.4.17 Table 6-25; audit rows 336–338.
    /// Upstream enum `em_base.h:501-507`, sender `em_configuration.cpp:4407-4414`,
    /// receiver `em_msg.cpp:346-363`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SupportedFreqBand(pub FrequencyBand) => TlvType::SUPPORTED_FREQ_BAND;
}

/// WSC, IEEE 1905.1 §6.4.18 Table 6-26; audit rows 339–342.
/// Upstream attribute header `em_base.h:2256-2260`, senders
/// `em_configuration.cpp:4205-4221`, `:4308-4314`, receivers `:4593-4630`,
/// `:4636-4756`. The constructor and decoder validate the complete attribute
/// list using [`WscAttrIter`]; an empty list is allowed (note 0009 D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wsc<'a> {
    value: &'a [u8],
}

impl<'a> Wsc<'a> {
    /// Validates and borrows a WSC attribute list.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Truncated`] if an attribute header or value is incomplete.
    pub fn new(value: &'a [u8]) -> Result<Self, DecodeError> {
        for attr in WscAttrIter::new(value) {
            attr?;
        }
        Ok(Self { value })
    }

    /// Returns the validated attribute bytes.
    #[must_use]
    pub const fn value(&self) -> &'a [u8] {
        self.value
    }

    /// Walks the attributes in wire order.
    #[must_use]
    pub fn attributes(&self) -> WscAttrIter<'a> {
        WscAttrIter::new(self.value)
    }
}

impl<'a> TlvBody<'a> for Wsc<'a> {
    const TLV_TYPE: TlvType = TlvType::WSC;
    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        Self::new(value)
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        Ok(self.value.to_vec())
    }
}

/// One entry of upstream vendor TLV 0xf2 (no spec table or audit row).
/// Upstream `em_base.h:1617-1621`, sender `em_configuration.cpp:1559-1568`,
/// receiver `:1623-1639`.
/// Both two-octet fields use little-endian, matching note 0009 D6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendorBss {
    /// BSS identifier.
    pub bssid: MacAddr,
    /// Upstream haul type, in its raw 16-bit form.
    pub haul_type: u16,
    /// Upstream VAP mode, in its raw 16-bit form.
    pub vap_mode: u16,
}

/// One radio of upstream vendor TLV 0xf2 (no spec table or audit row).
/// Upstream `em_base.h:1623-1627`, sender `em_configuration.cpp:1549-1571`,
/// receiver `:1604-1648`.
/// Its BSS fields use little-endian (note 0009 D6).
#[derive(Debug, PartialEq, Eq)]
pub struct VendorRadio {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// BSS entries in wire order.
    pub bsses: Vec<VendorBss>,
}

/// Upstream `VENDOR_OPERATIONAL_BSS` (0xf2), not a standard TLV; no audit row.
/// Upstream structs `em_base.h:1617-1632`, sender `em_configuration.cpp:1532-1575`,
/// receiver `:1604-1648`. Haul type and VAP mode are little-endian per note 0009 D6,
/// matching the x86 host-order writes at the pin.
#[derive(Debug, PartialEq, Eq)]
pub struct VendorOperationalBss {
    /// Radios in wire order.
    pub radios: Vec<VendorRadio>,
}

impl TlvBody<'_> for VendorOperationalBss {
    const TLV_TYPE: TlvType = TlvType::VENDOR_OPERATIONAL_BSS;
    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut radios = bounded_vec(count, reader.remaining(), 7);
        for _ in 0..count {
            let ruid = reader.mac()?;
            let bss_count = usize::from(reader.u8()?);
            let mut bsses = bounded_vec(bss_count, reader.remaining(), 10);
            for _ in 0..bss_count {
                bsses.push(VendorBss {
                    bssid: reader.mac()?,
                    haul_type: reader.le_u16()?,
                    vap_mode: reader.le_u16()?,
                });
            }
            radios.push(VendorRadio { ruid, bsses });
        }
        reader.finish()?;
        Ok(Self { radios })
    }
    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.count_u8(radio.bsses.len())?;
            for bss in &radio.bsses {
                writer.mac(bss.bssid);
                writer.le_u16(bss.haul_type);
                writer.le_u16(bss.vap_mode);
            }
        }
        Ok(writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, truncated_in};
    use crate::tlv::Tlv;
    use core::fmt::Debug;
    use core::mem::size_of;

    const GROUP: &str = "autoconfig";
    const TOO_LONG: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    const fn truncated(needed: usize, remaining: usize) -> DecodeError {
        DecodeError::Truncated { needed, remaining }
    }
    const fn trailing(remaining: usize) -> DecodeError {
        DecodeError::TrailingBytes { remaining }
    }
    const fn field(offset: usize, width: usize) -> Field {
        Field { offset, width }
    }
    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }
    fn services(values: &[u8]) -> ServiceList {
        ServiceList {
            services: values.iter().copied().map(ServiceType::from).collect(),
        }
    }
    const fn agent() -> Agent {
        Agent {
            mac: MacAddr::new([0; 6]),
            profile: ProfileType(1),
            security: AgentSecurity::DISABLED,
        }
    }
    fn too_many_operational_radios() -> ApOperationalBss<'static> {
        ApOperationalBss {
            radios: (0..256)
                .map(|_| OperationalRadio {
                    ruid: agent().mac,
                    bsses: vec![],
                })
                .collect(),
        }
    }
    fn too_many_associated_bsses() -> AssociatedClients {
        AssociatedClients {
            bsses: (0..256)
                .map(|_| AssociatedBss {
                    bssid: agent().mac,
                    clients: vec![],
                })
                .collect(),
        }
    }
    fn too_many_vendor_radios() -> VendorOperationalBss {
        VendorOperationalBss {
            radios: (0..256)
                .map(|_| VendorRadio {
                    ruid: agent().mac,
                    bsses: vec![],
                })
                .collect(),
        }
    }
    // Each spec fixture is used by a golden or alternate check.
    const SPEC_CASES: &[Key] = &[
        ("0x80", "golden"),
        ("0x80", "agent-only"),
        ("0x80", "zero-services"),
        ("0x80", "emplus-reserved"),
        ("0x80", "emplus-ctrl-reserved"),
        ("0x80", "nine-services"),
        ("0x81", "golden"),
        ("0x81", "two-services"),
        ("0x81", "zero-services"),
        ("0x81", "reserved-ff"),
        ("0x82", "golden"),
        ("0xb8", "golden"),
        ("0x83", "golden"),
        ("0x83", "zero-radios"),
        ("0x83", "radio-zero-bss"),
        ("0x83", "ssid-empty"),
        ("0x83", "ssid-32"),
        ("0x83", "ssid-ends-nul"),
        ("0x83", "ssid-33-ends-nul"),
        ("0x84", "golden"),
        ("0x84", "zero-bss"),
        ("0x84", "bss-zero-clients"),
        ("0x84", "assoc-time-ffff"),
        ("0x84", "assoc-time-zero"),
        ("0xb3", "golden"),
        ("0xb3", "profile-1"),
        ("0xb3", "profile-2"),
        ("0xb3", "reserved-00"),
        ("0xb3", "reserved-04"),
        ("0xb3", "reserved-ff"),
        ("0xbc", "golden"),
        ("0xbc", "reason-08-bssid"),
        ("0xbc", "reason-0e-bssid"),
        ("0xbc", "reason-01-rule-id"),
        ("0xbc", "reason-02-rule-id"),
        ("0xbc", "reason-0b-qmid"),
        ("0xbc", "reason-03-only"),
        ("0xbc", "reason-0c-only"),
        ("0xbc", "reason-reserved-00"),
        ("0xbc", "reason-reserved-0f"),
        ("0xbc", "reason-reserved-ff"),
        ("0xd5", "golden"),
        ("0xd5", "one-agent"),
        ("0xd5", "zero-agents"),
        ("0xd5", "reserved-values"),
        ("0x0d", "golden"),
        ("0x0d", "reserved-01"),
        ("0x0d", "reserved-ff"),
        ("0x0e", "golden"),
        ("0x0e", "band-24ghz"),
        ("0x0e", "band-60ghz"),
        ("0x0e", "band-03-6ghz"),
        ("0x0e", "reserved-04"),
        ("0x0e", "reserved-ff"),
        ("0x0f", "golden"),
        ("0x0f", "reserved-01"),
        ("0x0f", "reserved-ff"),
        ("0x10", "golden"),
        ("0x10", "band-24ghz"),
        ("0x10", "band-60ghz"),
        ("0x10", "band-03-6ghz"),
        ("0x10", "reserved-04"),
        ("0x10", "reserved-ff"),
        ("0x11", "golden"),
        ("0x11", "message-type-only"),
        ("0x11", "zero-length-attribute"),
        ("0x11", "empty"),
        ("0xf2", "golden"),
        ("0xf2", "zero-radios"),
        ("0xf2", "radio-zero-bss"),
        ("0xf2", "raw-values"),
    ];
    const LENIENT_CASES: &[Key] = &[
        ("0xb3", "four-octet-upstream"),
        ("0xb3", "four-octet-upstream-p1"),
    ];
    const INVALID_CASES: &[(Key, DecodeError)] = &[
        (("0x80", "empty"), truncated(1, 0)),
        (("0x80", "count-overstates"), truncated(1, 0)),
        (("0x80", "trailing-bytes"), trailing(1)),
        (("0x81", "empty"), truncated(1, 0)),
        (("0x81", "count-overstates"), truncated(1, 0)),
        (("0x81", "trailing-bytes"), trailing(1)),
        (("0x82", "cut-ruid"), truncated(6, 5)),
        (("0x82", "seven-octets"), trailing(1)),
        (("0x82", "empty"), truncated(6, 0)),
        (("0xb8", "cut-bssid"), truncated(6, 5)),
        (("0xb8", "seven-octets"), trailing(1)),
        (("0xb8", "empty"), truncated(6, 0)),
        (("0x83", "empty"), truncated(1, 0)),
        (("0x83", "cut-ruid"), truncated(6, 4)),
        (("0x83", "missing-num-bss"), truncated(1, 0)),
        (("0x83", "radios-overstate"), truncated(6, 0)),
        (("0x83", "bss-overstates"), truncated(6, 0)),
        (("0x83", "cut-ap-mac"), truncated(6, 3)),
        (("0x83", "missing-ssid-length"), truncated(1, 0)),
        (("0x83", "ssid-length-overstates"), truncated(8, 4)),
        (("0x83", "trailing-bytes"), trailing(1)),
        (("0x84", "empty"), truncated(1, 0)),
        (("0x84", "bss-overstates"), truncated(6, 0)),
        (("0x84", "cut-ap-mac"), truncated(6, 4)),
        (("0x84", "cut-num-client"), truncated(2, 1)),
        (("0x84", "clients-overstate-be"), truncated(6, 0)),
        (("0x84", "clients-overstate"), truncated(6, 0)),
        (("0x84", "cut-assoc-time"), truncated(2, 1)),
        (("0x84", "missing-assoc-time"), truncated(2, 0)),
        (("0x84", "trailing-bytes"), trailing(1)),
        (("0xb3", "empty"), truncated(1, 0)),
        (("0xb3", "two-octets"), trailing(1)),
        (("0xb3", "three-octets"), trailing(2)),
        (("0xb3", "five-octets"), trailing(4)),
        (("0xbc", "empty"), truncated(1, 0)),
        (("0xbc", "reason-07-missing-bssid"), truncated(6, 0)),
        (("0xbc", "reason-07-cut-bssid"), truncated(6, 3)),
        (("0xbc", "reason-01-missing-rule-id"), truncated(4, 0)),
        (("0xbc", "reason-02-cut-rule-id"), truncated(4, 3)),
        (("0xbc", "reason-0b-cut-qmid"), truncated(2, 1)),
        (("0xbc", "reason-03-trailing"), trailing(1)),
        (("0xbc", "reason-07-trailing"), trailing(1)),
        (("0xbc", "reason-0b-trailing"), trailing(1)),
        (("0xbc", "reason-reserved-trailing"), trailing(1)),
        (("0xd5", "empty"), truncated(1, 0)),
        (("0xd5", "count-overstates"), truncated(6, 0)),
        (("0xd5", "partial-agent"), trailing(7)),
        (("0xd5", "trailing-bytes"), trailing(1)),
        (("0x0d", "empty"), truncated(1, 0)),
        (("0x0d", "two-octets"), trailing(1)),
        (("0x0e", "empty"), truncated(1, 0)),
        (("0x0e", "two-octets"), trailing(1)),
        (("0x0f", "empty"), truncated(1, 0)),
        (("0x0f", "two-octets"), trailing(1)),
        (("0x10", "empty"), truncated(1, 0)),
        (("0x10", "two-octets"), trailing(1)),
        (("0x11", "attr-length-overstates"), truncated(5, 1)),
        (("0xf2", "empty"), truncated(1, 0)),
        (("0xf2", "cut-ruid"), truncated(6, 4)),
        (("0xf2", "radios-overstate"), truncated(6, 0)),
        (("0xf2", "bss-overstates"), truncated(6, 0)),
        (("0xf2", "cut-haul-type"), truncated(2, 1)),
        (("0xf2", "missing-vap-mode"), truncated(2, 0)),
        (("0xf2", "trailing-bytes"), trailing(1)),
    ];

    body_checks! {
        mod supported_service_checks;
        body: SupportedService;
        golden: &fixture(("0x80", "golden")), SupportedService(services(&[0x00, 0x01]));
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 1), field(2, 1)]);
        trailing_byte: &fixture(("0x80", "golden"));
        count_too_large: SupportedService(services(&[0; 256])), TOO_LONG;
        alternates: [
            (&fixture(("0x80", "agent-only")), SupportedService(services(&[0x01])), &fixture(("0x80", "agent-only"))),
            (&fixture(("0x80", "zero-services")), SupportedService(services(&[])), &fixture(("0x80", "zero-services"))),
            (&fixture(("0x80", "emplus-reserved")), SupportedService(services(&[0x01, 0xa1])), &fixture(("0x80", "emplus-reserved"))),
            (&fixture(("0x80", "emplus-ctrl-reserved")), SupportedService(services(&[0x00, 0xa0])), &fixture(("0x80", "emplus-ctrl-reserved"))),
            (&fixture(("0x80", "nine-services")), SupportedService(services(&[0x00, 0x01, 0x02, 0x03, 0x7f, 0x80, 0xa0, 0xa1, 0xff])), &fixture(("0x80", "nine-services"))),
        ];
    }

    body_checks! {
        mod searched_service_checks;
        body: SearchedService;
        golden: &fixture(("0x81", "golden")), SearchedService(services(&[0x00]));
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 1)]);
        trailing_byte: &fixture(("0x81", "golden"));
        count_too_large: SearchedService(services(&[0; 256])), TOO_LONG;
        alternates: [
            (&fixture(("0x81", "two-services")), SearchedService(services(&[0x00, 0x01])), &fixture(("0x81", "two-services"))),
            (&fixture(("0x81", "zero-services")), SearchedService(services(&[])), &fixture(("0x81", "zero-services"))),
            (&fixture(("0x81", "reserved-ff")), SearchedService(services(&[0xff])), &fixture(("0x81", "reserved-ff"))),
        ];
    }

    body_checks! {
        mod ap_radio_checks;
        body: ApRadioIdentifier;
        golden: &fixture(("0x82", "golden")), ApRadioIdentifier(MacValue { mac: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]) });
        prefixes: |_| false, truncated_in(&[field(0, 6)]);
        trailing_byte: &fixture(("0x82", "golden"));
    }

    body_checks! {
        mod bssid_checks;
        body: Bssid;
        golden: &fixture(("0xb8", "golden")), Bssid(MacValue { mac: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]) });
        prefixes: |_| false, truncated_in(&[field(0, 6)]);
        trailing_byte: &fixture(("0xb8", "golden"));
    }

    body_checks! {
        mod operational_bss_checks;
        body: ApOperationalBss<'a>;
        golden: &fixture(("0x83", "golden")), ApOperationalBss { radios: vec![OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![OperationalBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]), ssid: &[0x48, 0x6f, 0x6d, 0x65, 0x2d, 0x35, 0x47] }, OperationalBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x52]), ssid: &[0x62, 0x68, 0x2d, 0x61, 0x63, 0x31] }] }, OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf0]), bsses: vec![OperationalBss { bssid: MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]), ssid: &[0x48, 0x6f, 0x6d, 0x65, 0x2d, 0x32, 0x47] }] }] };
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1), field(8, 6), field(14, 1), field(15, 7), field(22, 6), field(28, 1), field(29, 6), field(35, 6), field(41, 1), field(42, 6), field(48, 1), field(49, 7)]);
        trailing_byte: &fixture(("0x83", "golden"));
        count_too_large: too_many_operational_radios(), TOO_LONG;
        alternates: [
            (&fixture(("0x83", "zero-radios")), ApOperationalBss { radios: vec![] }, &fixture(("0x83", "zero-radios"))),
            (&fixture(("0x83", "radio-zero-bss")), ApOperationalBss { radios: vec![OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![] }, OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf0]), bsses: vec![OperationalBss { bssid: MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]), ssid: &[0x48, 0x6f, 0x6d, 0x65, 0x2d, 0x32, 0x47] }] }] }, &fixture(("0x83", "radio-zero-bss"))),
            (&fixture(("0x83", "ssid-empty")), ApOperationalBss { radios: vec![OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![OperationalBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]), ssid: &[] }] }] }, &fixture(("0x83", "ssid-empty"))),
            (&fixture(("0x83", "ssid-32")), ApOperationalBss { radios: vec![OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![OperationalBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]), ssid: &[0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35] }] }] }, &fixture(("0x83", "ssid-32"))),
            (&fixture(("0x83", "ssid-ends-nul")), ApOperationalBss { radios: vec![OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![OperationalBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]), ssid: &[0x48, 0x6f, 0x6d, 0x65, 0x00] }] }] }, &fixture(("0x83", "ssid-ends-nul"))),
            (&fixture(("0x83", "ssid-33-ends-nul")), ApOperationalBss { radios: vec![OperationalRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![OperationalBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]), ssid: &[0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x00] }] }] }, &fixture(("0x83", "ssid-33-ends-nul"))),
        ];
    }

    body_checks! {
        mod associated_clients_checks;
        body: AssociatedClients;
        golden: &fixture(("0x84", "golden")), AssociatedClients { bsses: vec![AssociatedBss { bssid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]), clients: vec![AssociatedClient { mac: MacAddr::new([0x5c, 0x1a, 0x2b, 0x3c, 0x4d, 0x01]), association_time: 4660 }, AssociatedClient { mac: MacAddr::new([0x5c, 0x1a, 0x2b, 0x3c, 0x4d, 0x02]), association_time: 258 }] }, AssociatedBss { bssid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe2]), clients: vec![AssociatedClient { mac: MacAddr::new([0x5c, 0x1a, 0x2b, 0x3c, 0x4d, 0x03]), association_time: 65534 }] }] };
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 2), field(9, 6), field(15, 2), field(17, 6), field(23, 2), field(25, 6), field(31, 2), field(33, 6), field(39, 2)]);
        trailing_byte: &fixture(("0x84", "golden"));
        count_too_large: too_many_associated_bsses(), TOO_LONG;
        alternates: [
            (&fixture(("0x84", "zero-bss")), AssociatedClients { bsses: vec![] }, &fixture(("0x84", "zero-bss"))),
            (&fixture(("0x84", "bss-zero-clients")), AssociatedClients { bsses: vec![AssociatedBss { bssid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]), clients: vec![] }, AssociatedBss { bssid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe2]), clients: vec![AssociatedClient { mac: MacAddr::new([0x5c, 0x1a, 0x2b, 0x3c, 0x4d, 0x03]), association_time: 4660 }] }] }, &fixture(("0x84", "bss-zero-clients"))),
            (&fixture(("0x84", "assoc-time-ffff")), AssociatedClients { bsses: vec![AssociatedBss { bssid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]), clients: vec![AssociatedClient { mac: MacAddr::new([0x5c, 0x1a, 0x2b, 0x3c, 0x4d, 0x01]), association_time: 65535 }] }] }, &fixture(("0x84", "assoc-time-ffff"))),
            (&fixture(("0x84", "assoc-time-zero")), AssociatedClients { bsses: vec![AssociatedBss { bssid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]), clients: vec![AssociatedClient { mac: MacAddr::new([0x5c, 0x1a, 0x2b, 0x3c, 0x4d, 0x01]), association_time: 0 }] }] }, &fixture(("0x84", "assoc-time-zero"))),
        ];
    }

    body_checks! {
        mod multi_ap_profile_checks;
        body: MultiApProfile;
        golden: &fixture(("0xb3", "golden")), MultiApProfile { profile: ProfileType(0x03) };
        lenient;
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0xb3", "golden"));
        alternates: [
            (&fixture(("0xb3", "profile-1")), MultiApProfile { profile: ProfileType(0x01) }, &fixture(("0xb3", "profile-1"))),
            (&fixture(("0xb3", "profile-2")), MultiApProfile { profile: ProfileType(0x02) }, &fixture(("0xb3", "profile-2"))),
            (&fixture(("0xb3", "reserved-00")), MultiApProfile { profile: ProfileType(0x00) }, &fixture(("0xb3", "reserved-00"))),
            (&fixture(("0xb3", "reserved-04")), MultiApProfile { profile: ProfileType(0x04) }, &fixture(("0xb3", "reserved-04"))),
            (&fixture(("0xb3", "reserved-ff")), MultiApProfile { profile: ProfileType(0xff) }, &fixture(("0xb3", "reserved-ff"))),
        ];
        lenient: [
            (&fixture(("0xb3", "four-octet-upstream")), MultiApProfile { profile: ProfileType(0x03) }, &fixture(("0xb3", "golden"))),
            (&fixture(("0xb3", "four-octet-upstream-p1")), MultiApProfile { profile: ProfileType(0x01) }, &fixture(("0xb3", "profile-1"))),
        ];
    }

    body_checks! {
        mod profile_2_error_checks;
        body: Profile2ErrorCode;
        golden: &fixture(("0xbc", "golden")), Profile2ErrorCode::new(Profile2ErrorReason(0x07), Profile2ErrorDetail::Bssid(MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe7]))).unwrap();
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6)]);
        trailing_byte: &fixture(("0xbc", "golden"));
        alternates: [
            (&fixture(("0xbc", "reason-08-bssid")), Profile2ErrorCode::new(Profile2ErrorReason(0x08), Profile2ErrorDetail::Bssid(MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe7]))).unwrap(), &fixture(("0xbc", "reason-08-bssid"))),
            (&fixture(("0xbc", "reason-0e-bssid")), Profile2ErrorCode::new(Profile2ErrorReason(0x0e), Profile2ErrorDetail::Bssid(MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe7]))).unwrap(), &fixture(("0xbc", "reason-0e-bssid"))),
            (&fixture(("0xbc", "reason-01-rule-id")), Profile2ErrorCode::new(Profile2ErrorReason(0x01), Profile2ErrorDetail::RuleId(0x0a0b_0c0d)).unwrap(), &fixture(("0xbc", "reason-01-rule-id"))),
            (&fixture(("0xbc", "reason-02-rule-id")), Profile2ErrorCode::new(Profile2ErrorReason(0x02), Profile2ErrorDetail::RuleId(0x1234_5678)).unwrap(), &fixture(("0xbc", "reason-02-rule-id"))),
            (&fixture(("0xbc", "reason-0b-qmid")), Profile2ErrorCode::new(Profile2ErrorReason(0x0b), Profile2ErrorDetail::Qmid(0x1a2b)).unwrap(), &fixture(("0xbc", "reason-0b-qmid"))),
            (&fixture(("0xbc", "reason-03-only")), Profile2ErrorCode::new(Profile2ErrorReason(0x03), Profile2ErrorDetail::None).unwrap(), &fixture(("0xbc", "reason-03-only"))),
            (&fixture(("0xbc", "reason-0c-only")), Profile2ErrorCode::new(Profile2ErrorReason(0x0c), Profile2ErrorDetail::None).unwrap(), &fixture(("0xbc", "reason-0c-only"))),
            (&fixture(("0xbc", "reason-reserved-00")), Profile2ErrorCode::new(Profile2ErrorReason(0x00), Profile2ErrorDetail::None).unwrap(), &fixture(("0xbc", "reason-reserved-00"))),
            (&fixture(("0xbc", "reason-reserved-0f")), Profile2ErrorCode::new(Profile2ErrorReason(0x0f), Profile2ErrorDetail::None).unwrap(), &fixture(("0xbc", "reason-reserved-0f"))),
            (&fixture(("0xbc", "reason-reserved-ff")), Profile2ErrorCode::new(Profile2ErrorReason(0xff), Profile2ErrorDetail::None).unwrap(), &fixture(("0xbc", "reason-reserved-ff"))),
        ];
    }

    body_checks! {
        mod agent_list_checks;
        body: AgentList;
        golden: &fixture(("0xd5", "golden")), AgentList { agents: vec![Agent { mac: MacAddr::new([0x02, 0x66, 0x77, 0x88, 0x99, 0xaa]), profile: ProfileType(0x03), security: AgentSecurity(0x01) }, Agent { mac: MacAddr::new([0x02, 0xbb, 0xcc, 0xdd, 0xee, 0xff]), profile: ProfileType(0x02), security: AgentSecurity(0x00) }] };
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1), field(8, 1), field(9, 6), field(15, 1), field(16, 1)]);
        trailing_byte: &fixture(("0xd5", "golden"));
        count_too_large: AgentList { agents: vec![agent(); 256] }, TOO_LONG;
        alternates: [
            (&fixture(("0xd5", "one-agent")), AgentList { agents: vec![Agent { mac: MacAddr::new([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]), profile: ProfileType(0x01), security: AgentSecurity(0x00) }] }, &fixture(("0xd5", "one-agent"))),
            (&fixture(("0xd5", "zero-agents")), AgentList { agents: vec![] }, &fixture(("0xd5", "zero-agents"))),
            (&fixture(("0xd5", "reserved-values")), AgentList { agents: vec![Agent { mac: MacAddr::new([0x02, 0x66, 0x77, 0x88, 0x99, 0xaa]), profile: ProfileType(0x00), security: AgentSecurity(0x02) }, Agent { mac: MacAddr::new([0x02, 0xbb, 0xcc, 0xdd, 0xee, 0xff]), profile: ProfileType(0x04), security: AgentSecurity(0xff) }] }, &fixture(("0xd5", "reserved-values"))),
        ];
    }

    body_checks! {
        mod searched_role_checks;
        body: SearchedRole;
        golden: &fixture(("0x0d", "golden")), SearchedRole(RoleValue { role: Role(0x00) });
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0x0d", "golden"));
        alternates: [
            (&fixture(("0x0d", "reserved-01")), SearchedRole(RoleValue { role: Role(0x01) }), &fixture(("0x0d", "reserved-01"))),
            (&fixture(("0x0d", "reserved-ff")), SearchedRole(RoleValue { role: Role(0xff) }), &fixture(("0x0d", "reserved-ff"))),
        ];
    }

    body_checks! {
        mod autoconfig_band_checks;
        body: AutoconfigFreqBand;
        golden: &fixture(("0x0e", "golden")), AutoconfigFreqBand(FrequencyBand { band: FreqBand(0x01) });
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0x0e", "golden"));
        alternates: [
            (&fixture(("0x0e", "band-24ghz")), AutoconfigFreqBand(FrequencyBand { band: FreqBand(0x00) }), &fixture(("0x0e", "band-24ghz"))),
            (&fixture(("0x0e", "band-60ghz")), AutoconfigFreqBand(FrequencyBand { band: FreqBand(0x02) }), &fixture(("0x0e", "band-60ghz"))),
            (&fixture(("0x0e", "band-03-6ghz")), AutoconfigFreqBand(FrequencyBand { band: FreqBand(0x03) }), &fixture(("0x0e", "band-03-6ghz"))),
            (&fixture(("0x0e", "reserved-04")), AutoconfigFreqBand(FrequencyBand { band: FreqBand(0x04) }), &fixture(("0x0e", "reserved-04"))),
            (&fixture(("0x0e", "reserved-ff")), AutoconfigFreqBand(FrequencyBand { band: FreqBand(0xff) }), &fixture(("0x0e", "reserved-ff"))),
        ];
    }

    body_checks! {
        mod supported_role_checks;
        body: SupportedRole;
        golden: &fixture(("0x0f", "golden")), SupportedRole(RoleValue { role: Role(0x00) });
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0x0f", "golden"));
        alternates: [
            (&fixture(("0x0f", "reserved-01")), SupportedRole(RoleValue { role: Role(0x01) }), &fixture(("0x0f", "reserved-01"))),
            (&fixture(("0x0f", "reserved-ff")), SupportedRole(RoleValue { role: Role(0xff) }), &fixture(("0x0f", "reserved-ff"))),
        ];
    }

    body_checks! {
        mod supported_band_checks;
        body: SupportedFreqBand;
        golden: &fixture(("0x10", "golden")), SupportedFreqBand(FrequencyBand { band: FreqBand(0x01) });
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0x10", "golden"));
        alternates: [
            (&fixture(("0x10", "band-24ghz")), SupportedFreqBand(FrequencyBand { band: FreqBand(0x00) }), &fixture(("0x10", "band-24ghz"))),
            (&fixture(("0x10", "band-60ghz")), SupportedFreqBand(FrequencyBand { band: FreqBand(0x02) }), &fixture(("0x10", "band-60ghz"))),
            (&fixture(("0x10", "band-03-6ghz")), SupportedFreqBand(FrequencyBand { band: FreqBand(0x03) }), &fixture(("0x10", "band-03-6ghz"))),
            (&fixture(("0x10", "reserved-04")), SupportedFreqBand(FrequencyBand { band: FreqBand(0x04) }), &fixture(("0x10", "reserved-04"))),
            (&fixture(("0x10", "reserved-ff")), SupportedFreqBand(FrequencyBand { band: FreqBand(0xff) }), &fixture(("0x10", "reserved-ff"))),
        ];
    }

    body_checks! {
        mod wsc_checks;
        body: Wsc<'a>;
        golden: &fixture(("0x11", "golden")), Wsc::new(&[0x10, 0x4a, 0x00, 0x01, 0x10, 0x10, 0x22, 0x00, 0x01, 0x04, 0x10, 0x47, 0x00, 0x10, 0x0f, 0x1e, 0x2d, 0x3c, 0x4b, 0x5a, 0x69, 0x78, 0x87, 0x96, 0xa5, 0xb4, 0xc3, 0xd2, 0xe1, 0xf0]).unwrap();
        prefixes: |len| matches!(len, 0 | 5 | 10), truncated_in(&[field(0, 2), field(2, 2), field(4, 1), field(5, 2), field(7, 2), field(9, 1), field(10, 2), field(12, 2), field(14, 16)]);
        alternates: [
            (&fixture(("0x11", "message-type-only")), Wsc::new(&[0x10, 0x22, 0x00, 0x01, 0x05]).unwrap(), &fixture(("0x11", "message-type-only"))),
            (&fixture(("0x11", "zero-length-attribute")), Wsc::new(&[0x10, 0x22, 0x00, 0x01, 0x04, 0x10, 0x49, 0x00, 0x00]).unwrap(), &fixture(("0x11", "zero-length-attribute"))),
            (&fixture(("0x11", "empty")), Wsc::new(&[]).unwrap(), &fixture(("0x11", "empty"))),
        ];
    }

    body_checks! {
        mod vendor_bss_checks;
        body: VendorOperationalBss;
        golden: &fixture(("0xf2", "golden")), VendorOperationalBss { radios: vec![VendorRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![VendorBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]), haul_type: 0x0000, vap_mode: 0x0000 }, VendorBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x52]), haul_type: 0x0001, vap_mode: 0x0000 }] }, VendorRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf0]), bsses: vec![VendorBss { bssid: MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]), haul_type: 0x0001, vap_mode: 0x0001 }] }] };
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1), field(8, 6), field(14, 2), field(16, 2), field(18, 6), field(24, 2), field(26, 2), field(28, 6), field(34, 1), field(35, 6), field(41, 2), field(43, 2)]);
        trailing_byte: &fixture(("0xf2", "golden"));
        count_too_large: too_many_vendor_radios(), TOO_LONG;
        alternates: [
            (&fixture(("0xf2", "zero-radios")), VendorOperationalBss { radios: vec![] }, &fixture(("0xf2", "zero-radios"))),
            (&fixture(("0xf2", "radio-zero-bss")), VendorOperationalBss { radios: vec![VendorRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![] }, VendorRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf0]), bsses: vec![VendorBss { bssid: MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]), haul_type: 0x0000, vap_mode: 0x0000 }] }] }, &fixture(("0xf2", "radio-zero-bss"))),
            (&fixture(("0xf2", "raw-values")), VendorOperationalBss { radios: vec![VendorRadio { ruid: MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]), bsses: vec![VendorBss { bssid: MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]), haul_type: 0x0102, vap_mode: 0xa1b2 }] }] }, &fixture(("0xf2", "raw-values"))),
        ];
    }

    fn check_invalid<'a, B: TlvBody<'a> + Debug>(value: &'a [u8], expected: DecodeError, key: Key) {
        crate::body_testkit::assert_passes(crate::body_testkit::check_tlv_column::<B>(key.0));
        assert_eq!(
            B::decode_value(value).err(),
            Some(expected),
            "direct decode of {key:?}"
        );
        let tlv = Tlv {
            tlv_type: B::TLV_TYPE,
            value,
        };
        assert_eq!(
            tlv.decode::<B>().err(),
            Some(expected),
            "TLV decode of {key:?}"
        );
    }

    #[test]
    fn every_invalid_fixture_has_its_exact_error_on_both_paths() {
        for &(key, expected) in INVALID_CASES {
            let fields = body_fixtures::fields(GROUP, key);
            assert!(
                fields.split(';').any(|field| field == "kind=invalid"),
                "{key:?}"
            );
            let kind = match expected {
                DecodeError::Truncated { .. } => "error=Truncated",
                DecodeError::TrailingBytes { .. } => "error=TrailingBytes",
                _ => panic!("unexpected error for {key:?}"),
            };
            assert!(
                fields.split(';').any(|field| field == kind),
                "{key:?}: {fields}"
            );
            let value = fixture(key);
            match key.0 {
                "0x80" => check_invalid::<SupportedService>(&value, expected, key),
                "0x81" => check_invalid::<SearchedService>(&value, expected, key),
                "0x82" => check_invalid::<ApRadioIdentifier>(&value, expected, key),
                "0xb8" => check_invalid::<Bssid>(&value, expected, key),
                "0x83" => check_invalid::<ApOperationalBss<'_>>(&value, expected, key),
                "0x84" => check_invalid::<AssociatedClients>(&value, expected, key),
                "0xb3" => check_invalid::<MultiApProfile>(&value, expected, key),
                "0xbc" => check_invalid::<Profile2ErrorCode>(&value, expected, key),
                "0xd5" => check_invalid::<AgentList>(&value, expected, key),
                "0x0d" => check_invalid::<SearchedRole>(&value, expected, key),
                "0x0e" => check_invalid::<AutoconfigFreqBand>(&value, expected, key),
                "0x0f" => check_invalid::<SupportedRole>(&value, expected, key),
                "0x10" => check_invalid::<SupportedFreqBand>(&value, expected, key),
                "0x11" => check_invalid::<Wsc<'_>>(&value, expected, key),
                "0xf2" => check_invalid::<VendorOperationalBss>(&value, expected, key),
                _ => panic!("unknown fixture TLV {key:?}"),
            }
        }
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        use crate::body_testkit::{sweep_row, sweep_row_lenient};
        match row.tlv {
            "0xb3" => sweep_row_lenient::<MultiApProfile>(row, bytes, canonical, &[]),
            "0x80" => sweep_row::<SupportedService>(row, bytes, canonical),
            "0x81" => sweep_row::<SearchedService>(row, bytes, canonical),
            "0x82" => sweep_row::<ApRadioIdentifier>(row, bytes, canonical),
            "0xb8" => sweep_row::<Bssid>(row, bytes, canonical),
            "0x83" => sweep_row::<ApOperationalBss<'_>>(row, bytes, canonical),
            "0x84" => sweep_row::<AssociatedClients>(row, bytes, canonical),
            "0xbc" => sweep_row::<Profile2ErrorCode>(row, bytes, canonical),
            "0xd5" => sweep_row::<AgentList>(row, bytes, canonical),
            "0x0d" => sweep_row::<SearchedRole>(row, bytes, canonical),
            "0x0e" => sweep_row::<AutoconfigFreqBand>(row, bytes, canonical),
            "0x0f" => sweep_row::<SupportedRole>(row, bytes, canonical),
            "0x10" => sweep_row::<SupportedFreqBand>(row, bytes, canonical),
            "0x11" => sweep_row::<Wsc<'_>>(row, bytes, canonical),
            "0xf2" => sweep_row::<VendorOperationalBss>(row, bytes, canonical),
            other => Err(format!("no body for TLV {other}")),
        }
    }

    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, &[], dispatch_row);
    }

    #[test]
    fn every_fixture_row_is_used() {
        let used: Vec<Key> = SPEC_CASES
            .iter()
            .copied()
            .chain(LENIENT_CASES.iter().copied())
            .chain(INVALID_CASES.iter().map(|&(key, _)| key))
            .collect();
        body_fixtures::assert_all_used(GROUP, &used);
        for &key in SPEC_CASES {
            assert!(
                !body_fixtures::fields(GROUP, key).contains("kind="),
                "{key:?}"
            );
        }
        for &key in LENIENT_CASES {
            assert!(
                body_fixtures::fields(GROUP, key).contains("kind=lenient"),
                "{key:?}"
            );
        }
    }

    #[test]
    fn conditional_error_fields_match_every_reason() {
        let details = [
            Profile2ErrorDetail::None,
            Profile2ErrorDetail::Bssid(agent().mac),
            Profile2ErrorDetail::RuleId(0x0102_0304),
            Profile2ErrorDetail::Qmid(0x0102),
        ];
        for reason in 0..=u8::MAX {
            for detail in details {
                let expected = match reason {
                    0x01 | 0x02 => matches!(detail, Profile2ErrorDetail::RuleId(_)),
                    0x07 | 0x08 | 0x0e => matches!(detail, Profile2ErrorDetail::Bssid(_)),
                    0x0b => matches!(detail, Profile2ErrorDetail::Qmid(_)),
                    _ => matches!(detail, Profile2ErrorDetail::None),
                };
                let body = Profile2ErrorCode::new(Profile2ErrorReason(reason), detail);
                assert_eq!(body.is_some(), expected, "reason {reason:02x}, {detail:?}");
                if let Some(body) = body {
                    assert_eq!(body.reason(), Profile2ErrorReason(reason));
                    assert_eq!(body.detail(), detail);
                    let value = body.encode_value().unwrap();
                    assert_eq!(Profile2ErrorCode::decode_value(&value), Ok(body));
                }
            }
        }
    }

    #[test]
    fn lenient_profile_ignores_all_three_extra_octets_only_in_four_byte_form() {
        let expected = MultiApProfile {
            profile: ProfileType(0xff),
        };
        assert_eq!(
            MultiApProfile::decode_value_lenient(&[0xff, 0xa1, 0xb2, 0xc3]),
            Ok(Some(expected))
        );
        for (value, error) in [
            (&[][..], truncated(1, 0)),
            (&[1, 2][..], trailing(1)),
            (&[1, 2, 3][..], trailing(2)),
            (&[1, 2, 3, 4, 5][..], trailing(4)),
        ] {
            assert_eq!(MultiApProfile::decode_value_lenient(value), Err(error));
        }
    }

    #[test]
    fn wsc_validation_borrows_bytes_and_checks_every_attribute() {
        let value = fixture(("0x11", "golden"));
        let body = Wsc::new(&value).unwrap();
        assert!(core::ptr::eq(body.value().as_ptr(), value.as_ptr()));
        let attrs: Vec<_> = body.attributes().collect();
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0].as_ref().unwrap().id, 0x104a);
        assert_eq!(attrs[2].as_ref().unwrap().value.len(), 16);
        assert_eq!(Wsc::new(&[0x10]), Err(truncated(2, 1)));
        assert_eq!(Wsc::new(&[0x10, 0x22, 0, 1]), Err(truncated(1, 0)));
        assert_eq!(Wsc::new(&[0x10, 0x22, 0, 0, 0xa5]), Err(truncated(2, 1)));
        assert_eq!(Wsc::new(&[]).unwrap().encode_value(), Ok(vec![]));
    }

    #[test]
    fn nested_counts_and_ssid_lengths_cannot_be_cut_on_encode() {
        let mac = agent().mac;
        let bss = OperationalBss {
            bssid: mac,
            ssid: &[],
        };
        let body = ApOperationalBss {
            radios: vec![OperationalRadio {
                ruid: mac,
                bsses: vec![bss; 256],
            }],
        };
        assert_eq!(body.encode_value(), Err(TOO_LONG));
        let body = ApOperationalBss {
            radios: vec![OperationalRadio {
                ruid: mac,
                bsses: vec![OperationalBss {
                    bssid: mac,
                    ssid: &[0; 256],
                }],
            }],
        };
        assert_eq!(body.encode_value(), Err(TOO_LONG));
        let client = AssociatedClient {
            mac,
            association_time: 0,
        };
        let body = AssociatedClients {
            bsses: vec![AssociatedBss {
                bssid: mac,
                clients: vec![client; 65536],
            }],
        };
        assert_eq!(
            body.encode_value(),
            Err(EncodeError::CountTooLarge {
                len: 65536,
                max: 65535
            })
        );
        let bss = VendorBss {
            bssid: mac,
            haul_type: 0,
            vap_mode: 0,
        };
        let body = VendorOperationalBss {
            radios: vec![VendorRadio {
                ruid: mac,
                bsses: vec![bss; 256],
            }],
        };
        assert_eq!(body.encode_value(), Err(TOO_LONG));
    }

    #[test]
    fn largest_wire_counts_encode_and_decode_without_truncation() {
        let body = SupportedService(services(&[0xff; 255]));
        let value = body.encode_value().unwrap();
        assert_eq!(value[0], 255);
        assert_eq!(SupportedService::decode_value(&value), Ok(body));
        let client = AssociatedClient {
            mac: agent().mac,
            association_time: 0xffff,
        };
        let body = AssociatedClients {
            bsses: vec![AssociatedBss {
                bssid: agent().mac,
                clients: vec![client; 65535],
            }],
        };
        let value = body.encode_value().unwrap();
        assert_eq!(&value[7..9], &[0xff, 0xff]);
        assert_eq!(AssociatedClients::decode_value(&value), Ok(body));
    }

    #[test]
    fn maximal_claims_in_short_values_are_truncated() {
        assert_eq!(SupportedService::decode_value(&[255]), Err(truncated(1, 0)));
        assert_eq!(ApOperationalBss::decode_value(&[255]), Err(truncated(6, 0)));
        assert_eq!(
            AssociatedClients::decode_value(&[1, 0, 0, 0, 0, 0, 0, 0xff, 0xff]),
            Err(truncated(6, 0))
        );
        assert_eq!(AgentList::decode_value(&[255]), Err(truncated(6, 0)));
        assert_eq!(
            VendorOperationalBss::decode_value(&[255]),
            Err(truncated(6, 0))
        );
    }

    #[test]
    fn operational_ssid_borrows_the_tlv_value() {
        let value = fixture(("0x83", "golden"));
        let body = ApOperationalBss::decode_value(&value).unwrap();
        assert!(core::ptr::eq(
            body.radios[0].bsses[0].ssid.as_ptr(),
            value[15..].as_ptr()
        ));
    }

    #[test]
    fn body_and_record_sizes_have_bounded_stack_cost() {
        let word = size_of::<usize>();
        assert!(size_of::<Role>() <= 1);
        assert!(size_of::<AgentSecurity>() <= 1);
        assert!(size_of::<Profile2ErrorReason>() <= 1);
        assert!(size_of::<ServiceList>() <= 3 * word);
        assert!(size_of::<SupportedService>() <= 3 * word);
        assert!(size_of::<SearchedService>() <= 3 * word);
        assert!(size_of::<MacValue>() <= 6);
        assert!(size_of::<ApRadioIdentifier>() <= 6);
        assert!(size_of::<OperationalBss<'_>>() <= 4 * word);
        assert!(size_of::<OperationalRadio<'_>>() <= 5 * word);
        assert!(size_of::<ApOperationalBss<'_>>() <= 3 * word);
        assert!(size_of::<AssociatedClient>() <= 8);
        assert!(size_of::<AssociatedBss>() <= 5 * word);
        assert!(size_of::<AssociatedClients>() <= 3 * word);
        assert!(size_of::<MultiApProfile>() <= 1);
        assert!(size_of::<Profile2ErrorDetail>() <= 8);
        assert!(size_of::<Profile2ErrorCode>() <= 12);
        assert!(size_of::<Bssid>() <= 6);
        assert!(size_of::<Agent>() <= 8);
        assert!(size_of::<AgentList>() <= 3 * word);
        assert!(size_of::<RoleValue>() <= 1);
        assert!(size_of::<SearchedRole>() <= 1);
        assert!(size_of::<SupportedRole>() <= 1);
        assert!(size_of::<FrequencyBand>() <= 1);
        assert!(size_of::<AutoconfigFreqBand>() <= 1);
        assert!(size_of::<SupportedFreqBand>() <= 1);
        assert!(size_of::<Wsc<'_>>() <= 2 * word);
        assert!(size_of::<VendorBss>() <= 10);
        assert!(size_of::<VendorRadio>() <= 5 * word);
        assert!(size_of::<VendorOperationalBss>() <= 3 * word);
    }
}
