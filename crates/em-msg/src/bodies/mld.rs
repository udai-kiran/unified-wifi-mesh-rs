// SPDX-License-Identifier: Apache-2.0

//! BSS configuration and Wi-Fi 7 MLD bodies (note 0009, B10): the BSS Configuration Report,
//! Request and Response, the AP MLD, bSTA MLD and associated STA MLD configurations, the MLD
//! Structure, the affiliated STA and AP metrics, the TID-to-Link Mapping Policy and the EHT
//! Operations.
//!
//! All multi-octet integers are big-endian (§3.1.2), except the TID-to-Link Mapping Policy
//! `Num_Mapping`, which upstream writes in host order and is little-endian here (audit row 19,
//! note 0008 D6). Reserved bits, octets and blocks are ignored on decode and written as zero
//! (note 0009, D2). A "valid" flag never changes the layout: the field it qualifies is always
//! on the wire and is kept as found, and so is every reserved value. Zero counts are accepted
//! wherever the spec does not say "one or more".

use em_types::{MacAddr, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// `BackHaul BSS` bit of a BSS Configuration Report flags octet; 1 means "not in use".
const BACKHAUL_NOT_IN_USE: u8 = 0x80;
/// `Fronthaul BSS` bit of a BSS Configuration Report flags octet; 1 means "not in use".
const FRONTHAUL_NOT_IN_USE: u8 = 0x40;
/// `R1 disallowed status` bit of a BSS Configuration Report flags octet.
const R1_DISALLOWED: u8 = 0x20;
/// `R2 disallowed status` bit of a BSS Configuration Report flags octet.
const R2_DISALLOWED: u8 = 0x10;
/// `Multiple BSSID` bit of a BSS Configuration Report flags octet.
const MULTIPLE_BSSID: u8 = 0x08;
/// `Transmitted BSSID` bit of a BSS Configuration Report flags octet.
const TRANSMITTED_BSSID: u8 = 0x04;

/// `STR` bit of an MLD modes octet.
const MODE_STR: u8 = 0x80;
/// `NSTR` bit of an MLD modes octet.
const MODE_NSTR: u8 = 0x40;
/// `EMLSR` bit of an MLD modes octet.
const MODE_EMLSR: u8 = 0x20;
/// `EMLMR` bit of an MLD modes octet.
const MODE_EMLMR: u8 = 0x10;

/// `AP_MLD_MAC_Addr_Valid` bit of an Agent AP MLD Configuration entry.
const AP_MLD_MAC_ADDR_VALID: u8 = 0x80;
/// `Affiliated_AP_MAC_Addr_Valid` bit of an affiliated AP entry.
const AFFILIATED_AP_MAC_ADDR_VALID: u8 = 0x80;
/// `LinkID_Valid` bit of an affiliated AP entry.
const LINK_ID_VALID: u8 = 0x40;

/// `bSTA_MLD_MAC_Addr_Valid` bit of the Backhaul STA MLD Configuration.
const BSTA_MLD_MAC_ADDR_VALID: u8 = 0x80;
/// `AP_MLD_MAC_Addr_valid` bit of the Backhaul STA MLD Configuration.
const BSTA_AP_MLD_MAC_ADDR_VALID: u8 = 0x40;
/// `Affiliated_bSTA_MAC_Addr_Valid` bit of an affiliated bSTA entry.
const AFFILIATED_BSTA_MAC_ADDR_VALID: u8 = 0x80;

/// `is_bSTA_Config` bit of the TID-to-Link Mapping Policy header octet.
const IS_BSTA_CONFIG: u8 = 0x80;
/// `TID-To-Link_Mapping_Negotiation` bit of the TID-to-Link Mapping Policy.
const MAPPING_NEGOTIATION: u8 = 0x80;
/// `addRemove mapping` bit of a TID-to-Link mapping: 1 adds the mapping.
const ADD_MAPPING: u8 = 0x80;
/// Bit position of the Direction field in the TID-to-Link control octet (bits 7-6).
const DIRECTION_SHIFT: u8 = 6;
/// `Default_Link_Mapping` bit of the TID-to-Link control octet.
const DEFAULT_LINK_MAPPING: u8 = 0x20;
/// `Mapping_Switch_Time_Present` bit of the TID-to-Link control octet.
const MAPPING_SWITCH_TIME_PRESENT: u8 = 0x10;
/// `Expected_Duration_Present` bit of the TID-to-Link control octet.
const EXPECTED_DURATION_PRESENT: u8 = 0x08;
/// `Link_Mapping_Size` bit of the TID-to-Link control octet.
const LINK_MAPPING_SIZE: u8 = 0x04;

/// `EHT_Operation_Information_Valid` bit of an EHT Operations BSS flags octet.
const EHT_OPERATION_INFORMATION_VALID: u8 = 0x80;
/// `Disabled_Subchannel_Valid` bit of an EHT Operations BSS flags octet.
const DISABLED_SUBCHANNEL_VALID: u8 = 0x40;
/// `EHT Default PE Duration` bit of an EHT Operations BSS flags octet.
const EHT_DEFAULT_PE_DURATION: u8 = 0x20;
/// `Group Addressed BU Indication Limit` bit of an EHT Operations BSS flags octet.
const GROUP_ADDRESSED_BU_INDICATION_LIMIT: u8 = 0x10;
/// Bit position of the Group Addressed BU Indication Exponent (bits 3-2).
const BU_EXPONENT_SHIFT: u8 = 2;

/// Largest value of a two-bit field.
const MAX_TWO_BIT_VALUE: u8 = 3;

/// Reserved octets after the modes octet of an Agent AP MLD entry.
const AP_MLD_RESERVED_OCTETS: usize = 20;
/// Reserved octets at the end of an affiliated AP entry.
const AFFILIATED_AP_RESERVED_OCTETS: usize = 18;
/// Reserved octets after the modes octet of the Backhaul STA MLD Configuration.
const BSTA_MLD_RESERVED_OCTETS: usize = 17;
/// Reserved octets at the end of an affiliated bSTA entry.
const AFFILIATED_BSTA_RESERVED_OCTETS: usize = 19;
/// Reserved octets after the modes octet of the Associated STA MLD Configuration Report.
const STA_MLD_RESERVED_OCTETS: usize = 18;
/// Reserved octets at the end of an affiliated STA entry.
const AFFILIATED_STA_RESERVED_OCTETS: usize = 19;
/// Reserved octets after the MLD MAC address of the MLD Structure.
const MLD_STRUCTURE_RESERVED_OCTETS: usize = 25;
/// Reserved octets at the end of an affiliated MAC entry of the MLD Structure.
const AFFILIATED_MAC_RESERVED_OCTETS: usize = 26;
/// Reserved octets after the negotiation octet of the TID-to-Link Mapping Policy.
const TTLM_HEADER_RESERVED_OCTETS: usize = 22;
/// Reserved octets at the end of a TID-to-Link mapping.
const TTLM_MAPPING_RESERVED_OCTETS: usize = 7;
/// Reserved octets at the start of the EHT Operations.
const EHT_HEADER_RESERVED_OCTETS: usize = 32;
/// Reserved octets at the end of an EHT Operations BSS.
const EHT_BSS_RESERVED_OCTETS: usize = 16;
/// Reserved octets at the end of an EHT Operations radio.
const EHT_RADIO_RESERVED_OCTETS: usize = 25;

/// Returns `mask` if `is_set`, else zero.
fn flag_bit(is_set: bool, mask: u8) -> u8 {
    if is_set { mask } else { 0 }
}

/// Checks that a two-bit field value fits in its two bits.
fn two_bit_value(value: u8) -> Result<u8, EncodeError> {
    if value > MAX_TWO_BIT_VALUE {
        return Err(EncodeError::ValueTooLarge {
            value: u32::from(value),
            max: u32::from(MAX_TWO_BIT_VALUE),
        });
    }
    Ok(value)
}

/// Reads a one-octet length and that many raw SSID octets.
fn read_ssid<'a>(reader: &mut Reader<'a>) -> Result<&'a [u8], DecodeError> {
    let len = usize::from(reader.u8()?);
    reader.bytes(len)
}

/// Writes a one-octet length and the raw SSID octets.
fn write_ssid(writer: &mut Writer, ssid: &[u8]) -> Result<(), EncodeError> {
    writer.count_u8(ssid.len())?;
    writer.bytes(ssid);
    Ok(())
}

/// The MLD operation modes of an MLD modes octet (bits 7-4; bits 3-0 are reserved).
///
/// In an AP MLD or bSTA MLD configuration a set bit means the mode is (to be) enabled; in an
/// Associated STA MLD Configuration Report it means the mode is enabled on the STA MLD.
#[allow(clippy::struct_excessive_bools)] // Independent wire flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MldModes {
    /// `STR` (bit 7): simultaneous transmit and receive.
    pub str_enabled: bool,
    /// `NSTR` (bit 6): non-simultaneous transmit and receive.
    pub nstr_enabled: bool,
    /// `EMLSR` (bit 5): enhanced multi-link single radio.
    pub emlsr_enabled: bool,
    /// `EMLMR` (bit 4): enhanced multi-link multi-radio.
    pub emlmr_enabled: bool,
}

impl MldModes {
    fn from_octet(octet: u8) -> Self {
        Self {
            str_enabled: octet & MODE_STR != 0,
            nstr_enabled: octet & MODE_NSTR != 0,
            emlsr_enabled: octet & MODE_EMLSR != 0,
            emlmr_enabled: octet & MODE_EMLMR != 0,
        }
    }

    fn to_octet(self) -> u8 {
        flag_bit(self.str_enabled, MODE_STR)
            | flag_bit(self.nstr_enabled, MODE_NSTR)
            | flag_bit(self.emlsr_enabled, MODE_EMLSR)
            | flag_bit(self.emlmr_enabled, MODE_EMLMR)
    }
}

/// One BSS of a radio in [`BssConfigurationReport`].
#[allow(clippy::struct_excessive_bools)] // Independent wire flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BssConfigurationReportBss<'a> {
    /// BSSID of the BSS; for an MLD, the MAC address of the affiliated AP.
    pub bssid: MacAddr,
    /// `BackHaul BSS` (bit 7), raw: 1 means the BSS is not in use as a backhaul BSS.
    pub backhaul_not_in_use: bool,
    /// `Fronthaul BSS` (bit 6), raw: 1 means the BSS is not in use as a fronthaul BSS.
    pub fronthaul_not_in_use: bool,
    /// `R1 disallowed status` (bit 5): 1 means Profile-1 clients are disallowed.
    pub r1_disallowed: bool,
    /// `R2 disallowed status` (bit 4): 1 means Profile-2 clients are disallowed.
    pub r2_disallowed: bool,
    /// `Multiple BSSID` (bit 3): 1 means the BSS is in a configured multiple BSSID set.
    pub multiple_bssid: bool,
    /// `Transmitted BSSID` (bit 2): 1 means the transmitted BSSID, 0 a non-transmitted one.
    pub transmitted_bssid: bool,
    /// SSID bytes, raw: any length from 0 to 255 octets is kept, including a trailing NUL
    /// (note 0009, D2 and D4).
    pub ssid: &'a [u8],
}

/// One radio in [`BssConfigurationReport`].
#[derive(Debug, PartialEq, Eq)]
pub struct BssConfigurationReportRadio<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// The BSSs currently operating on the radio, in wire order. The count on the wire is the
    /// list length; which BSSs the sender counts is the consumer's concern (note 0009, D3).
    pub bsses: Vec<BssConfigurationReportBss<'a>>,
}

/// BSS Configuration Report, `EasyMesh` §17.2.75 Table 97; audit rows 228-233.
///
/// Upstream sender `create_bss_config_rprt_tlv` (`em_configuration.cpp:1115-1169`). The receiver
/// is a stub, so nothing is parsed at the pin (audit row 232). The value is the radio count and,
/// per radio, the RUID, the BSS count and the BSS records. A BSS record is the BSSID, a flags
/// octet, a reserved octet, the SSID length and the SSID. Bits 1-0 of the flags octet and the
/// reserved octet are ignored on decode and written as zero (audit row 228).
///
/// Intentional difference: upstream sends the SSID length as `strlen + 1` and the trailing NUL
/// as part of the SSID (`em_configuration.cpp:1149-1155`, audit row 229). Those bytes are a spec
/// SSID that ends in a NUL, and this body keeps them raw (note 0009, D4). An SSID longer than
/// 255 octets does not encode: [`EncodeError::CountTooLarge`], as for the radio and BSS counts.
#[derive(Debug, PartialEq, Eq)]
pub struct BssConfigurationReport<'a> {
    /// The reported radios, in wire order.
    pub radios: Vec<BssConfigurationReportRadio<'a>>,
}

impl<'a> TlvBody<'a> for BssConfigurationReport<'a> {
    const TLV_TYPE: TlvType = TlvType::BSS_CONFIGURATION_REPORT;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let radio_count = usize::from(reader.u8()?);
        // A radio is at least the RUID and the BSS count.
        let mut radios = bounded_vec(radio_count, reader.remaining(), 7);
        for _ in 0..radio_count {
            let ruid = reader.mac()?;
            let bss_count = usize::from(reader.u8()?);
            // A BSS is at least the BSSID, two octets, and the SSID length.
            let mut bsses = bounded_vec(bss_count, reader.remaining(), 9);
            for _ in 0..bss_count {
                let bssid = reader.mac()?;
                let flags = reader.u8()?;
                reader.skip(1)?;
                let ssid = read_ssid(&mut reader)?;
                bsses.push(BssConfigurationReportBss {
                    bssid,
                    backhaul_not_in_use: flags & BACKHAUL_NOT_IN_USE != 0,
                    fronthaul_not_in_use: flags & FRONTHAUL_NOT_IN_USE != 0,
                    r1_disallowed: flags & R1_DISALLOWED != 0,
                    r2_disallowed: flags & R2_DISALLOWED != 0,
                    multiple_bssid: flags & MULTIPLE_BSSID != 0,
                    transmitted_bssid: flags & TRANSMITTED_BSSID != 0,
                    ssid,
                });
            }
            radios.push(BssConfigurationReportRadio { ruid, bsses });
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
                writer.u8(flag_bit(bss.backhaul_not_in_use, BACKHAUL_NOT_IN_USE)
                    | flag_bit(bss.fronthaul_not_in_use, FRONTHAUL_NOT_IN_USE)
                    | flag_bit(bss.r1_disallowed, R1_DISALLOWED)
                    | flag_bit(bss.r2_disallowed, R2_DISALLOWED)
                    | flag_bit(bss.multiple_bssid, MULTIPLE_BSSID)
                    | flag_bit(bss.transmitted_bssid, TRANSMITTED_BSSID));
                writer.zeros(1);
                write_ssid(&mut writer, bss.ssid)?;
            }
        }
        Ok(writer.into_bytes())
    }
}

/// BSS Configuration Request, `EasyMesh` §17.2.84 Table 106; audit rows 252-253.
///
/// The value is one or more DPP attributes that carry the JSON DPP Configuration Request Object
/// (DPP §8.1). It is kept as raw octets for `em-easyconnect` (note 0009, D5); no attribute
/// framing or JSON is parsed. An empty value is [`DecodeError::Truncated`], and it does not
/// encode: [`EncodeError::CountTooSmall`] (note 0009, D8).
///
/// Upstream sender `create_bss_conf_req_tlv` (`em_configuration.cpp:3913-3964`) writes three
/// attributes: the request object, the Initiator Protocol Key and the Finite Cyclic Group;
/// receiver `handle_bss_config_req_tlv` (`em_configuration.cpp:5264-5311`). Those are all bytes
/// of the value, so they are accepted as they are. Upstream's attribute walk does not check
/// that an attribute fits the TLV (audit row 253); this body does not walk attributes at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BssConfigurationRequest<'a> {
    /// The raw TLV value.
    pub value: &'a [u8],
}

impl<'a> TlvBody<'a> for BssConfigurationRequest<'a> {
    const TLV_TYPE: TlvType = TlvType::BSS_CONFIGURATION_REQUEST;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        decode_nonempty_value(value).map(|value| Self { value })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        encode_nonempty_value(self.value)
    }
}

/// BSS Configuration Response, `EasyMesh` §17.2.85 Table 107; audit rows 254-256.
///
/// The value is one or more DPP attributes that carry the JSON DPP Configuration Object (DPP
/// §8.1). It is kept as raw octets for `em-easyconnect` (note 0009, D5); no attribute framing
/// or JSON is parsed. An empty value is [`DecodeError::Truncated`], and it does not encode:
/// [`EncodeError::CountTooSmall`] (note 0009, D8).
///
/// Upstream sender `create_bss_conf_resp_tlv` (`em_configuration.cpp:4012-4067`) writes the bare
/// JSON string with no attribute header (audit row 254); receiver `handle_bss_config_rsp_tlv`
/// (`em_configuration.cpp:5313-5543`). The value is kept raw, so this body does not tell the bare
/// JSON form from the attribute form: both are kept as the bytes found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BssConfigurationResponse<'a> {
    /// The raw TLV value.
    pub value: &'a [u8],
}

impl<'a> TlvBody<'a> for BssConfigurationResponse<'a> {
    const TLV_TYPE: TlvType = TlvType::BSS_CONFIGURATION_RESPONSE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        decode_nonempty_value(value).map(|value| Self { value })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        encode_nonempty_value(self.value)
    }
}

/// Takes the whole value of a TLV that must hold at least one octet.
fn decode_nonempty_value(value: &[u8]) -> Result<&[u8], DecodeError> {
    if value.is_empty() {
        return Err(DecodeError::Truncated {
            needed: 1,
            remaining: 0,
        });
    }
    Ok(value)
}

/// Copies the raw value of a TLV that must hold at least one octet.
fn encode_nonempty_value(value: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if value.is_empty() {
        return Err(EncodeError::CountTooSmall { len: 0, min: 1 });
    }
    Ok(value.to_vec())
}

/// One affiliated AP of an AP MLD in [`AgentApMldConfiguration`].
#[allow(clippy::struct_excessive_bools)] // Independent wire flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffiliatedAp {
    /// `Affiliated_AP_MAC_Addr_Valid` (bit 7): whether `mac` is valid. `mac` is always on the
    /// wire and is kept as found.
    pub mac_valid: bool,
    /// `LinkID_Valid` (bit 6): whether `link_id` is valid. `link_id` is always on the wire and
    /// is kept as found.
    pub link_id_valid: bool,
    /// RUID of the radio on which the affiliated AP operates.
    pub ruid: MacAddr,
    /// MAC address of the affiliated AP.
    pub mac: MacAddr,
    /// Link ID of the setup link: 0-15 is a link ID, 16-255 is reserved. Kept raw.
    pub link_id: u8,
}

/// One AP MLD in [`AgentApMldConfiguration`].
#[derive(Debug, PartialEq, Eq)]
pub struct ApMld<'a> {
    /// `AP_MLD_MAC_Addr_Valid` (bit 7): whether `mac` is valid. `mac` is always on the wire and
    /// is kept as found.
    pub mac_valid: bool,
    /// SSID of the APs affiliated to the MLD, raw: any length from 0 to 255 octets is kept.
    pub ssid: &'a [u8],
    /// AP MLD MAC address.
    pub mac: MacAddr,
    /// The MLD modes (to be) enabled on the AP MLD.
    pub modes: MldModes,
    /// The affiliated APs, in wire order.
    pub affiliated_aps: Vec<AffiliatedAp>,
}

/// Agent AP MLD Configuration, `EasyMesh` §17.2.96 Table 118; audit rows 278-286.
///
/// Upstream sender `create_ap_mld_config_tlv` (`em_configuration.cpp:1366-1426`), receiver
/// `handle_ap_mld_config_tlv` (`em_configuration.cpp:2740-2858`). The value is the AP MLD count
/// and, per AP MLD, the valid octet, the SSID length and SSID, the AP MLD MAC address, the modes
/// octet, 20 reserved octets, the affiliated AP count and the affiliated AP records. An
/// affiliated AP record is the flags octet, the RUID, the MAC address, the link ID and 18
/// reserved octets. Reserved bits and blocks are ignored on decode and written as zero.
///
/// The spec says `AP_MLD_MAC_Addr`, `Affiliated_AP_MAC_Addr` and `LinkID` are valid only if
/// their flag is set. Both ends keep the field on the wire (audit rows 285 and 286; upstream's
/// receiver reads them without testing the flag, `em_configuration.cpp:2824-2829`), so the
/// flags never change the layout and a field with a cleared flag is carried as found
/// (note 0009, D2).
///
/// Intentional differences: upstream's sender always writes SSID Length 32 and a 32-octet SSID
/// padded with NULs (`em_configuration.cpp:1393-1394`, audit row 279); this body keeps those
/// octets as the SSID. Upstream's receiver checks the SSID length (`em_configuration.cpp:2776`)
/// but then steps over each entry by the fixed `sizeof(em_ap_mld_t)` (`em_configuration.cpp:2854`,
/// audit row 278); this body steps by the length on the wire.
#[derive(Debug, PartialEq, Eq)]
pub struct AgentApMldConfiguration<'a> {
    /// The AP MLDs, in wire order.
    pub ap_mlds: Vec<ApMld<'a>>,
}

impl<'a> TlvBody<'a> for AgentApMldConfiguration<'a> {
    const TLV_TYPE: TlvType = TlvType::AGENT_AP_MLD_CONFIGURATION;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let mld_count = usize::from(reader.u8()?);
        // An AP MLD is at least its fixed fields with an empty SSID and no affiliated APs.
        let mut ap_mlds = bounded_vec(mld_count, reader.remaining(), 30);
        for _ in 0..mld_count {
            let valid_octet = reader.u8()?;
            let ssid = read_ssid(&mut reader)?;
            let mac = reader.mac()?;
            let modes = MldModes::from_octet(reader.u8()?);
            reader.skip(AP_MLD_RESERVED_OCTETS)?;
            let affiliated_count = usize::from(reader.u8()?);
            let mut affiliated_aps = bounded_vec(affiliated_count, reader.remaining(), 32);
            for _ in 0..affiliated_count {
                let flags = reader.u8()?;
                let ruid = reader.mac()?;
                let affiliated_mac = reader.mac()?;
                let link_id = reader.u8()?;
                reader.skip(AFFILIATED_AP_RESERVED_OCTETS)?;
                affiliated_aps.push(AffiliatedAp {
                    mac_valid: flags & AFFILIATED_AP_MAC_ADDR_VALID != 0,
                    link_id_valid: flags & LINK_ID_VALID != 0,
                    ruid,
                    mac: affiliated_mac,
                    link_id,
                });
            }
            ap_mlds.push(ApMld {
                mac_valid: valid_octet & AP_MLD_MAC_ADDR_VALID != 0,
                ssid,
                mac,
                modes,
                affiliated_aps,
            });
        }
        reader.finish()?;
        Ok(Self { ap_mlds })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.ap_mlds.len())?;
        for ap_mld in &self.ap_mlds {
            writer.u8(flag_bit(ap_mld.mac_valid, AP_MLD_MAC_ADDR_VALID));
            write_ssid(&mut writer, ap_mld.ssid)?;
            writer.mac(ap_mld.mac);
            writer.u8(ap_mld.modes.to_octet());
            writer.zeros(AP_MLD_RESERVED_OCTETS);
            writer.count_u8(ap_mld.affiliated_aps.len())?;
            for affiliated in &ap_mld.affiliated_aps {
                writer.u8(flag_bit(affiliated.mac_valid, AFFILIATED_AP_MAC_ADDR_VALID)
                    | flag_bit(affiliated.link_id_valid, LINK_ID_VALID));
                writer.mac(affiliated.ruid);
                writer.mac(affiliated.mac);
                writer.u8(affiliated.link_id);
                writer.zeros(AFFILIATED_AP_RESERVED_OCTETS);
            }
        }
        Ok(writer.into_bytes())
    }
}

/// One affiliated bSTA of the bSTA MLD in [`BackhaulStaMldConfiguration`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffiliatedBsta {
    /// `Affiliated_bSTA_MAC_Addr_Valid` (bit 7): whether `mac` is valid. `mac` is always on the
    /// wire and is kept as found.
    pub mac_valid: bool,
    /// RUID of the radio on which the affiliated bSTA operates.
    pub ruid: MacAddr,
    /// MAC address of the affiliated bSTA.
    pub mac: MacAddr,
}

/// Backhaul STA MLD Configuration, `EasyMesh` §17.2.97 Table 119; audit rows 287-290.
///
/// Upstream sender `create_bsta_mld_config_tlv` (`em_configuration.cpp:1428-1472`), receiver
/// `handle_bsta_mld_config_req` (`em_configuration.cpp:2169-2176`), which parses nothing and only
/// sends a response (audit row 289). The value is the
/// valid octet, the bSTA MLD and AP MLD MAC addresses, the modes octet, 17 reserved octets, the
/// affiliated bSTA count and the affiliated bSTA records (flags octet, RUID, MAC address and 19
/// reserved octets). Reserved bits and blocks are ignored on decode and written as zero. Both
/// MAC addresses are always on the wire, and so is the affiliated MAC address; the valid flags
/// never change the layout (audit row 290).
///
/// Lenient form: when no bSTA MLD is configured upstream still sends the TLV with an empty
/// value (`em_configuration.cpp:1439-1442`). The BSS Configuration Response adds the TLV without a
/// check (`em_configuration.cpp:3771-3773`, audit row 287). An empty value decodes to `Ok(None)`,
/// which means the TLV is absent. The strict
/// decoder rejects it with `Truncated`.
#[derive(Debug, PartialEq, Eq)]
pub struct BackhaulStaMldConfiguration {
    /// `bSTA_MLD_MAC_Addr_Valid` (bit 7): whether `bsta_mld_mac` is valid.
    pub bsta_mld_mac_valid: bool,
    /// `AP_MLD_MAC_Addr_valid` (bit 6): whether `ap_mld_mac` is valid.
    pub ap_mld_mac_valid: bool,
    /// MAC address of the bSTA MLD.
    pub bsta_mld_mac: MacAddr,
    /// MAC address of the AP MLD to which the bSTA MLD is associated.
    pub ap_mld_mac: MacAddr,
    /// The MLD modes (to be) enabled on the bSTA MLD.
    pub modes: MldModes,
    /// The affiliated bSTAs of the bSTA non-AP MLD, in wire order.
    pub affiliated_bstas: Vec<AffiliatedBsta>,
}

impl TlvBody<'_> for BackhaulStaMldConfiguration {
    const TLV_TYPE: TlvType = TlvType::BACKHAUL_STA_MLD_CONFIGURATION;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let valid_octet = reader.u8()?;
        let bsta_mld_mac = reader.mac()?;
        let ap_mld_mac = reader.mac()?;
        let modes = MldModes::from_octet(reader.u8()?);
        reader.skip(BSTA_MLD_RESERVED_OCTETS)?;
        let affiliated_count = usize::from(reader.u8()?);
        let mut affiliated_bstas = bounded_vec(affiliated_count, reader.remaining(), 32);
        for _ in 0..affiliated_count {
            let flags = reader.u8()?;
            let ruid = reader.mac()?;
            let mac = reader.mac()?;
            reader.skip(AFFILIATED_BSTA_RESERVED_OCTETS)?;
            affiliated_bstas.push(AffiliatedBsta {
                mac_valid: flags & AFFILIATED_BSTA_MAC_ADDR_VALID != 0,
                ruid,
                mac,
            });
        }
        reader.finish()?;
        Ok(Self {
            bsta_mld_mac_valid: valid_octet & BSTA_MLD_MAC_ADDR_VALID != 0,
            ap_mld_mac_valid: valid_octet & BSTA_AP_MLD_MAC_ADDR_VALID != 0,
            bsta_mld_mac,
            ap_mld_mac,
            modes,
            affiliated_bstas,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(flag_bit(self.bsta_mld_mac_valid, BSTA_MLD_MAC_ADDR_VALID)
            | flag_bit(self.ap_mld_mac_valid, BSTA_AP_MLD_MAC_ADDR_VALID));
        writer.mac(self.bsta_mld_mac);
        writer.mac(self.ap_mld_mac);
        writer.u8(self.modes.to_octet());
        writer.zeros(BSTA_MLD_RESERVED_OCTETS);
        writer.count_u8(self.affiliated_bstas.len())?;
        for affiliated in &self.affiliated_bstas {
            writer.u8(flag_bit(
                affiliated.mac_valid,
                AFFILIATED_BSTA_MAC_ADDR_VALID,
            ));
            writer.mac(affiliated.ruid);
            writer.mac(affiliated.mac);
            writer.zeros(AFFILIATED_BSTA_RESERVED_OCTETS);
        }
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for BackhaulStaMldConfiguration {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        if value.is_empty() {
            Ok(None)
        } else {
            Self::decode_value(value).map(Some)
        }
    }
}

/// One affiliated STA of the STA MLD in [`AssociatedStaMldConfigurationReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffiliatedSta {
    /// BSSID of the affiliated AP to which the affiliated STA has a setup link.
    pub bssid: MacAddr,
    /// MAC address of the affiliated STA.
    pub mac: MacAddr,
}

/// Associated STA MLD Configuration Report, `EasyMesh` §17.2.98 Table 120; audit rows 291-292.
///
/// Upstream sender `create_assoc_sta_mld_config_report_tlv` (`em_configuration.cpp:1474-1530`),
/// receiver `handle_assoc_sta_mld_conf_rep_tlv` (`em_configuration.cpp:2602-2738`). The value is
/// the STA MLD and AP MLD MAC addresses, the modes octet, 18 reserved octets, the affiliated STA
/// count and the affiliated STA records (BSSID, MAC address and 19 reserved octets). Reserved
/// bits and blocks are ignored on decode and written as zero.
///
/// Intentional difference: when the count does not fit the value, upstream logs "Malformed" and
/// continues with the count clamped to the entries that fit (`em_configuration.cpp:2637-2644`,
/// audit row 291). This body reports
/// [`DecodeError::Truncated`].
#[derive(Debug, PartialEq, Eq)]
pub struct AssociatedStaMldConfigurationReport {
    /// MAC address of the associated client MLD or bSTA MLD.
    pub sta_mld_mac: MacAddr,
    /// MAC address of the AP MLD to which the STA MLD is associated.
    pub ap_mld_mac: MacAddr,
    /// The MLD modes enabled on the STA MLD.
    pub modes: MldModes,
    /// The affiliated STAs of the STA non-AP MLD, in wire order.
    pub affiliated_stas: Vec<AffiliatedSta>,
}

impl TlvBody<'_> for AssociatedStaMldConfigurationReport {
    const TLV_TYPE: TlvType = TlvType::ASSOCIATED_STA_MLD_CONFIGURATION_REPORT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mld_mac = reader.mac()?;
        let ap_mld_mac = reader.mac()?;
        let modes = MldModes::from_octet(reader.u8()?);
        reader.skip(STA_MLD_RESERVED_OCTETS)?;
        let affiliated_count = usize::from(reader.u8()?);
        let mut affiliated_stas = bounded_vec(affiliated_count, reader.remaining(), 31);
        for _ in 0..affiliated_count {
            let bssid = reader.mac()?;
            let mac = reader.mac()?;
            reader.skip(AFFILIATED_STA_RESERVED_OCTETS)?;
            affiliated_stas.push(AffiliatedSta { bssid, mac });
        }
        reader.finish()?;
        Ok(Self {
            sta_mld_mac,
            ap_mld_mac,
            modes,
            affiliated_stas,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mld_mac);
        writer.mac(self.ap_mld_mac);
        writer.u8(self.modes.to_octet());
        writer.zeros(STA_MLD_RESERVED_OCTETS);
        writer.count_u8(self.affiliated_stas.len())?;
        for affiliated in &self.affiliated_stas {
            writer.mac(affiliated.bssid);
            writer.mac(affiliated.mac);
            writer.zeros(AFFILIATED_STA_RESERVED_OCTETS);
        }
        Ok(writer.into_bytes())
    }
}

/// MLD Structure, `EasyMesh` §17.2.99 Table 121; audit row 293.
///
/// Spec only: upstream has no type code, sender or receiver at the pin. The value is the MLD
/// MAC address, 25 reserved octets, the affiliated count and, per affiliated MAC address, the
/// address and 26 reserved octets. Reserved blocks are ignored on decode and written as zero.
#[derive(Debug, PartialEq, Eq)]
pub struct MldStructure {
    /// MAC address of the MLD.
    pub mld_mac: MacAddr,
    /// MAC addresses of the affiliated STAs or APs, in wire order.
    pub affiliated_macs: Vec<MacAddr>,
}

impl TlvBody<'_> for MldStructure {
    const TLV_TYPE: TlvType = TlvType::MLD_STRUCTURE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let mld_mac = reader.mac()?;
        reader.skip(MLD_STRUCTURE_RESERVED_OCTETS)?;
        let affiliated_count = usize::from(reader.u8()?);
        let mut affiliated_macs = bounded_vec(affiliated_count, reader.remaining(), 32);
        for _ in 0..affiliated_count {
            affiliated_macs.push(reader.mac()?);
            reader.skip(AFFILIATED_MAC_RESERVED_OCTETS)?;
        }
        reader.finish()?;
        Ok(Self {
            mld_mac,
            affiliated_macs,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.mld_mac);
        writer.zeros(MLD_STRUCTURE_RESERVED_OCTETS);
        writer.count_u8(self.affiliated_macs.len())?;
        for mac in &self.affiliated_macs {
            writer.mac(*mac);
            writer.zeros(AFFILIATED_MAC_RESERVED_OCTETS);
        }
        Ok(writer.into_bytes())
    }
}

/// Affiliated STA Metrics, `EasyMesh` §17.2.100 Table 122; audit row 294.
///
/// Upstream sender `create_affiliated_sta_metrics_tlvs` (`em_configuration.cpp:93-154`), which
/// writes each counter with `htonl`; there is no receiver. The value is the STA MAC address and
/// five 4-octet counters. The octets after the counters are the spec's "reserved for future
/// expansion" (Table 122): decoding ignores them, of any length and without
/// [`DecodeError::TrailingBytes`], and encoding writes none (`EasyMesh` §3.1.2: reserved objects
/// are ignored by the recipient; the fixture header of `mld.a.tsv` states the same rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffiliatedStaMetrics {
    /// MAC address of the affiliated STA.
    pub sta_mac: MacAddr,
    /// Raw counter of bytes sent to the affiliated STA.
    pub bytes_sent: u32,
    /// Raw counter of bytes received from the affiliated STA.
    pub bytes_received: u32,
    /// Raw counter of packets sent to the affiliated STA.
    pub packets_sent: u32,
    /// Raw counter of packets received from the affiliated STA.
    pub packets_received: u32,
    /// Raw counter of packets that could not be sent to the affiliated STA due to errors.
    pub packets_sent_errors: u32,
}

impl TlvBody<'_> for AffiliatedStaMetrics {
    const TLV_TYPE: TlvType = TlvType::AFFILIATED_STA_METRICS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        // Whatever follows the counters is reserved and ignored: no `finish`.
        Ok(Self {
            sta_mac: reader.mac()?,
            bytes_sent: reader.be_u32()?,
            bytes_received: reader.be_u32()?,
            packets_sent: reader.be_u32()?,
            packets_received: reader.be_u32()?,
            packets_sent_errors: reader.be_u32()?,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.be_u32(self.bytes_sent);
        writer.be_u32(self.bytes_received);
        writer.be_u32(self.packets_sent);
        writer.be_u32(self.packets_received);
        writer.be_u32(self.packets_sent_errors);
        Ok(writer.into_bytes())
    }
}

/// Affiliated AP Metrics, `EasyMesh` §17.2.101 Table 123; audit row 295.
///
/// Spec only: upstream has no type code, sender or receiver at the pin. The value is the BSSID
/// and nine 4-octet counters in the table order. The octets after the counters are the spec's
/// "reserved for future expansion" (Table 123): decoding ignores them, of any length and without
/// [`DecodeError::TrailingBytes`], and encoding writes none (`EasyMesh` §3.1.2: reserved objects
/// are ignored by the recipient; the fixture header of `mld.a.tsv` states the same rule). Byte
/// counters are in the units of the Byte Counter Units field of the Profile-2 AP Capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffiliatedApMetrics {
    /// BSSID of the affiliated AP.
    pub bssid: MacAddr,
    /// Raw counter of packets sent by the affiliated AP (MLD traffic only).
    pub packets_sent: u32,
    /// Raw counter of packets received by the affiliated AP (MLD traffic only).
    pub packets_received: u32,
    /// Raw counter of packets that could not be sent due to errors (MLD traffic only).
    pub packet_sent_errors: u32,
    /// `BSS.UnicastBytesSent` counter (MLD traffic only).
    pub unicast_bytes_sent: u32,
    /// `BSS.UnicastBytesReceived` counter (MLD traffic only).
    pub unicast_bytes_received: u32,
    /// `BSS.MulticastBytesSent` counter (MLD traffic only).
    pub multicast_bytes_sent: u32,
    /// `BSS.MulticastBytesReceived` counter (MLD traffic only).
    pub multicast_bytes_received: u32,
    /// `BSS.BroadcastBytesSent` counter (MLD traffic only).
    pub broadcast_bytes_sent: u32,
    /// `BSS.BroadcastBytesReceived` counter (MLD traffic only).
    pub broadcast_bytes_received: u32,
}

impl TlvBody<'_> for AffiliatedApMetrics {
    const TLV_TYPE: TlvType = TlvType::AFFILIATED_AP_METRICS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        // Whatever follows the counters is reserved and ignored: no `finish`.
        Ok(Self {
            bssid: reader.mac()?,
            packets_sent: reader.be_u32()?,
            packets_received: reader.be_u32()?,
            packet_sent_errors: reader.be_u32()?,
            unicast_bytes_sent: reader.be_u32()?,
            unicast_bytes_received: reader.be_u32()?,
            multicast_bytes_sent: reader.be_u32()?,
            multicast_bytes_received: reader.be_u32()?,
            broadcast_bytes_sent: reader.be_u32()?,
            broadcast_bytes_received: reader.be_u32()?,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.be_u32(self.packets_sent);
        writer.be_u32(self.packets_received);
        writer.be_u32(self.packet_sent_errors);
        writer.be_u32(self.unicast_bytes_sent);
        writer.be_u32(self.unicast_bytes_received);
        writer.be_u32(self.multicast_bytes_sent);
        writer.be_u32(self.multicast_bytes_received);
        writer.be_u32(self.broadcast_bytes_sent);
        writer.be_u32(self.broadcast_bytes_received);
        Ok(writer.into_bytes())
    }
}

/// One TID-to-Link mapping of an AP MLD in [`TidToLinkMappingPolicy`].
#[allow(clippy::struct_excessive_bools)] // Independent wire flags.
#[derive(Debug, PartialEq, Eq)]
pub struct TidToLinkMapping {
    /// `addRemove mapping` (bit 7): true adds the mapping, false removes it. An Agent that
    /// reports its current configuration sets it.
    pub add: bool,
    /// MAC address of the client MLD or bSTA MLD to which the mapping applies.
    pub sta_mld_mac: MacAddr,
    /// `Direction` (bits 7-6 of the control octet), 0 to 3, all carried (note 0009, D3). A value
    /// above 3 does not encode: [`EncodeError::ValueTooLarge`].
    pub direction: u8,
    /// `Default_Link_Mapping` (bit 5 of the control octet).
    pub default_link_mapping: bool,
    /// `Mapping_Switch_Time_Present` (bit 4 of the control octet).
    pub mapping_switch_time_present: bool,
    /// `Expected_Duration_Present` (bit 3 of the control octet). `expected_duration` is always
    /// on the wire and is kept as found.
    pub expected_duration_present: bool,
    /// `Link_Mapping_Size` (bit 2 of the control octet).
    pub link_mapping_size: bool,
    /// `Link_Mapping_Presence_Indicator`: the TIDs that have a TID-to-Link mapping. How its bits
    /// map to the entries of `tid_to_link` is not interpreted here.
    pub link_mapping_presence: u8,
    /// `Expected_Duration` in TUs, a 3-octet big-endian value. A value above `0xFF_FFFF` does
    /// not encode: [`EncodeError::ValueTooLarge`].
    pub expected_duration: u32,
    /// The `TID-to-Link_Mapping` entries, 2 raw octets each, in wire order. There is one entry
    /// for every set bit of `link_mapping_presence`; any other count does not encode.
    pub tid_to_link: Vec<[u8; 2]>,
}

/// What a [`TidToLinkMappingPolicy`] applies to, which also decides whether it has mappings.
///
/// The spec sets `Num_Mapping` to zero when `is_bSTA_Config` is 1, so a policy for a bSTA MLD
/// cannot hold mappings here (note 0009, A8).
#[derive(Debug, PartialEq, Eq)]
pub enum TidToLinkScope {
    /// `is_bSTA_Config` 0: the policy is for an AP MLD, with its mappings.
    ApMld {
        /// The TID-to-Link mappings, in wire order.
        mappings: Vec<TidToLinkMapping>,
    },
    /// `is_bSTA_Config` 1: the policy is for a bSTA MLD. `Num_Mapping` is zero.
    BstaMld,
}

impl TidToLinkScope {
    fn mappings(&self) -> &[TidToLinkMapping] {
        match self {
            Self::ApMld { mappings } => mappings,
            Self::BstaMld => &[],
        }
    }
}

/// TID-to-Link Mapping Policy, `EasyMesh` §17.2.102 Table 124; audit rows 18-24.
///
/// Upstream sender `create_tid_to_link_map_policy_tlv` (`em_configuration.cpp:1650-1697`); there
/// is no receiver (audit row 24). The value is the `is_bSTA_Config` octet, the MLD MAC address,
/// the negotiation octet, 22 reserved octets, `Num_Mapping` and the mappings. A mapping is the
/// `addRemove` octet, the STA MLD MAC address, the control octet, the presence indicator, the
/// 3-octet `Expected_Duration`, the TID-to-Link entries and 7 reserved octets. Reserved bits and
/// blocks are ignored on decode and written as zero. `Expected_Duration` is always on the wire,
/// also when `Expected_Duration_Present` is 0.
///
/// `Num_Mapping` is little-endian: upstream copies it with a plain assignment and no `htons`
/// (`em_configuration.cpp:1674`, audit row 19; note 0008, D6). `Expected_Duration` is big-endian
/// (§3.1.2); the TID-to-Link entries are raw octets.
///
/// With `is_bSTA_Config` 1 and `Num_Mapping` not 0 the strict decoder returns
/// [`DecodeError::ValueOutOfRange`] with the count (note 0009, A8). Upstream does not force
/// `Num_Mapping` to 0 for a bSTA MLD (`em_configuration.cpp:1674`, audit row 22). The count is
/// checked before any mapping is read.
///
/// Intentional difference: upstream writes every mapping as a fixed 19-octet record and never
/// writes the TID-to-Link entries (`//TODO: tid_to_link_map`, `em_configuration.cpp:1687-1690`,
/// audit row 18), so a mapping with a nonzero presence indicator does not parse as spec bytes.
/// There is no lenient form for it (user decision): the strict decoder reads the entries the
/// indicator announces and fails with [`DecodeError::Truncated`] or misreads the next record.
#[derive(Debug, PartialEq, Eq)]
pub struct TidToLinkMappingPolicy {
    /// MAC address of the AP MLD or bSTA MLD to which the policy applies.
    pub mld_mac: MacAddr,
    /// `TID-To-Link_Mapping_Negotiation` (bit 7): whether TID-to-link mapping negotiation is
    /// (to be) enabled on the MLD.
    pub negotiation_enabled: bool,
    /// The kind of MLD, with its mappings.
    pub scope: TidToLinkScope,
}

impl TlvBody<'_> for TidToLinkMappingPolicy {
    const TLV_TYPE: TlvType = TlvType::TID_TO_LINK_MAPPING_POLICY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let header = reader.u8()?;
        let mld_mac = reader.mac()?;
        let negotiation = reader.u8()?;
        reader.skip(TTLM_HEADER_RESERVED_OCTETS)?;
        let mapping_count = reader.le_u16()?;
        let scope = if header & IS_BSTA_CONFIG == 0 {
            // A mapping is at least the fixed 19 octets.
            let mut mappings = bounded_vec(usize::from(mapping_count), reader.remaining(), 19);
            for _ in 0..mapping_count {
                mappings.push(read_mapping(&mut reader)?);
            }
            TidToLinkScope::ApMld { mappings }
        } else if mapping_count == 0 {
            TidToLinkScope::BstaMld
        } else {
            return Err(DecodeError::ValueOutOfRange {
                value: u32::from(mapping_count),
            });
        };
        reader.finish()?;
        Ok(Self {
            mld_mac,
            negotiation_enabled: negotiation & MAPPING_NEGOTIATION != 0,
            scope,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mappings = self.scope.mappings();
        let Ok(mapping_count) = u16::try_from(mappings.len()) else {
            return Err(EncodeError::CountTooLarge {
                len: mappings.len(),
                max: usize::from(u16::MAX),
            });
        };
        let mut writer = Writer::new();
        writer.u8(match self.scope {
            TidToLinkScope::ApMld { .. } => 0,
            TidToLinkScope::BstaMld => IS_BSTA_CONFIG,
        });
        writer.mac(self.mld_mac);
        writer.u8(flag_bit(self.negotiation_enabled, MAPPING_NEGOTIATION));
        writer.zeros(TTLM_HEADER_RESERVED_OCTETS);
        writer.le_u16(mapping_count);
        for mapping in mappings {
            write_mapping(&mut writer, mapping)?;
        }
        Ok(writer.into_bytes())
    }
}

/// Reads one TID-to-Link mapping.
fn read_mapping(reader: &mut Reader<'_>) -> Result<TidToLinkMapping, DecodeError> {
    let add_octet = reader.u8()?;
    let sta_mld_mac = reader.mac()?;
    let control = reader.u8()?;
    let link_mapping_presence = reader.u8()?;
    let expected_duration = reader.be_u24()?;
    let entry_count = link_mapping_presence.count_ones() as usize;
    let mut tid_to_link = bounded_vec(entry_count, reader.remaining(), 2);
    for _ in 0..entry_count {
        tid_to_link.push(reader.array::<2>()?);
    }
    reader.skip(TTLM_MAPPING_RESERVED_OCTETS)?;
    Ok(TidToLinkMapping {
        add: add_octet & ADD_MAPPING != 0,
        sta_mld_mac,
        direction: control >> DIRECTION_SHIFT,
        default_link_mapping: control & DEFAULT_LINK_MAPPING != 0,
        mapping_switch_time_present: control & MAPPING_SWITCH_TIME_PRESENT != 0,
        expected_duration_present: control & EXPECTED_DURATION_PRESENT != 0,
        link_mapping_size: control & LINK_MAPPING_SIZE != 0,
        link_mapping_presence,
        expected_duration,
        tid_to_link,
    })
}

/// Writes one TID-to-Link mapping, if the strict decoder would accept it.
fn write_mapping(writer: &mut Writer, mapping: &TidToLinkMapping) -> Result<(), EncodeError> {
    let direction = two_bit_value(mapping.direction)?;
    let expected_entries = mapping.link_mapping_presence.count_ones() as usize;
    let entries = mapping.tid_to_link.len();
    if entries > expected_entries {
        return Err(EncodeError::CountTooLarge {
            len: entries,
            max: expected_entries,
        });
    }
    if entries < expected_entries {
        return Err(EncodeError::CountTooSmall {
            len: entries,
            min: expected_entries,
        });
    }
    writer.u8(flag_bit(mapping.add, ADD_MAPPING));
    writer.mac(mapping.sta_mld_mac);
    writer.u8((direction << DIRECTION_SHIFT)
        | flag_bit(mapping.default_link_mapping, DEFAULT_LINK_MAPPING)
        | flag_bit(
            mapping.mapping_switch_time_present,
            MAPPING_SWITCH_TIME_PRESENT,
        )
        | flag_bit(mapping.expected_duration_present, EXPECTED_DURATION_PRESENT)
        | flag_bit(mapping.link_mapping_size, LINK_MAPPING_SIZE));
    writer.u8(mapping.link_mapping_presence);
    writer.be_u24(mapping.expected_duration)?;
    for entry in &mapping.tid_to_link {
        writer.bytes(entry);
    }
    writer.zeros(TTLM_MAPPING_RESERVED_OCTETS);
    Ok(())
}

/// One BSS of a radio in [`EhtOperations`].
#[allow(clippy::struct_excessive_bools)] // Independent wire flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EhtOperationsBss {
    /// BSSID of the BSS.
    pub bssid: MacAddr,
    /// `EHT_Operation_Information_Valid` (bit 7): whether `control`, `ccfs0` and `ccfs1` are
    /// valid. They are always on the wire and are kept as found.
    pub operation_information_valid: bool,
    /// `Disabled_Subchannel_Valid` (bit 6): whether `disabled_subchannel_bitmap` is valid. It
    /// is always on the wire and is kept as found.
    pub disabled_subchannel_valid: bool,
    /// `EHT Default PE Duration` (bit 5): bit 2 of the EHT Operation Parameters field.
    pub default_pe_duration: bool,
    /// `Group Addressed BU Indication Limit` (bit 4): bit 3 of the EHT Operation Parameters
    /// field.
    pub group_addressed_bu_indication_limit: bool,
    /// `Group Addressed BU Indication Exponent` (bits 3-2), 0 to 3: bits 4-5 of the EHT
    /// Operation Parameters field. A value above 3 does not encode:
    /// [`EncodeError::ValueTooLarge`].
    pub group_addressed_bu_indication_exponent: u8,
    /// `Basic EHT-MCS And Nss Set`, 4 raw octets of the EHT Operation element (IEEE 802.11).
    pub basic_eht_mcs_and_nss_set: [u8; 4],
    /// `Control` of the EHT Operation Information field, raw.
    pub control: u8,
    /// `CCFS0` of the EHT Operation Information field, raw.
    pub ccfs0: u8,
    /// `CCFS1` of the EHT Operation Information field, raw.
    pub ccfs1: u8,
    /// `Disabled_Subchannel_Bitmap`, 2 raw octets of the EHT Operation Information field.
    pub disabled_subchannel_bitmap: [u8; 2],
}

/// One radio in [`EhtOperations`].
#[derive(Debug, PartialEq, Eq)]
pub struct EhtOperationsRadio {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// The BSSs for which configuration information is provided, in wire order.
    pub bsses: Vec<EhtOperationsBss>,
}

/// EHT Operations, `EasyMesh` §17.2.103 Table 125; audit row 296.
///
/// Upstream sender `create_eht_operations_tlv` (`em.cpp:2009-2068`), receiver
/// `handle_eht_operations_tlv` (`em.cpp:2140-2243`). The value is 32 reserved octets, the radio
/// count and, per radio, the RUID, the BSS count, the BSS records and 25 reserved octets. A BSS
/// record is the BSSID, a flags octet, the 4-octet Basic EHT-MCS And Nss Set, Control, CCFS0,
/// CCFS1, the 2-octet Disabled Subchannel Bitmap and 16 reserved octets. Reserved bits and
/// blocks are ignored on decode and written as zero. The valid flags never change the layout.
///
/// Intentional difference: upstream's receiver rejects the whole TLV with more than
/// `EM_MAX_RADIO_PER_AGENT` (4) radios or more than `EM_MAX_BSS_PER_RADIO` (16) BSSs in a radio
/// (`em.cpp:2163-2166` and `em.cpp:2183-2186`). That is a storage limit of its data model, not a wire rule, so this
/// body accepts every count from 0 to 255.
#[derive(Debug, PartialEq, Eq)]
pub struct EhtOperations {
    /// The radios for which configuration information is provided, in wire order.
    pub radios: Vec<EhtOperationsRadio>,
}

impl TlvBody<'_> for EhtOperations {
    const TLV_TYPE: TlvType = TlvType::EHT_OPERATIONS;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        reader.skip(EHT_HEADER_RESERVED_OCTETS)?;
        let radio_count = usize::from(reader.u8()?);
        // A radio is at least the RUID, the BSS count and its reserved block.
        let mut radios = bounded_vec(radio_count, reader.remaining(), 32);
        for _ in 0..radio_count {
            let ruid = reader.mac()?;
            let bss_count = usize::from(reader.u8()?);
            let mut bsses = bounded_vec(bss_count, reader.remaining(), 32);
            for _ in 0..bss_count {
                bsses.push(read_eht_bss(&mut reader)?);
            }
            reader.skip(EHT_RADIO_RESERVED_OCTETS)?;
            radios.push(EhtOperationsRadio { ruid, bsses });
        }
        reader.finish()?;
        Ok(Self { radios })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.zeros(EHT_HEADER_RESERVED_OCTETS);
        writer.count_u8(self.radios.len())?;
        for radio in &self.radios {
            writer.mac(radio.ruid);
            writer.count_u8(radio.bsses.len())?;
            for bss in &radio.bsses {
                write_eht_bss(&mut writer, bss)?;
            }
            writer.zeros(EHT_RADIO_RESERVED_OCTETS);
        }
        Ok(writer.into_bytes())
    }
}

/// Reads one BSS record of the EHT Operations.
fn read_eht_bss(reader: &mut Reader<'_>) -> Result<EhtOperationsBss, DecodeError> {
    let bssid = reader.mac()?;
    let flags = reader.u8()?;
    let basic_eht_mcs_and_nss_set = reader.array::<4>()?;
    let control = reader.u8()?;
    let ccfs0 = reader.u8()?;
    let ccfs1 = reader.u8()?;
    let disabled_subchannel_bitmap = reader.array::<2>()?;
    reader.skip(EHT_BSS_RESERVED_OCTETS)?;
    Ok(EhtOperationsBss {
        bssid,
        operation_information_valid: flags & EHT_OPERATION_INFORMATION_VALID != 0,
        disabled_subchannel_valid: flags & DISABLED_SUBCHANNEL_VALID != 0,
        default_pe_duration: flags & EHT_DEFAULT_PE_DURATION != 0,
        group_addressed_bu_indication_limit: flags & GROUP_ADDRESSED_BU_INDICATION_LIMIT != 0,
        group_addressed_bu_indication_exponent: (flags >> BU_EXPONENT_SHIFT) & MAX_TWO_BIT_VALUE,
        basic_eht_mcs_and_nss_set,
        control,
        ccfs0,
        ccfs1,
        disabled_subchannel_bitmap,
    })
}

/// Writes one BSS record of the EHT Operations.
fn write_eht_bss(writer: &mut Writer, bss: &EhtOperationsBss) -> Result<(), EncodeError> {
    let exponent = two_bit_value(bss.group_addressed_bu_indication_exponent)?;
    writer.mac(bss.bssid);
    writer.u8(flag_bit(
        bss.operation_information_valid,
        EHT_OPERATION_INFORMATION_VALID,
    ) | flag_bit(bss.disabled_subchannel_valid, DISABLED_SUBCHANNEL_VALID)
        | flag_bit(bss.default_pe_duration, EHT_DEFAULT_PE_DURATION)
        | flag_bit(
            bss.group_addressed_bu_indication_limit,
            GROUP_ADDRESSED_BU_INDICATION_LIMIT,
        )
        | (exponent << BU_EXPONENT_SHIFT));
    writer.bytes(&bss.basic_eht_mcs_and_nss_set);
    writer.u8(bss.control);
    writer.u8(bss.ccfs0);
    writer.u8(bss.ccfs1);
    writer.bytes(&bss.disabled_subchannel_bitmap);
    writer.zeros(EHT_BSS_RESERVED_OCTETS);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, sweep_row, sweep_row_lenient, truncated_in};
    use core::mem::size_of;

    const GROUP: &str = "mld";
    const TOO_MANY: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    const ZERO_MAC: MacAddr = MacAddr::new([0; 6]);
    const RUID0: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe0]);
    const RUID1: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf0]);
    const BSSID_E1: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe1]);
    const BSSID_E2: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe2]);
    const BSSID_F1: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xf1]);
    const MLD: MacAddr = MacAddr::new([0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f]);
    const AFFILIATED_60: MacAddr = MacAddr::new([0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x60]);
    const AFFILIATED_61: MacAddr = MacAddr::new([0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x61]);
    const STA_21: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]);
    const STA_22: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x22]);
    const STA_23: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x23]);
    /// Invalid rows that a lenient decoder accepts; the only lenient form is the empty value,
    /// which is a kind=lenient row.
    const LENIENT_INPUTS: &[Key] = &[];
    const CANONICAL: &[(Key, Key)] = &[
        (("0xb7", "reserved-set"), ("0xb7", "flags-none")),
        (("0xb7", "reserved-set-with-flags"), ("0xb7", "flags-mixed")),
        (("0xe0", "reserved-set"), ("0xe0", "reserved-clear")),
        (("0xe1", "reserved-set"), ("0xe1", "reserved-clear")),
        (("0xe2", "reserved-set"), ("0xe2", "reserved-clear")),
        (("0xe3", "reserved-set"), ("0xe3", "reserved-clear")),
        (("0xe4", "expansion-one-octet"), ("0xe4", "golden")),
        (("0xe4", "expansion-seven-octets"), ("0xe4", "golden")),
        (("0xe5", "expansion-one-octet"), ("0xe5", "golden")),
        (("0xe5", "expansion-seven-octets"), ("0xe5", "golden")),
        (("0xe6", "reserved-set"), ("0xe6", "reserved-clear")),
        (
            ("0xe6", "reserved-set-bsta-config"),
            ("0xe6", "bsta-config-zero-mappings"),
        ),
        (("0xe7", "reserved-set"), ("0xe7", "reserved-clear")),
    ];

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    /// The fixture bytes with a `'static` lifetime, for the raw-value bodies that borrow them.
    /// The test process frees them when it ends; a fixture is a few bytes.
    fn static_fixture(key: Key) -> &'static [u8] {
        Box::leak(fixture(key).into_boxed_slice())
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0xb7" => sweep_row::<BssConfigurationReport<'_>>(row, bytes, canonical),
            "0xbb" => sweep_row::<BssConfigurationRequest<'_>>(row, bytes, canonical),
            "0xbd" => sweep_row::<BssConfigurationResponse<'_>>(row, bytes, canonical),
            "0xe0" => sweep_row::<AgentApMldConfiguration<'_>>(row, bytes, canonical),
            "0xe1" => sweep_row_lenient::<BackhaulStaMldConfiguration>(
                row,
                bytes,
                canonical,
                LENIENT_INPUTS,
            ),
            "0xe2" => sweep_row::<AssociatedStaMldConfigurationReport>(row, bytes, canonical),
            "0xe3" => sweep_row::<MldStructure>(row, bytes, canonical),
            "0xe4" => sweep_row::<AffiliatedStaMetrics>(row, bytes, canonical),
            "0xe5" => sweep_row::<AffiliatedApMetrics>(row, bytes, canonical),
            "0xe6" => sweep_row::<TidToLinkMappingPolicy>(row, bytes, canonical),
            "0xe7" => sweep_row::<EhtOperations>(row, bytes, canonical),
            other => Err(format!("no body for TLV {other}")),
        }
    }

    #[test]
    fn every_fixture_row_decodes_and_encodes_as_its_kind_says() {
        crate::body_testkit::sweep_group(GROUP, CANONICAL, dispatch_row);
    }

    /// The expected error of each prefix of a value that is a plain sequence of fields of the
    /// given widths.
    fn truncated_at_widths(widths: &[usize], prefix_len: usize) -> DecodeError {
        let mut fields = Vec::new();
        let mut offset = 0;
        for &width in widths {
            fields.push(Field { offset, width });
            offset += width;
        }
        truncated_in(&fields)(prefix_len)
    }

    const fn field(offset: usize, width: usize) -> Field {
        Field { offset, width }
    }

    // ---- 0xB7 BSS Configuration Report ----

    fn mesh_bss() -> BssConfigurationReportBss<'static> {
        BssConfigurationReportBss {
            bssid: BSSID_E1,
            backhaul_not_in_use: false,
            fronthaul_not_in_use: false,
            r1_disallowed: false,
            r2_disallowed: false,
            multiple_bssid: false,
            transmitted_bssid: false,
            ssid: b"Mesh5G!",
        }
    }

    fn report_of(bss: BssConfigurationReportBss<'_>) -> BssConfigurationReport<'_> {
        BssConfigurationReport {
            radios: vec![BssConfigurationReportRadio {
                ruid: RUID0,
                bsses: vec![bss],
            }],
        }
    }

    fn report_golden() -> BssConfigurationReport<'static> {
        BssConfigurationReport {
            radios: vec![
                BssConfigurationReportRadio {
                    ruid: RUID0,
                    bsses: vec![
                        BssConfigurationReportBss {
                            bssid: BSSID_E1,
                            backhaul_not_in_use: true,
                            transmitted_bssid: true,
                            ssid: b"HomeNet-2G",
                            ..mesh_bss()
                        },
                        BssConfigurationReportBss {
                            bssid: BSSID_E2,
                            fronthaul_not_in_use: true,
                            r1_disallowed: true,
                            multiple_bssid: true,
                            ssid: b"Guest",
                            ..mesh_bss()
                        },
                    ],
                },
                BssConfigurationReportRadio {
                    ruid: RUID1,
                    bsses: vec![BssConfigurationReportBss {
                        bssid: BSSID_F1,
                        r2_disallowed: true,
                        ..mesh_bss()
                    }],
                },
            ],
        }
    }

    fn report_widths() -> Vec<usize> {
        [
            &[1, 6, 1][..],
            &[6, 1, 1, 1, 10],
            &[6, 1, 1, 1, 5],
            &[6, 1],
            &[6, 1, 1, 1, 7],
        ]
        .concat()
    }

    fn report_too_many_bsses() -> BssConfigurationReport<'static> {
        BssConfigurationReport {
            radios: vec![BssConfigurationReportRadio {
                ruid: RUID0,
                bsses: vec![mesh_bss(); 256],
            }],
        }
    }

    fn report_too_many_radios() -> BssConfigurationReport<'static> {
        BssConfigurationReport {
            radios: (0..256)
                .map(|_| BssConfigurationReportRadio {
                    ruid: RUID0,
                    bsses: Vec::new(),
                })
                .collect(),
        }
    }

    // ---- 0xBB and 0xBD ----

    fn request_golden() -> BssConfigurationRequest<'static> {
        BssConfigurationRequest {
            value: static_fixture(("0xbb", "golden")),
        }
    }

    fn response_golden() -> BssConfigurationResponse<'static> {
        BssConfigurationResponse {
            value: static_fixture(("0xbd", "golden")),
        }
    }

    // ---- 0xE0 Agent AP MLD Configuration ----

    fn affiliated_ap(mac_valid: bool, link_id_valid: bool, link_id: u8) -> AffiliatedAp {
        AffiliatedAp {
            mac_valid,
            link_id_valid,
            ruid: RUID0,
            mac: AFFILIATED_60,
            link_id,
        }
    }

    fn ap_mld_golden() -> AgentApMldConfiguration<'static> {
        AgentApMldConfiguration {
            ap_mlds: vec![
                ApMld {
                    mac_valid: true,
                    ssid: b"MeshHome-7G",
                    mac: MLD,
                    modes: MldModes {
                        str_enabled: true,
                        nstr_enabled: false,
                        emlsr_enabled: true,
                        emlmr_enabled: false,
                    },
                    affiliated_aps: vec![
                        affiliated_ap(true, true, 3),
                        AffiliatedAp {
                            mac_valid: true,
                            link_id_valid: false,
                            ruid: RUID1,
                            mac: AFFILIATED_61,
                            link_id: 0,
                        },
                    ],
                },
                ApMld {
                    mac_valid: false,
                    ssid: b"Guest",
                    mac: ZERO_MAC,
                    modes: MldModes {
                        str_enabled: false,
                        nstr_enabled: true,
                        emlsr_enabled: false,
                        emlmr_enabled: false,
                    },
                    affiliated_aps: vec![AffiliatedAp {
                        mac_valid: false,
                        link_id_valid: true,
                        ruid: RUID1,
                        mac: ZERO_MAC,
                        link_id: 9,
                    }],
                },
            ],
        }
    }

    /// The AP MLD of the `reserved-*` rows of 0xE0.
    fn ap_mld_single() -> AgentApMldConfiguration<'static> {
        AgentApMldConfiguration {
            ap_mlds: vec![ApMld {
                mac_valid: true,
                ssid: b"MeshHome-7G",
                mac: MLD,
                modes: MldModes {
                    str_enabled: true,
                    nstr_enabled: false,
                    emlsr_enabled: true,
                    emlmr_enabled: false,
                },
                affiliated_aps: vec![affiliated_ap(true, true, 3)],
            }],
        }
    }

    fn ap_mld_widths() -> Vec<usize> {
        [
            &[1][..],
            &[1, 1, 11, 6, 1, 20, 1],
            &[1, 6, 6, 1, 18],
            &[1, 6, 6, 1, 18],
            &[1, 1, 5, 6, 1, 20, 1],
            &[1, 6, 6, 1, 18],
        ]
        .concat()
    }

    fn empty_ap_mld() -> ApMld<'static> {
        ApMld {
            mac_valid: false,
            ssid: b"",
            mac: ZERO_MAC,
            modes: no_modes(),
            affiliated_aps: Vec::new(),
        }
    }

    fn no_modes() -> MldModes {
        MldModes {
            str_enabled: false,
            nstr_enabled: false,
            emlsr_enabled: false,
            emlmr_enabled: false,
        }
    }

    // ---- 0xE1 Backhaul STA MLD Configuration ----

    fn bsta_golden() -> BackhaulStaMldConfiguration {
        BackhaulStaMldConfiguration {
            bsta_mld_mac_valid: true,
            ap_mld_mac_valid: true,
            bsta_mld_mac: STA_21,
            ap_mld_mac: MLD,
            modes: MldModes {
                str_enabled: true,
                nstr_enabled: true,
                ..no_modes()
            },
            affiliated_bstas: vec![
                AffiliatedBsta {
                    mac_valid: true,
                    ruid: RUID0,
                    mac: AFFILIATED_60,
                },
                AffiliatedBsta {
                    mac_valid: false,
                    ruid: RUID1,
                    mac: ZERO_MAC,
                },
            ],
        }
    }

    /// The bSTA MLD of the `reserved-*` rows of 0xE1.
    fn bsta_single() -> BackhaulStaMldConfiguration {
        BackhaulStaMldConfiguration {
            affiliated_bstas: vec![AffiliatedBsta {
                mac_valid: true,
                ruid: RUID0,
                mac: AFFILIATED_60,
            }],
            ..bsta_golden()
        }
    }

    fn bsta_widths() -> Vec<usize> {
        [&[1, 6, 6, 1, 17, 1][..], &[1, 6, 6, 19], &[1, 6, 6, 19]].concat()
    }

    // ---- 0xE2 Associated STA MLD Configuration Report ----

    fn sta_mld_golden() -> AssociatedStaMldConfigurationReport {
        AssociatedStaMldConfigurationReport {
            sta_mld_mac: STA_21,
            ap_mld_mac: MLD,
            modes: MldModes {
                str_enabled: true,
                emlmr_enabled: true,
                ..no_modes()
            },
            affiliated_stas: vec![
                AffiliatedSta {
                    bssid: BSSID_E1,
                    mac: STA_22,
                },
                AffiliatedSta {
                    bssid: BSSID_F1,
                    mac: STA_23,
                },
            ],
        }
    }

    /// The report of the `reserved-*` rows of 0xE2.
    fn sta_mld_single() -> AssociatedStaMldConfigurationReport {
        AssociatedStaMldConfigurationReport {
            affiliated_stas: vec![AffiliatedSta {
                bssid: BSSID_E1,
                mac: STA_22,
            }],
            ..sta_mld_golden()
        }
    }

    fn sta_mld_widths() -> Vec<usize> {
        [&[6, 6, 1, 18, 1][..], &[6, 6, 19], &[6, 6, 19]].concat()
    }

    // ---- 0xE3 MLD Structure ----

    fn mld_structure_golden() -> MldStructure {
        MldStructure {
            mld_mac: MLD,
            affiliated_macs: vec![BSSID_E1, BSSID_F1, STA_22],
        }
    }

    fn mld_structure_widths() -> Vec<usize> {
        [&[6, 25, 1][..], &[6, 26], &[6, 26], &[6, 26]].concat()
    }

    // ---- 0xE4 and 0xE5 ----

    fn sta_metrics_golden() -> AffiliatedStaMetrics {
        AffiliatedStaMetrics {
            sta_mac: STA_21,
            bytes_sent: 0x0102_0304,
            bytes_received: 0x1122_3344,
            packets_sent: 0x0a0b_0c0d,
            packets_received: 0x0001_0203,
            packets_sent_errors: 0xf0e0_d0c0,
        }
    }

    fn ap_metrics_golden() -> AffiliatedApMetrics {
        AffiliatedApMetrics {
            bssid: BSSID_E1,
            packets_sent: 0x0102_0304,
            packets_received: 0x1122_3344,
            packet_sent_errors: 0x0a0b_0c0d,
            unicast_bytes_sent: 0x0001_0203,
            unicast_bytes_received: 0xf0e0_d0c0,
            multicast_bytes_sent: 0x2122_2324,
            multicast_bytes_received: 0x3132_3334,
            broadcast_bytes_sent: 0x4142_4344,
            broadcast_bytes_received: 0x5152_5354,
        }
    }

    // ---- 0xE6 TID-to-Link Mapping Policy ----

    fn zero_mapping() -> TidToLinkMapping {
        TidToLinkMapping {
            add: false,
            sta_mld_mac: ZERO_MAC,
            direction: 0,
            default_link_mapping: false,
            mapping_switch_time_present: false,
            expected_duration_present: false,
            link_mapping_size: false,
            link_mapping_presence: 0,
            expected_duration: 0,
            tid_to_link: Vec::new(),
        }
    }

    fn policy_golden() -> TidToLinkMappingPolicy {
        TidToLinkMappingPolicy {
            mld_mac: MLD,
            negotiation_enabled: true,
            scope: TidToLinkScope::ApMld {
                mappings: vec![
                    TidToLinkMapping {
                        add: true,
                        sta_mld_mac: STA_21,
                        direction: 1,
                        mapping_switch_time_present: true,
                        expected_duration_present: true,
                        link_mapping_presence: 0x05,
                        expected_duration: 0x01_2345,
                        tid_to_link: vec![[0xa5, 0x0f], [0x3c, 0x01]],
                        ..zero_mapping()
                    },
                    TidToLinkMapping {
                        sta_mld_mac: STA_22,
                        direction: 2,
                        default_link_mapping: true,
                        link_mapping_size: true,
                        link_mapping_presence: 0x80,
                        tid_to_link: vec![[0x00, 0xff]],
                        ..zero_mapping()
                    },
                ],
            },
        }
    }

    fn policy_with(mapping: TidToLinkMapping) -> TidToLinkMappingPolicy {
        TidToLinkMappingPolicy {
            mld_mac: MLD,
            negotiation_enabled: false,
            scope: TidToLinkScope::ApMld {
                mappings: vec![mapping],
            },
        }
    }

    /// The policy of the `reserved-*` rows of 0xE6.
    fn policy_reserved_row() -> TidToLinkMappingPolicy {
        policy_with(TidToLinkMapping {
            add: true,
            sta_mld_mac: STA_21,
            direction: 1,
            link_mapping_presence: 0x01,
            tid_to_link: vec![[0x12, 0x34]],
            ..zero_mapping()
        })
    }

    fn bsta_policy(negotiation_enabled: bool) -> TidToLinkMappingPolicy {
        TidToLinkMappingPolicy {
            mld_mac: MLD,
            negotiation_enabled,
            scope: TidToLinkScope::BstaMld,
        }
    }

    fn policy_widths() -> Vec<usize> {
        [
            &[1, 6, 1, 22, 2][..],
            &[1, 6, 1, 1, 3, 2, 2, 7],
            &[1, 6, 1, 1, 3, 2, 7],
        ]
        .concat()
    }

    fn policy_too_many_mappings() -> TidToLinkMappingPolicy {
        TidToLinkMappingPolicy {
            mld_mac: MLD,
            negotiation_enabled: false,
            scope: TidToLinkScope::ApMld {
                mappings: (0..=usize::from(u16::MAX))
                    .map(|_| zero_mapping())
                    .collect(),
            },
        }
    }

    // ---- 0xE7 EHT Operations ----

    fn eht_bss() -> EhtOperationsBss {
        EhtOperationsBss {
            bssid: BSSID_E1,
            operation_information_valid: false,
            disabled_subchannel_valid: false,
            default_pe_duration: false,
            group_addressed_bu_indication_limit: false,
            group_addressed_bu_indication_exponent: 0,
            basic_eht_mcs_and_nss_set: [0xf1, 0xa2, 0x0b, 0x3c],
            control: 0,
            ccfs0: 0,
            ccfs1: 0,
            disabled_subchannel_bitmap: [0, 0],
        }
    }

    fn eht_golden() -> EhtOperations {
        EhtOperations {
            radios: vec![
                EhtOperationsRadio {
                    ruid: RUID0,
                    bsses: vec![
                        EhtOperationsBss {
                            operation_information_valid: true,
                            default_pe_duration: true,
                            group_addressed_bu_indication_exponent: 1,
                            control: 0x15,
                            ccfs0: 42,
                            ccfs1: 155,
                            disabled_subchannel_bitmap: [0x0f, 0xf0],
                            ..eht_bss()
                        },
                        EhtOperationsBss {
                            bssid: BSSID_E2,
                            disabled_subchannel_valid: true,
                            group_addressed_bu_indication_limit: true,
                            group_addressed_bu_indication_exponent: 2,
                            basic_eht_mcs_and_nss_set: [0x11, 0x22, 0x33, 0x44],
                            control: 0x02,
                            disabled_subchannel_bitmap: [0x80, 0x01],
                            ..eht_bss()
                        },
                    ],
                },
                EhtOperationsRadio {
                    ruid: RUID1,
                    bsses: vec![EhtOperationsBss {
                        bssid: BSSID_F1,
                        operation_information_valid: true,
                        disabled_subchannel_valid: true,
                        default_pe_duration: true,
                        group_addressed_bu_indication_limit: true,
                        group_addressed_bu_indication_exponent: 3,
                        basic_eht_mcs_and_nss_set: [0x04, 0x03, 0x02, 0x01],
                        control: 0xf0,
                        ccfs0: 7,
                        ccfs1: 200,
                        disabled_subchannel_bitmap: [0x12, 0x34],
                    }],
                },
            ],
        }
    }

    /// The radio of the `reserved-*` rows of 0xE7.
    fn eht_single() -> EhtOperations {
        EhtOperations {
            radios: vec![EhtOperationsRadio {
                ruid: RUID0,
                bsses: vec![EhtOperationsBss {
                    operation_information_valid: true,
                    group_addressed_bu_indication_exponent: 1,
                    ..eht_bss()
                }],
            }],
        }
    }

    fn eht_widths() -> Vec<usize> {
        let bss = [6, 1, 4, 1, 1, 1, 2, 16];
        [&[32, 1, 6, 1][..], &bss, &bss, &[25, 6, 1], &bss, &[25]].concat()
    }

    fn eht_too_many_bsses() -> EhtOperations {
        EhtOperations {
            radios: vec![EhtOperationsRadio {
                ruid: RUID0,
                bsses: vec![eht_bss(); 256],
            }],
        }
    }

    fn eht_too_many_radios() -> EhtOperations {
        EhtOperations {
            radios: (0..256)
                .map(|_| EhtOperationsRadio {
                    ruid: RUID0,
                    bsses: Vec::new(),
                })
                .collect(),
        }
    }

    body_checks! {
        mod report_checks;
        body: BssConfigurationReport<'a>;
        golden: &fixture(("0xb7", "golden")), report_golden();
        prefixes: |_| false, |len| truncated_at_widths(&report_widths(), len);
        trailing_byte: &fixture(("0xb7", "golden"));
        count_too_large: report_too_many_radios(), TOO_MANY;
        reserved_bits: [
            (&fixture(("0xb7", "reserved-set")), report_of(mesh_bss()), &fixture(("0xb7", "flags-none"))),
            (&fixture(("0xb7", "reserved-set-with-flags")), report_of(BssConfigurationReportBss { backhaul_not_in_use: true, r1_disallowed: true, transmitted_bssid: true, ..mesh_bss() }), &fixture(("0xb7", "flags-mixed")))
        ];
        alternates: [
            (&fixture(("0xb7", "zero-radios")), BssConfigurationReport { radios: vec![] }, &fixture(("0xb7", "zero-radios"))),
            (&fixture(("0xb7", "zero-bss")), BssConfigurationReport { radios: vec![BssConfigurationReportRadio { ruid: RUID0, bsses: vec![] }] }, &fixture(("0xb7", "zero-bss"))),
            (&fixture(("0xb7", "ssid-empty")), report_of(BssConfigurationReportBss { ssid: b"", ..mesh_bss() }), &fixture(("0xb7", "ssid-empty")))
        ];
    }
    body_checks! {
        mod request_checks;
        body: BssConfigurationRequest<'a>;
        golden: &fixture(("0xbb", "golden")), request_golden();
        prefixes: |len| len > 0, truncated_in(&[field(0, 1)]);
        alternates: [(&fixture(("0xbb", "one-octet")), BssConfigurationRequest { value: &[0x7b] }, &fixture(("0xbb", "one-octet")))];
    }
    body_checks! {
        mod response_checks;
        body: BssConfigurationResponse<'a>;
        golden: &fixture(("0xbd", "golden")), response_golden();
        prefixes: |len| len > 0, truncated_in(&[field(0, 1)]);
        alternates: [(&fixture(("0xbd", "one-octet")), BssConfigurationResponse { value: &[0x7b] }, &fixture(("0xbd", "one-octet")))];
    }
    body_checks! {
        mod ap_mld_checks;
        body: AgentApMldConfiguration<'a>;
        golden: &fixture(("0xe0", "golden")), ap_mld_golden();
        prefixes: |_| false, |len| truncated_at_widths(&ap_mld_widths(), len);
        trailing_byte: &fixture(("0xe0", "golden"));
        count_too_large: AgentApMldConfiguration { ap_mlds: (0..256).map(|_| empty_ap_mld()).collect() }, TOO_MANY;
        reserved_bits: [(&fixture(("0xe0", "reserved-set")), ap_mld_single(), &fixture(("0xe0", "reserved-clear")))];
        alternates: [
            (&fixture(("0xe0", "zero-apmld")), AgentApMldConfiguration { ap_mlds: vec![] }, &fixture(("0xe0", "zero-apmld"))),
            (&fixture(("0xe0", "linkid-255")), AgentApMldConfiguration { ap_mlds: vec![ApMld { modes: no_modes(), affiliated_aps: vec![affiliated_ap(true, true, 255)], ..ap_mld_single().ap_mlds.remove(0) }] }, &fixture(("0xe0", "linkid-255")))
        ];
    }
    body_checks! {
        mod bsta_mld_checks;
        body: BackhaulStaMldConfiguration;
        golden: &fixture(("0xe1", "golden")), bsta_golden();
        lenient;
        prefixes: |_| false, |len| truncated_at_widths(&bsta_widths(), len);
        trailing_byte: &fixture(("0xe1", "golden"));
        count_too_large: BackhaulStaMldConfiguration { affiliated_bstas: vec![AffiliatedBsta { mac_valid: false, ruid: ZERO_MAC, mac: ZERO_MAC }; 256], ..bsta_golden() }, TOO_MANY;
        reserved_bits: [(&fixture(("0xe1", "reserved-set")), bsta_single(), &fixture(("0xe1", "reserved-clear")))];
        alternates: [(&fixture(("0xe1", "zero-affiliated")), BackhaulStaMldConfiguration { affiliated_bstas: vec![], ..bsta_golden() }, &fixture(("0xe1", "zero-affiliated")))];
        lenient_absent: [&fixture(("0xe1", "empty-upstream"))];
        lenient_rejects: [(&fixture(("0xe1", "cut-bsta-mac")), DecodeError::Truncated { needed: 6, remaining: 3 })];
    }
    body_checks! {
        mod sta_mld_checks;
        body: AssociatedStaMldConfigurationReport;
        golden: &fixture(("0xe2", "golden")), sta_mld_golden();
        prefixes: |_| false, |len| truncated_at_widths(&sta_mld_widths(), len);
        trailing_byte: &fixture(("0xe2", "golden"));
        count_too_large: AssociatedStaMldConfigurationReport { affiliated_stas: vec![AffiliatedSta { bssid: ZERO_MAC, mac: ZERO_MAC }; 256], ..sta_mld_golden() }, TOO_MANY;
        reserved_bits: [(&fixture(("0xe2", "reserved-set")), sta_mld_single(), &fixture(("0xe2", "reserved-clear")))];
        alternates: [(&fixture(("0xe2", "zero-affiliated")), AssociatedStaMldConfigurationReport { affiliated_stas: vec![], ..sta_mld_golden() }, &fixture(("0xe2", "zero-affiliated")))];
    }
    body_checks! {
        mod mld_structure_checks;
        body: MldStructure;
        golden: &fixture(("0xe3", "golden")), mld_structure_golden();
        prefixes: |_| false, |len| truncated_at_widths(&mld_structure_widths(), len);
        trailing_byte: &fixture(("0xe3", "golden"));
        count_too_large: MldStructure { mld_mac: MLD, affiliated_macs: vec![ZERO_MAC; 256] }, TOO_MANY;
        reserved_bits: [(&fixture(("0xe3", "reserved-set")), MldStructure { mld_mac: MLD, affiliated_macs: vec![BSSID_E1] }, &fixture(("0xe3", "reserved-clear")))];
        alternates: [(&fixture(("0xe3", "zero-affiliated")), MldStructure { mld_mac: MLD, affiliated_macs: vec![] }, &fixture(("0xe3", "zero-affiliated")))];
    }
    body_checks! {
        mod sta_metrics_checks;
        body: AffiliatedStaMetrics;
        golden: &fixture(("0xe4", "golden")), sta_metrics_golden();
        prefixes: |_| false, |len| truncated_at_widths(&[6, 4, 4, 4, 4, 4], len);
        alternates: [
            (&fixture(("0xe4", "expansion-one-octet")), sta_metrics_golden(), &fixture(("0xe4", "golden"))),
            (&fixture(("0xe4", "expansion-seven-octets")), sta_metrics_golden(), &fixture(("0xe4", "golden")))
        ];
    }
    body_checks! {
        mod ap_metrics_checks;
        body: AffiliatedApMetrics;
        golden: &fixture(("0xe5", "golden")), ap_metrics_golden();
        prefixes: |_| false, |len| truncated_at_widths(&[6, 4, 4, 4, 4, 4, 4, 4, 4, 4], len);
        alternates: [
            (&fixture(("0xe5", "expansion-one-octet")), ap_metrics_golden(), &fixture(("0xe5", "golden"))),
            (&fixture(("0xe5", "expansion-seven-octets")), ap_metrics_golden(), &fixture(("0xe5", "golden")))
        ];
    }
    body_checks! {
        mod policy_checks;
        body: TidToLinkMappingPolicy;
        golden: &fixture(("0xe6", "golden")), policy_golden();
        prefixes: |_| false, |len| truncated_at_widths(&policy_widths(), len);
        trailing_byte: &fixture(("0xe6", "golden"));
        count_too_large: policy_too_many_mappings(), EncodeError::CountTooLarge { len: 65536, max: 65535 };
        reserved_bits: [
            (&fixture(("0xe6", "reserved-set")), policy_reserved_row(), &fixture(("0xe6", "reserved-clear"))),
            (&fixture(("0xe6", "reserved-set-bsta-config")), bsta_policy(false), &fixture(("0xe6", "bsta-config-zero-mappings")))
        ];
        alternates: [
            (&fixture(("0xe6", "zero-mappings")), TidToLinkMappingPolicy { mld_mac: MLD, negotiation_enabled: false, scope: TidToLinkScope::ApMld { mappings: vec![] } }, &fixture(("0xe6", "zero-mappings"))),
            (&fixture(("0xe6", "bsta-config-negotiation-zero-mappings")), bsta_policy(true), &fixture(("0xe6", "bsta-config-negotiation-zero-mappings")))
        ];
    }
    body_checks! {
        mod eht_checks;
        body: EhtOperations;
        golden: &fixture(("0xe7", "golden")), eht_golden();
        prefixes: |_| false, |len| truncated_at_widths(&eht_widths(), len);
        trailing_byte: &fixture(("0xe7", "golden"));
        count_too_large: eht_too_many_radios(), TOO_MANY;
        reserved_bits: [(&fixture(("0xe7", "reserved-set")), eht_single(), &fixture(("0xe7", "reserved-clear")))];
        alternates: [
            (&fixture(("0xe7", "zero-radios")), EhtOperations { radios: vec![] }, &fixture(("0xe7", "zero-radios"))),
            (&fixture(("0xe7", "zero-bss")), EhtOperations { radios: vec![EhtOperationsRadio { ruid: RUID0, bsses: vec![] }] }, &fixture(("0xe7", "zero-bss")))
        ];
    }

    /// A wire flag: octet offset in the value, bit mask, and a setter for the matching field.
    type Flag<B> = (usize, u8, fn(&mut B));

    /// Checks that each flag, set alone in an otherwise zero value, lands on its own wire bit
    /// and nowhere else. Fixture rows that set several flags at once cannot tell two flags
    /// apart when the pair is always set or clear together.
    fn assert_each_flag_alone<'a, B>(zero_bytes: &'a [u8], zero_body: fn() -> B, flags: &[Flag<B>])
    where
        B: TlvBody<'a> + core::fmt::Debug + PartialEq,
    {
        assert_eq!(B::decode_value(zero_bytes), Ok(zero_body()));
        for &(offset, mask, set) in flags {
            let mut bytes = zero_bytes.to_vec();
            bytes[offset] |= mask;
            // Leaked so that a body that borrows from the value can outlive this iteration.
            let bytes: &'a [u8] = Box::leak(bytes.into_boxed_slice());
            let mut expected = zero_body();
            set(&mut expected);
            assert_eq!(
                B::decode_value(bytes),
                Ok(expected),
                "decode, octet {offset} mask {mask:#04x}"
            );
            let mut expected = zero_body();
            set(&mut expected);
            assert_eq!(
                expected.encode_value(),
                Ok(bytes.to_vec()),
                "encode, octet {offset} mask {mask:#04x}"
            );
        }
    }

    #[test]
    fn report_flags_each_use_their_own_bit() {
        let zero = [&[1][..], &[0; 6], &[1], &[0; 6], &[0, 0, 0]].concat();
        let flags: &[Flag<BssConfigurationReport<'_>>] = &[
            (14, 0x80, |b| report_bss_mut(b).backhaul_not_in_use = true),
            (14, 0x40, |b| report_bss_mut(b).fronthaul_not_in_use = true),
            (14, 0x20, |b| report_bss_mut(b).r1_disallowed = true),
            (14, 0x10, |b| report_bss_mut(b).r2_disallowed = true),
            (14, 0x08, |b| report_bss_mut(b).multiple_bssid = true),
            (14, 0x04, |b| report_bss_mut(b).transmitted_bssid = true),
        ];
        assert_each_flag_alone(&zero, zero_report, flags);
    }

    fn zero_report() -> BssConfigurationReport<'static> {
        BssConfigurationReport {
            radios: vec![BssConfigurationReportRadio {
                ruid: ZERO_MAC,
                bsses: vec![BssConfigurationReportBss {
                    bssid: ZERO_MAC,
                    ssid: b"",
                    ..mesh_bss()
                }],
            }],
        }
    }

    fn zero_ap_mld_body() -> AgentApMldConfiguration<'static> {
        AgentApMldConfiguration {
            ap_mlds: vec![ApMld {
                affiliated_aps: vec![AffiliatedAp {
                    mac_valid: false,
                    link_id_valid: false,
                    ruid: ZERO_MAC,
                    mac: ZERO_MAC,
                    link_id: 0,
                }],
                ..empty_ap_mld()
            }],
        }
    }

    #[test]
    fn ap_mld_flags_each_use_their_own_bit() {
        let mut zero = vec![1, 0, 0];
        zero.extend([0; 6]);
        zero.push(0);
        zero.extend([0; 20]);
        zero.push(1);
        zero.push(0);
        zero.extend([0; 6 + 6 + 1 + 18]);
        let flags: &[Flag<AgentApMldConfiguration<'_>>] = &[
            (1, 0x80, |b| b.ap_mlds[0].mac_valid = true),
            (9, 0x80, |b| b.ap_mlds[0].modes.str_enabled = true),
            (9, 0x40, |b| b.ap_mlds[0].modes.nstr_enabled = true),
            (9, 0x20, |b| b.ap_mlds[0].modes.emlsr_enabled = true),
            (9, 0x10, |b| b.ap_mlds[0].modes.emlmr_enabled = true),
            (31, 0x80, |b| affiliated_ap_mut(b).mac_valid = true),
            (31, 0x40, |b| affiliated_ap_mut(b).link_id_valid = true),
        ];
        assert_each_flag_alone(&zero, zero_ap_mld_body, flags);
    }

    fn zero_bsta_body() -> BackhaulStaMldConfiguration {
        BackhaulStaMldConfiguration {
            bsta_mld_mac_valid: false,
            ap_mld_mac_valid: false,
            bsta_mld_mac: ZERO_MAC,
            ap_mld_mac: ZERO_MAC,
            modes: no_modes(),
            affiliated_bstas: vec![AffiliatedBsta {
                mac_valid: false,
                ruid: ZERO_MAC,
                mac: ZERO_MAC,
            }],
        }
    }

    #[test]
    fn bsta_mld_flags_each_use_their_own_bit() {
        let mut zero = vec![0; 32];
        zero[31] = 1;
        zero.extend([0; 32]);
        let flags: &[Flag<BackhaulStaMldConfiguration>] = &[
            (0, 0x80, |b| b.bsta_mld_mac_valid = true),
            (0, 0x40, |b| b.ap_mld_mac_valid = true),
            (13, 0x80, |b| b.modes.str_enabled = true),
            (13, 0x40, |b| b.modes.nstr_enabled = true),
            (13, 0x20, |b| b.modes.emlsr_enabled = true),
            (13, 0x10, |b| b.modes.emlmr_enabled = true),
            (32, 0x80, |b| b.affiliated_bstas[0].mac_valid = true),
        ];
        assert_each_flag_alone(&zero, zero_bsta_body, flags);
    }

    #[test]
    fn sta_mld_modes_each_use_their_own_bit() {
        let zero = vec![0; 32];
        let flags: &[Flag<AssociatedStaMldConfigurationReport>] = &[
            (12, 0x80, |b| b.modes.str_enabled = true),
            (12, 0x40, |b| b.modes.nstr_enabled = true),
            (12, 0x20, |b| b.modes.emlsr_enabled = true),
            (12, 0x10, |b| b.modes.emlmr_enabled = true),
        ];
        assert_each_flag_alone(
            &zero,
            || AssociatedStaMldConfigurationReport {
                sta_mld_mac: ZERO_MAC,
                ap_mld_mac: ZERO_MAC,
                modes: no_modes(),
                affiliated_stas: vec![],
            },
            flags,
        );
    }

    #[test]
    fn policy_header_and_negotiation_bits_are_their_own() {
        let zero = vec![0; 32];
        let flags: &[Flag<TidToLinkMappingPolicy>] = &[
            (0, 0x80, |b| b.scope = TidToLinkScope::BstaMld),
            (7, 0x80, |b| b.negotiation_enabled = true),
        ];
        assert_each_flag_alone(
            &zero,
            || TidToLinkMappingPolicy {
                mld_mac: ZERO_MAC,
                negotiation_enabled: false,
                scope: TidToLinkScope::ApMld { mappings: vec![] },
            },
            flags,
        );
    }

    #[test]
    fn policy_mapping_flags_and_direction_each_use_their_own_bits() {
        let mut zero = vec![0; 51];
        zero[30] = 1;
        let flags: &[Flag<TidToLinkMappingPolicy>] = &[
            (32, 0x80, |b| mapping_mut(b).add = true),
            (39, 0x40, |b| mapping_mut(b).direction = 1),
            (39, 0x80, |b| mapping_mut(b).direction = 2),
            (39, 0x20, |b| mapping_mut(b).default_link_mapping = true),
            (39, 0x10, |b| {
                mapping_mut(b).mapping_switch_time_present = true;
            }),
            (39, 0x08, |b| {
                mapping_mut(b).expected_duration_present = true;
            }),
            (39, 0x04, |b| mapping_mut(b).link_mapping_size = true),
        ];
        assert_each_flag_alone(
            &zero,
            || TidToLinkMappingPolicy {
                mld_mac: ZERO_MAC,
                ..policy_with(zero_mapping())
            },
            flags,
        );
    }

    fn report_bss_mut<'r, 'a>(
        report: &'r mut BssConfigurationReport<'a>,
    ) -> &'r mut BssConfigurationReportBss<'a> {
        &mut report.radios[0].bsses[0]
    }

    fn affiliated_ap_mut<'r>(body: &'r mut AgentApMldConfiguration<'_>) -> &'r mut AffiliatedAp {
        &mut body.ap_mlds[0].affiliated_aps[0]
    }

    fn eht_bss_mut(body: &mut EhtOperations) -> &mut EhtOperationsBss {
        &mut body.radios[0].bsses[0]
    }

    fn mapping_mut(policy: &mut TidToLinkMappingPolicy) -> &mut TidToLinkMapping {
        match &mut policy.scope {
            TidToLinkScope::ApMld { mappings } => &mut mappings[0],
            TidToLinkScope::BstaMld => panic!("a bSTA MLD policy has no mappings"),
        }
    }

    #[test]
    fn eht_flags_and_exponent_each_use_their_own_bits() {
        let mut zero = vec![0; 32];
        zero.push(1);
        zero.extend([0; 6]);
        zero.push(1);
        zero.extend([0; 6 + 1 + 4 + 1 + 1 + 1 + 2 + 16 + 25]);
        let flags: &[Flag<EhtOperations>] = &[
            (46, 0x80, |b| {
                eht_bss_mut(b).operation_information_valid = true;
            }),
            (46, 0x40, |b| {
                eht_bss_mut(b).disabled_subchannel_valid = true;
            }),
            (46, 0x20, |b| eht_bss_mut(b).default_pe_duration = true),
            (46, 0x10, |b| {
                eht_bss_mut(b).group_addressed_bu_indication_limit = true;
            }),
            (46, 0x04, |b| {
                eht_bss_mut(b).group_addressed_bu_indication_exponent = 1;
            }),
            (46, 0x08, |b| {
                eht_bss_mut(b).group_addressed_bu_indication_exponent = 2;
            }),
        ];
        assert_each_flag_alone(
            &zero,
            || EhtOperations {
                radios: vec![EhtOperationsRadio {
                    ruid: ZERO_MAC,
                    bsses: vec![EhtOperationsBss {
                        bssid: ZERO_MAC,
                        basic_eht_mcs_and_nss_set: [0; 4],
                        ..eht_bss()
                    }],
                }],
            },
            flags,
        );
    }

    // ---- Encoders refuse what the strict decoders reject ----

    #[test]
    fn long_ssids_do_not_encode() {
        let ssid = [b'a'; 256];
        let too_long = EncodeError::CountTooLarge { len: 256, max: 255 };
        let report = report_of(BssConfigurationReportBss {
            ssid: &ssid,
            ..mesh_bss()
        });
        assert_eq!(report.encode_value(), Err(too_long));
        let ap_mlds = AgentApMldConfiguration {
            ap_mlds: vec![ApMld {
                ssid: &ssid,
                ..empty_ap_mld()
            }],
        };
        assert_eq!(ap_mlds.encode_value(), Err(too_long));
        // 255 octets is the longest SSID that fits the length octet.
        let longest = report_of(BssConfigurationReportBss {
            ssid: &ssid[..255],
            ..mesh_bss()
        });
        let bytes = longest.encode_value().unwrap();
        assert_eq!(BssConfigurationReport::decode_value(&bytes), Ok(longest));
    }

    #[test]
    fn nested_counts_are_checked_before_encoding() {
        let mut report = report_golden();
        report.radios[0].bsses = vec![mesh_bss(); 256];
        assert_eq!(report.encode_value(), Err(TOO_MANY));
        assert_eq!(report_too_many_bsses().encode_value(), Err(TOO_MANY));
        let mut ap_mld = ap_mld_golden();
        ap_mld.ap_mlds[0].affiliated_aps = vec![affiliated_ap(true, true, 1); 256];
        assert_eq!(ap_mld.encode_value(), Err(TOO_MANY));
        assert_eq!(eht_too_many_bsses().encode_value(), Err(TOO_MANY));
    }

    #[test]
    fn raw_value_bodies_do_not_encode_an_empty_value() {
        let too_small = EncodeError::CountTooSmall { len: 0, min: 1 };
        assert_eq!(
            BssConfigurationRequest { value: &[] }.encode_value(),
            Err(too_small)
        );
        assert_eq!(
            BssConfigurationResponse { value: &[] }.encode_value(),
            Err(too_small)
        );
    }

    #[test]
    fn two_bit_values_above_three_do_not_encode() {
        let too_large = EncodeError::ValueTooLarge { value: 4, max: 3 };
        let mapping = TidToLinkMapping {
            direction: 4,
            ..zero_mapping()
        };
        assert_eq!(policy_with(mapping).encode_value(), Err(too_large));
        let eht = EhtOperations {
            radios: vec![EhtOperationsRadio {
                ruid: RUID0,
                bsses: vec![EhtOperationsBss {
                    group_addressed_bu_indication_exponent: 4,
                    ..eht_bss()
                }],
            }],
        };
        assert_eq!(eht.encode_value(), Err(too_large));
    }

    #[test]
    fn expected_duration_above_24_bits_does_not_encode() {
        let mapping = TidToLinkMapping {
            expected_duration: 0x0100_0000,
            ..zero_mapping()
        };
        assert_eq!(
            policy_with(mapping).encode_value(),
            Err(EncodeError::ValueTooLarge {
                value: 0x0100_0000,
                max: 0x00FF_FFFF
            })
        );
        let mapping = TidToLinkMapping {
            expected_duration: 0x00FF_FFFF,
            ..zero_mapping()
        };
        let bytes = policy_with(mapping).encode_value().unwrap();
        assert_eq!(bytes[41..44], [0xff, 0xff, 0xff]);
    }

    #[test]
    fn tid_to_link_entries_must_match_the_presence_indicator() {
        let too_few = TidToLinkMapping {
            link_mapping_presence: 0x03,
            tid_to_link: vec![[1, 2]],
            ..zero_mapping()
        };
        assert_eq!(
            policy_with(too_few).encode_value(),
            Err(EncodeError::CountTooSmall { len: 1, min: 2 })
        );
        let too_many = TidToLinkMapping {
            link_mapping_presence: 0x00,
            tid_to_link: vec![[1, 2]],
            ..zero_mapping()
        };
        assert_eq!(
            policy_with(too_many).encode_value(),
            Err(EncodeError::CountTooLarge { len: 1, max: 0 })
        );
    }

    #[test]
    fn bsta_scope_encodes_the_flag_and_a_zero_count() {
        let bytes = bsta_policy(false).encode_value().unwrap();
        assert_eq!(bytes[0], 0x80);
        assert_eq!(bytes[30..32], [0, 0]);
        assert_eq!(bytes.len(), 32);
        let bytes = policy_with(zero_mapping()).encode_value().unwrap();
        assert_eq!(bytes[0], 0x00);
    }

    // ---- Byte order, intentional differences and upstream forms ----

    #[test]
    fn num_mapping_is_little_endian_as_upstream_writes_it() {
        // Audit row 19 (UPSTREAM, note 0008 D6): em_configuration.cpp:1674 copies the host-order
        // count with no htons, so the two count octets are [low, high].
        let value = fixture(("0xe6", "one-mapping-count-byte-order"));
        assert_eq!(value[30..32], [0x01, 0x00]);
        let policy = TidToLinkMappingPolicy::decode_value(&value).unwrap();
        assert_eq!(policy.scope.mappings().len(), 1);
        assert_eq!(policy.encode_value().unwrap(), value);
        // The big-endian reading of the same octets is 256 mappings, which the value cannot hold.
        let mut big_endian = fixture(("0xe6", "one-mapping-count-byte-order"));
        big_endian[30] = 0x00;
        big_endian[31] = 0x01;
        assert_eq!(
            TidToLinkMappingPolicy::decode_value(&big_endian),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
    }

    #[test]
    fn bsta_config_with_mappings_is_value_out_of_range() {
        // intentional-difference: upstream em_configuration.cpp:1674 copies num_mapping from the
        // data model and does not force it to 0 for a bSTA MLD (audit row 22). The spec says to
        // set it to zero, so the strict decoder refuses a nonzero count (note 0009, A8). The
        // count is reported before any mapping is read, so a cut mapping gives the same error.
        let value = fixture(("0xe6", "bsta-config-nonzero-mappings"));
        let out_of_range = Err(DecodeError::ValueOutOfRange { value: 1 });
        assert_eq!(TidToLinkMappingPolicy::decode_value(&value), out_of_range);
        assert_eq!(
            TidToLinkMappingPolicy::decode_value(&value[..32]),
            out_of_range
        );
        // The type cannot hold mappings for a bSTA MLD, so the encoder cannot write this value.
        assert_eq!(
            TidToLinkMappingPolicy::decode_value(&fixture(("0xe6", "bsta-config-zero-mappings"))),
            Ok(bsta_policy(false))
        );
    }

    #[test]
    fn upstream_mapping_record_without_tid_to_link_entries_is_rejected() {
        // intentional-difference: upstream em_configuration.cpp:1687-1690 writes every mapping as
        // a fixed 19-octet record and never writes the TID-to-Link entries (TODO in the sender,
        // audit row 18). With a nonzero Link_Mapping_Presence_Indicator the record is not spec bytes,
        // and there is no lenient form (user decision). One entry is announced; the record has
        // only its 7 reserved octets left, so the entry takes 2 and the reserved block is cut.
        let mut value = fixture(("0xe6", "one-mapping-count-byte-order"));
        assert_eq!(value.len(), 51);
        value[40] = 0x01;
        assert_eq!(
            TidToLinkMappingPolicy::decode_value(&value),
            Err(DecodeError::Truncated {
                needed: 7,
                remaining: 5
            })
        );
    }

    #[test]
    fn all_four_directions_are_carried() {
        // Note 0009, D3 (audit row 23): the codec does not decide policy.
        let rows = [
            ("direction-0", 0),
            ("direction-1", 1),
            ("direction-2", 2),
            ("direction-3", 3),
        ];
        for (case, direction) in rows {
            let value = fixture(("0xe6", case));
            let policy = TidToLinkMappingPolicy::decode_value(&value).unwrap();
            assert_eq!(policy.scope.mappings()[0].direction, direction);
        }
    }

    #[test]
    fn expected_duration_is_three_octets_big_endian_and_always_present() {
        let value = fixture(("0xe6", "expected-duration-bit-clear-duration-set"));
        let policy = TidToLinkMappingPolicy::decode_value(&value).unwrap();
        let mapping = &policy.scope.mappings()[0];
        assert!(!mapping.expected_duration_present);
        assert_ne!(mapping.expected_duration, 0);
        assert_eq!(value[41..44], mapping.expected_duration.to_be_bytes()[1..]);
        let golden = TidToLinkMappingPolicy::decode_value(&fixture(("0xe6", "golden"))).unwrap();
        assert_eq!(golden.scope.mappings()[0].expected_duration, 0x01_2345);
    }

    #[test]
    fn tid_to_link_entries_follow_the_wire_order_of_the_presence_indicator() {
        // The entry count is the number of set bits; the entries stay in wire order and the
        // codec does not map bits to entries.
        let value = fixture(("0xe6", "presence-all"));
        let policy = TidToLinkMappingPolicy::decode_value(&value).unwrap();
        let mapping = &policy.scope.mappings()[0];
        assert_eq!(mapping.link_mapping_presence, 0xff);
        assert_eq!(mapping.tid_to_link.len(), 8);
        assert_eq!(mapping.tid_to_link[0], [0x10, 0xe0]);
        assert_eq!(mapping.tid_to_link[7], [0x17, 0xe7]);
    }

    #[test]
    fn eht_operations_accepts_more_radios_and_bsses_than_upstream_stores() {
        // intentional-difference: upstream em.cpp:2163-2166 and 2183-2186 reject the whole TLV with
        // more than EM_MAX_RADIO_PER_AGENT (4) radios or more than EM_MAX_BSS_PER_RADIO (16) BSSs in a
        // radio. That is a storage limit of its data model, not a wire rule.
        let radios = EhtOperations::decode_value(&fixture(("0xe7", "five-radios"))).unwrap();
        assert_eq!(radios.radios.len(), 5);
        let bsses = EhtOperations::decode_value(&fixture(("0xe7", "seventeen-bss"))).unwrap();
        assert_eq!(bsses.radios[0].bsses.len(), 17);
    }

    #[test]
    fn eht_valid_flags_do_not_change_the_layout() {
        let flags_clear = fixture(("0xe7", "valid-clear-fields-set"));
        let body = EhtOperations::decode_value(&flags_clear).unwrap();
        let bss = body.radios[0].bsses[0];
        assert!(!bss.operation_information_valid && !bss.disabled_subchannel_valid);
        assert_ne!(bss.control, 0);
        assert_eq!(body.encode_value(), Ok(flags_clear));
    }

    #[test]
    fn backhaul_sta_mld_empty_value_is_absent_only_leniently() {
        // intentional-difference: upstream em_configuration.cpp:1439-1442 and 3771-3773 send the
        // TLV with an empty value when no bSTA MLD is configured (audit row 287). The strict
        // decoder refuses it; the lenient one reports the TLV as absent.
        assert_eq!(
            BackhaulStaMldConfiguration::decode_value(&[]),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
        assert_eq!(
            BackhaulStaMldConfiguration::decode_value_lenient(&[]),
            Ok(None)
        );
    }

    #[test]
    fn report_keeps_a_trailing_nul_of_upstreams_ssid_length() {
        // intentional-difference: upstream em_configuration.cpp:1149-1155 sends strlen + 1 as the
        // SSID length, so the trailing NUL is the last SSID octet (audit row 229, note 0009 D4).
        // Those bytes are a spec SSID that ends in a NUL; this body keeps them raw.
        let value = fixture(("0xb7", "ssid-33-trailing-nul"));
        let report = BssConfigurationReport::decode_value(&value).unwrap();
        let ssid = report.radios[0].bsses[0].ssid;
        assert_eq!(ssid.len(), 33);
        assert_eq!(ssid.last(), Some(&0));
        assert_eq!(report.encode_value(), Ok(value));
    }

    #[test]
    fn ap_mld_keeps_upstreams_nul_padded_ssid_and_steps_by_the_wire_length() {
        // intentional-difference: upstream em_configuration.cpp:1393-1394 always sends SSID
        // Length 32 and a NUL-padded SSID (audit row 279), and its receiver steps over each entry
        // by a fixed size (audit row 278, em_configuration.cpp:2740-2858). Here the padded SSID
        // is 32 raw octets, and AP MLDs with SSIDs of different lengths decode one after the
        // other.
        let padded = fixture(("0xe0", "ssid-32-nul-padded-upstream"));
        let body = AgentApMldConfiguration::decode_value(&padded).unwrap();
        let ssid = body.ap_mlds[0].ssid;
        assert_eq!(ssid.len(), 32);
        assert_eq!(&ssid[..8], b"5G-home\0");
        assert!(ssid[7..].iter().all(|&octet| octet == 0));
        let golden_value = fixture(("0xe0", "golden"));
        let golden = AgentApMldConfiguration::decode_value(&golden_value).unwrap();
        assert_eq!(golden.ap_mlds[0].ssid, b"MeshHome-7G");
        assert_eq!(golden.ap_mlds[1].ssid, b"Guest");
    }

    #[test]
    fn ap_mld_valid_flags_never_remove_a_field() {
        let value = fixture(("0xe0", "aff-both-clear-fields-nonzero"));
        let body = AgentApMldConfiguration::decode_value(&value).unwrap();
        let affiliated = body.ap_mlds[0].affiliated_aps[0];
        assert!(!affiliated.mac_valid && !affiliated.link_id_valid);
        assert_ne!(affiliated.mac, ZERO_MAC);
        assert_ne!(affiliated.link_id, 0);
        assert_eq!(body.encode_value(), Ok(value));
        // Link IDs 16 to 255 are reserved values, kept raw (note 0009, D2).
        let reserved = fixture(("0xe0", "linkid-16"));
        let body = AgentApMldConfiguration::decode_value(&reserved).unwrap();
        assert_eq!(body.ap_mlds[0].affiliated_aps[0].link_id, 16);
    }

    #[test]
    fn associated_sta_mld_count_past_the_value_is_truncated() {
        // intentional-difference: upstream em_configuration.cpp:2637-2644 logs "Malformed" for a
        // count that does not fit and continues with the count clamped to the entries that fit
        // (audit row 291). A count the value cannot hold is an error here.
        let value = fixture(("0xe2", "affil-overstated"));
        assert!(matches!(
            AssociatedStaMldConfigurationReport::decode_value(&value),
            Err(DecodeError::Truncated { .. })
        ));
    }

    #[test]
    fn configuration_values_are_raw_bytes_in_either_dpp_form() {
        // Audit rows 252-256, note 0009 D5: upstream's request carries three DPP attributes and
        // its response carries bare JSON with no attribute header (em_configuration.cpp:3913-3964,
        // 4012-4067). The value is raw, so each is the same kind of body.
        let attributes = fixture(("0xbb", "three-attributes-upstream"));
        let request = BssConfigurationRequest::decode_value(&attributes).unwrap();
        assert_eq!(request.value, attributes);
        let bare_json = fixture(("0xbd", "bare-json-upstream"));
        let response = BssConfigurationResponse::decode_value(&bare_json).unwrap();
        assert_eq!(response.value, bare_json);
        assert_eq!(response.value[0], b'{');
        assert_eq!(request.encode_value(), Ok(attributes));
    }

    #[test]
    fn metrics_expansion_octets_are_ignored_and_not_written() {
        for key in [
            "expansion-empty",
            "expansion-one-octet",
            "expansion-seven-octets",
        ] {
            let body = AffiliatedStaMetrics::decode_value(&fixture(("0xe4", key))).unwrap();
            assert_eq!(body, sta_metrics_golden());
            assert_eq!(body.encode_value(), Ok(fixture(("0xe4", "golden"))));
        }
        for key in [
            "expansion-empty",
            "expansion-one-octet",
            "expansion-seven-octets",
        ] {
            let body = AffiliatedApMetrics::decode_value(&fixture(("0xe5", key))).unwrap();
            assert_eq!(body, ap_metrics_golden());
            assert_eq!(body.encode_value(), Ok(fixture(("0xe5", "golden"))));
        }
    }

    #[test]
    fn counters_are_big_endian() {
        // §3.1.2 and audit row 294 (upstream writes htonl).
        let value = fixture(("0xe4", "counters-low-byte-only"));
        let body = AffiliatedStaMetrics::decode_value(&value).unwrap();
        assert_eq!((body.bytes_sent, body.packets_sent_errors), (1, 5));
        let value = fixture(("0xe5", "counters-low-byte-only"));
        let body = AffiliatedApMetrics::decode_value(&value).unwrap();
        assert_eq!((body.packets_sent, body.broadcast_bytes_received), (1, 9));
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<MldModes>() <= 4);
        assert!(size_of::<BssConfigurationReportBss<'_>>() <= 2 * word + 16);
        assert!(size_of::<BssConfigurationReportRadio<'_>>() <= 3 * word + 8);
        assert!(size_of::<BssConfigurationReport<'_>>() <= 3 * word);
        assert!(size_of::<BssConfigurationRequest<'_>>() <= 2 * word);
        assert!(size_of::<BssConfigurationResponse<'_>>() <= 2 * word);
        assert!(size_of::<AffiliatedAp>() <= 16 + 8);
        assert!(size_of::<ApMld<'_>>() <= 5 * word + 16);
        assert!(size_of::<AgentApMldConfiguration<'_>>() <= 3 * word);
        assert!(size_of::<AffiliatedBsta>() <= 16);
        assert!(size_of::<BackhaulStaMldConfiguration>() <= 3 * word + 24);
        assert!(size_of::<AffiliatedSta>() <= 12);
        assert!(size_of::<AssociatedStaMldConfigurationReport>() <= 3 * word + 20);
        assert!(size_of::<MldStructure>() <= 3 * word + 8);
        assert!(size_of::<AffiliatedStaMetrics>() <= 28);
        assert!(size_of::<AffiliatedApMetrics>() <= 44);
        assert!(size_of::<TidToLinkMapping>() <= 3 * word + 24);
        assert!(size_of::<TidToLinkScope>() <= 3 * word);
        assert!(size_of::<TidToLinkMappingPolicy>() <= 3 * word + 16);
        assert!(size_of::<EhtOperationsBss>() <= 24);
        assert!(size_of::<EhtOperationsRadio>() <= 3 * word + 8);
        assert!(size_of::<EhtOperations>() <= 3 * word);
    }

    #[test]
    fn bss_configuration_request_fills_a_tlv_through_the_builder() {
        let bytes = vec![0xa5; 65_535];
        let body = BssConfigurationRequest { value: &bytes };
        let message = crate::body_testkit::build_message(&body).unwrap();
        let tlv = crate::body_testkit::single_tlv(&message).unwrap();
        let decoded = tlv.decode::<BssConfigurationRequest<'_>>().unwrap();
        assert_eq!(decoded.value, bytes);
        assert_eq!(decoded.value.as_ptr(), tlv.value.as_ptr());
    }
}
