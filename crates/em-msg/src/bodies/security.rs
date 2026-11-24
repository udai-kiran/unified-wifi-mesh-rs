// SPDX-License-Identifier: Apache-2.0

//! DPP, security and encapsulation bodies (note 0009, B11): the Source Info, Tunneled Message
//! Type and Tunneled bodies, the 1905 Layer Security Capability, MIC and Encrypted Payload
//! bodies, the AKM Suite Capabilities body, and the 1905 Encap DPP, 1905 Encap EAPOL, DPP
//! Bootstrapping URI Notification, DPP CCE Indication, DPP Chirp Value and DPP Message bodies.
//!
//! All multi-octet integers are big-endian (§3.1.2). Carried frames, URIs, EAPOL, encrypted
//! payloads, MIC values and tunneled frames are raw bytes borrowed from the value (note 0009,
//! D5): em-msg decodes only the fields the `EasyMesh` table defines. An empty payload is
//! spec-valid wherever the table has no "one or more" rule. Reserved values are kept raw, and
//! reserved bits are ignored on decode and written as zero (note 0009, D2).

use em_types::{MacAddr, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// Largest value of a 2-bit field.
const MAX_TWO_BITS: u8 = 3;

/// `Enrollee MAC Address Present` bit of the 1905 Encap DPP and DPP Chirp Value flags octet.
const MAC_PRESENT: u8 = 0x80;
/// `DPP Frame Indicator` bit of the 1905 Encap DPP flags octet: set for a GAS frame.
const GAS_FRAME: u8 = 0x20;
/// `Hash Validity` bit of the DPP Chirp Value flags octet: set to establish, clear to purge.
const HASH_VALID: u8 = 0x40;

/// A 6-octet transmission counter: the Integrity Transmission Counter of the MIC body (§17.2.68
/// Table 90) and the Encryption Transmission Counter of the Encrypted Payload body (§17.2.69
/// Table 91). It is big-endian on the wire (§3.1.2), so a value above 2^48 - 1 cannot exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Counter48(u64);

impl Counter48 {
    /// Largest value of a 6-octet counter.
    pub const MAX: u64 = (1 << 48) - 1;

    /// Returns the counter, or `None` if `value` is above [`Counter48::MAX`].
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value > Self::MAX {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Returns the counter value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Reads a 6-octet big-endian transmission counter.
fn read_counter(reader: &mut Reader<'_>) -> Result<Counter48, DecodeError> {
    let counter_octets: [u8; 6] = reader.array()?;
    let mut padded = [0; 8];
    padded[2..].copy_from_slice(&counter_octets);
    Ok(Counter48(u64::from_be_bytes(padded)))
}

/// Writes a 6-octet big-endian transmission counter.
fn write_counter(writer: &mut Writer, counter: Counter48) {
    writer.bytes(&counter.get().to_be_bytes()[2..]);
}

fn checked_two_bits(value: u8) -> Result<u8, EncodeError> {
    if value > MAX_TWO_BITS {
        Err(EncodeError::ValueTooLarge {
            value: u32::from(value),
            max: u32::from(MAX_TWO_BITS),
        })
    } else {
        Ok(value)
    }
}

/// Source Info, `EasyMesh` §17.2.54 Table 76; audit row 188.
/// Upstream `em_source_info_t` (`em_base.h:1786-1788`) has no sender or receiver at the pin;
/// `em_msg.cpp:1131` only lists it as mandatory in the Tunneled CMDU. The value is exactly 6
/// octets: the MAC of the device that generated the message in the Tunneled TLV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceInfo {
    /// The MAC address of the device that generated the tunneled message.
    pub sta_mac: MacAddr,
}

impl TlvBody<'_> for SourceInfo {
    const TLV_TYPE: TlvType = TlvType::SOURCE_INFO;

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

/// Tunneled Message Type, `EasyMesh` §17.2.55 Table 77; audit row 189.
/// Upstream `em_tunneled_msg_type_t` (`em_base.h:1790-1792`) has no sender or receiver at the
/// pin (`em_msg.cpp:1132` lists it as mandatory). The value is exactly 1 octet and is kept raw:
/// 0 Association Request, 1 Re-Association Request, 2 BTM Query, 3 WNM Request, 4 ANQP request
/// for Neighbor Report, 5 DSCP Policy Query, 6 DSCP Policy Response, 7-255 reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TunneledMessageType {
    /// The tunneled protocol type, raw.
    pub message_type: u8,
}

impl TlvBody<'_> for TunneledMessageType {
    const TLV_TYPE: TlvType = TlvType::TUNNELED_MESSAGE_TYPE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let message_type = reader.u8()?;
        reader.finish()?;
        Ok(Self { message_type })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.message_type);
        Ok(writer.into_bytes())
    }
}

/// Tunneled, `EasyMesh` §17.2.56 Table 78; audit row 190.
/// Upstream has no sender or receiver at the pin; its `em_tunneled_t` (`em_base.h:1794-1797`,
/// a length and a pointer) cannot describe the wire form. The value is the 802.11 request frame
/// body and nothing else: it is the whole value, and may be empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tunneled<'a> {
    /// The 802.11 request frame body, raw.
    pub body: &'a [u8],
}

impl<'a> TlvBody<'a> for Tunneled<'a> {
    const TLV_TYPE: TlvType = TlvType::TUNNELED;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let body = reader.rest();
        reader.finish()?;
        Ok(Self { body })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.bytes(self.body);
        Ok(writer.into_bytes())
    }
}

/// 1905 Layer Security Capability, `EasyMesh` §17.2.67 Table 89; audit row 213.
/// Upstream `em_ieee_1905_security_cap_t` (`em_base.h:2629-2633`), sender
/// `em_configuration.cpp:4436-4438`, `em_capability.cpp:206-208,402-404`; the receivers do not
/// read the value. The value is exactly 3 octets, each kept raw: 0 is the only defined value of
/// each (1905 Device Provisioning Protocol, HMAC-SHA256, AES-SIV); 1-255 are reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ieee1905LayerSecurityCapability {
    /// Onboarding protocol, raw.
    pub onboarding_protocol: u8,
    /// Message integrity (MIC) algorithm, raw.
    pub mic_algorithm: u8,
    /// Message encryption algorithm, raw.
    pub encryption_algorithm: u8,
}

impl TlvBody<'_> for Ieee1905LayerSecurityCapability {
    const TLV_TYPE: TlvType = TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let onboarding_protocol = reader.u8()?;
        let mic_algorithm = reader.u8()?;
        let encryption_algorithm = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            onboarding_protocol,
            mic_algorithm,
            encryption_algorithm,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.onboarding_protocol);
        writer.u8(self.mic_algorithm);
        writer.u8(self.encryption_algorithm);
        Ok(writer.into_bytes())
    }
}

/// MIC, `EasyMesh` §17.2.68 Table 90; audit row 214.
/// Written from the spec alone: upstream has no sender or receiver at the pin (`em_base.h:872`
/// only declares the type code). The value is a flags octet (bits 7-6 the 1905 GTK Key Id,
/// bits 5-4 the MIC Version, bits 3-0 reserved), the 6-octet Integrity Transmission Counter,
/// the Source 1905 AL MAC Address, the 2-octet MIC Length and the MIC. Both counter and length
/// are big-endian (§3.1.2). The MIC Length is derived from `mic` on encode. A MIC Version above 0
/// is reserved and kept raw. The MIC is not computed or checked here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mic<'a> {
    /// 1905 GTK Key Id (2 bits, 0-3).
    pub gtk_key_id: u8,
    /// MIC Version (2 bits, 0-3): 0 is version 1, 1-3 are reserved.
    pub mic_version: u8,
    /// Integrity Transmission Counter (48 bits).
    pub counter: Counter48,
    /// Source 1905 AL MAC Address.
    pub source_al_mac: MacAddr,
    /// The Message Integrity Code, raw.
    pub mic: &'a [u8],
}

impl<'a> TlvBody<'a> for Mic<'a> {
    const TLV_TYPE: TlvType = TlvType::MIC;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let flags = reader.u8()?;
        let counter = read_counter(&mut reader)?;
        let source_al_mac = reader.mac()?;
        let mic_len = usize::from(reader.be_u16()?);
        let mic = reader.bytes(mic_len)?;
        reader.finish()?;
        Ok(Self {
            gtk_key_id: flags >> 6,
            mic_version: (flags >> 4) & MAX_TWO_BITS,
            counter,
            source_al_mac,
            mic,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let gtk_key_id = checked_two_bits(self.gtk_key_id)?;
        let mic_version = checked_two_bits(self.mic_version)?;
        let mut writer = Writer::new();
        writer.u8((gtk_key_id << 6) | (mic_version << 4));
        write_counter(&mut writer, self.counter);
        writer.mac(self.source_al_mac);
        writer.count_u16(self.mic.len())?;
        writer.bytes(self.mic);
        Ok(writer.into_bytes())
    }
}

/// Encrypted Payload, `EasyMesh` §17.2.69 Table 91; audit row 215.
/// Written from the spec alone: upstream has no sender or receiver at the pin (`em_base.h:873`
/// only declares the type code). The value is the 6-octet Encryption Transmission Counter, the
/// source and destination 1905 AL MAC addresses, the 2-octet AES-SIV length and the AES-SIV
/// output (the SIV followed by the encrypted TLVs). Counter and length are big-endian
/// (§3.1.2); the length is derived from `aes_siv` on encode. Nothing is decrypted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncryptedPayload<'a> {
    /// Encryption Transmission Counter (48 bits).
    pub counter: Counter48,
    /// Source 1905 AL MAC Address.
    pub source_al_mac: MacAddr,
    /// Destination 1905 AL MAC Address.
    pub destination_al_mac: MacAddr,
    /// The AES-SIV encryption output, raw.
    pub aes_siv: &'a [u8],
}

impl<'a> TlvBody<'a> for EncryptedPayload<'a> {
    const TLV_TYPE: TlvType = TlvType::ENCRYPTED_PAYLOAD;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let counter = read_counter(&mut reader)?;
        let source_al_mac = reader.mac()?;
        let destination_al_mac = reader.mac()?;
        let siv_len = usize::from(reader.be_u16()?);
        let aes_siv = reader.bytes(siv_len)?;
        reader.finish()?;
        Ok(Self {
            counter,
            source_al_mac,
            destination_al_mac,
            aes_siv,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        write_counter(&mut writer, self.counter);
        writer.mac(self.source_al_mac);
        writer.mac(self.destination_al_mac);
        writer.count_u16(self.aes_siv.len())?;
        writer.bytes(self.aes_siv);
        Ok(writer.into_bytes())
    }
}

/// One AKM suite selector of the AKM Suite Capabilities TLV: a 3-octet OUI and a suite type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AkmSuite {
    /// The OUI, as written on the wire.
    pub oui: [u8; 3],
    /// The AKM suite type.
    pub suite_type: u8,
}

impl AkmSuite {
    fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let oui = reader.array()?;
        let suite_type = reader.u8()?;
        Ok(Self { oui, suite_type })
    }

    fn write(self, writer: &mut Writer) {
        writer.bytes(&self.oui);
        writer.u8(self.suite_type);
    }
}

/// Reads a count and that many AKM suites. A count that is larger than the data is `Truncated`.
fn read_akm_suites(reader: &mut Reader<'_>) -> Result<Vec<AkmSuite>, DecodeError> {
    let count = usize::from(reader.u8()?);
    let mut suites = bounded_vec(count, reader.remaining(), 4);
    for _ in 0..count {
        suites.push(AkmSuite::read(reader)?);
    }
    Ok(suites)
}

fn write_akm_suites(writer: &mut Writer, suites: &[AkmSuite]) -> Result<(), EncodeError> {
    writer.count_u8(suites.len())?;
    for suite in suites {
        suite.write(writer);
    }
    Ok(())
}

/// AKM Suite Capabilities, `EasyMesh` §17.2.78 Table 100; audit rows 239–240.
/// Upstream `em_bh_akm_suite_t` and `em_fh_akm_suite_t` (`em_base.h:1712-1737`), sender
/// `em.cpp:1157-1254` (`create_akm_suite_cap_tlv`), receiver `em_configuration.cpp:4985-5121`
/// (`store_akm_suite_cap`). The value is `Num_BackAKM`, that many 4-octet suites, `Num_FrontAKM`
/// and that many suites. Both counts may be 0 and may reach 255; the counts are derived from the
/// lists on encode.
///
/// intentional-difference: the upstream receiver keeps at most `EM_MAX_AKMS` (10) suites per
/// BSS (`em_configuration.cpp:5095,5105`, `em_base.h:230`; audit row 240). That is storage
/// policy, not wire format, so this body keeps every suite listed. The same receiver also drops
/// a suite whose selector it does not map (`em_configuration.cpp:5007-5012`) and, for a BSS
/// with a network profile, takes that BSS's AKMs from the profile instead (`:5043-5090`). This
/// body does neither and does not map selectors to names.
#[derive(Debug, PartialEq, Eq)]
pub struct AkmSuiteCapabilities {
    /// Backhaul BSS suites (`Num_BackAKM` entries).
    pub backhaul: Vec<AkmSuite>,
    /// Fronthaul BSS suites (`Num_FrontAKM` entries).
    pub fronthaul: Vec<AkmSuite>,
}

impl TlvBody<'_> for AkmSuiteCapabilities {
    const TLV_TYPE: TlvType = TlvType::AKM_SUITE_CAPABILITIES;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let backhaul = read_akm_suites(&mut reader)?;
        let fronthaul = read_akm_suites(&mut reader)?;
        reader.finish()?;
        Ok(Self {
            backhaul,
            fronthaul,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        write_akm_suites(&mut writer, &self.backhaul)?;
        write_akm_suites(&mut writer, &self.fronthaul)?;
        Ok(writer.into_bytes())
    }
}

/// 1905 Encap DPP, `EasyMesh` §17.2.79 Table 101; audit rows 241–243.
/// Upstream `em_encap_dpp_t` (`em_base.h:1006-1020`), sender `ec_util.cpp:415-449`
/// (`create_encap_dpp_tlv`, length written in network order at `:443`), receiver
/// `ec_util.cpp:368-413` (`parse_encap_dpp_tlv`). The value is a flags octet (bit 7 Enrollee MAC
/// Address Present, bit 5 DPP Frame Indicator, bits 6 and 4-0 reserved), the Destination STA MAC
/// Address only if bit 7 is set, the Frame Type, the 2-octet frame length and the frame. The
/// flag follows `destination_sta_mac` and the length follows `frame` on encode. The frame type is
/// kept raw: the spec sets it to the DPP Public Action frame type, or to 255 for a GAS frame, but
/// that is a sender rule this body does not enforce.
///
/// intentional-difference: the upstream receiver compares the frame length with the octets left
/// before it takes off the 3 octets of frame type and length, so it accepts a frame length up to
/// 3 above the data (`ec_util.cpp:402`, audit row 242); and when bit 7 is set with fewer than 6
/// octets left it zeroes the MAC and reads on (`ec_util.cpp:381-388`, audit row 243). This body
/// reports `Truncated` in both cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ieee1905EncapDpp<'a> {
    /// The Destination STA MAC Address, present when the Enrollee MAC Address Present bit is set.
    pub destination_sta_mac: Option<MacAddr>,
    /// `true` for a GAS frame (DPP Frame Indicator 1), `false` for a DPP Public Action frame.
    pub gas_frame: bool,
    /// The frame type octet, raw.
    pub frame_type: u8,
    /// The encapsulated frame, raw.
    pub frame: &'a [u8],
}

impl<'a> TlvBody<'a> for Ieee1905EncapDpp<'a> {
    const TLV_TYPE: TlvType = TlvType::IEEE1905_ENCAP_DPP;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let flags = reader.u8()?;
        let destination_sta_mac = if flags & MAC_PRESENT != 0 {
            Some(reader.mac()?)
        } else {
            None
        };
        let frame_type = reader.u8()?;
        let frame_len = usize::from(reader.be_u16()?);
        let frame = reader.bytes(frame_len)?;
        reader.finish()?;
        Ok(Self {
            destination_sta_mac,
            gas_frame: flags & GAS_FRAME != 0,
            frame_type,
            frame,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut flags = 0;
        if self.destination_sta_mac.is_some() {
            flags |= MAC_PRESENT;
        }
        if self.gas_frame {
            flags |= GAS_FRAME;
        }
        let mut writer = Writer::new();
        writer.u8(flags);
        if let Some(mac) = self.destination_sta_mac {
            writer.mac(mac);
        }
        writer.u8(self.frame_type);
        writer.count_u16(self.frame.len())?;
        writer.bytes(self.frame);
        Ok(writer.into_bytes())
    }
}

/// 1905 Encap EAPOL, `EasyMesh` §17.2.80 Table 102; audit row 244.
/// Sender `em_provisioning.cpp:180-215` (`send_1905_eapol_encap_msg`), receiver
/// `em_provisioning.cpp:380-402` (`handle_1905_encap_eapol_msg`). The value is the EAPOL frame
/// and nothing else: it is the whole value, and may be empty. The frame is not parsed here.
///
/// intentional-difference: the upstream receiver rejects a zero-length EAPOL frame
/// (`em_provisioning.cpp:392-395`, audit row 244), and the upstream sender refuses one too
/// (`em_provisioning.cpp:183`). The spec has no minimum, so this body accepts
/// an empty value and leaves that policy to the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ieee1905EncapEapol<'a> {
    /// The EAPOL frame, raw.
    pub eapol: &'a [u8],
}

impl<'a> TlvBody<'a> for Ieee1905EncapEapol<'a> {
    const TLV_TYPE: TlvType = TlvType::IEEE1905_ENCAP_EAPOL;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let eapol = reader.rest();
        reader.finish()?;
        Ok(Self { eapol })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.bytes(self.eapol);
        Ok(writer.into_bytes())
    }
}

/// DPP Bootstrapping URI Notification, `EasyMesh` §17.2.81 Table 103; audit row 245.
/// Upstream `em_dpp_bootstrap_uri_t` (`em_base.h:995-1000`) has no production sender, and the
/// receiver case in `em_provisioning.cpp:459-460` is empty. The value is the RUID, the BSSID,
/// the bSTA address and the DPP URI to the end of the value; the URI is raw (no terminator is
/// stated) and may be empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DppBootstrappingUriNotification<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// The BSSID (or the affiliated AP MAC address of an MLD) the URI was received on.
    pub bssid: MacAddr,
    /// The backhaul STA address (or the affiliated bSTA address of an MLD) that sent the URI.
    pub bsta_mac: MacAddr,
    /// The DPP URI, raw.
    pub uri: &'a [u8],
}

impl<'a> TlvBody<'a> for DppBootstrappingUriNotification<'a> {
    const TLV_TYPE: TlvType = TlvType::DPP_BOOTSTRAPPING_URI_NOTIFICATION;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let ruid = reader.mac()?;
        let bssid = reader.mac()?;
        let bsta_mac = reader.mac()?;
        let uri = reader.rest();
        reader.finish()?;
        Ok(Self {
            ruid,
            bssid,
            bsta_mac,
            uri,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.ruid);
        writer.mac(self.bssid);
        writer.mac(self.bsta_mac);
        writer.bytes(self.uri);
        Ok(writer.into_bytes())
    }
}

/// DPP CCE Indication, `EasyMesh` §17.2.82 Table 104; audit row 246.
/// Upstream `em_cce_indication_t` (`em_base.h:1002-1004`), sender `em_provisioning.cpp:51-73`
/// (`create_cce_ind_msg`), receiver `em_provisioning.cpp:405-431`. The value is exactly 1 octet:
/// 1 enable, 0 disable, 2-255 reserved; it is kept raw.
///
/// intentional-difference: the upstream receiver treats every value but 1 as an error, so a
/// Disable is never acted on (`em_provisioning.cpp:416-421`, audit row 246). This body keeps the
/// octet raw and leaves that policy to the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DppCceIndication {
    /// Advertise CCE, raw.
    pub advertise_cce: u8,
}

impl TlvBody<'_> for DppCceIndication {
    const TLV_TYPE: TlvType = TlvType::DPP_CCE_INDICATION;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let advertise_cce = reader.u8()?;
        reader.finish()?;
        Ok(Self { advertise_cce })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.advertise_cce);
        Ok(writer.into_bytes())
    }
}

/// DPP Chirp Value, `EasyMesh` §17.2.83 Table 105; audit rows 247–251.
/// Upstream `em_dpp_chirp_value_t` (`em_base.h:2243-2248`), sender `ec_util.cpp:328-366`
/// (`create_dpp_chirp_tlv`), receiver `ec_util.cpp:276-326` (`parse_dpp_chirp_tlv`). The value is
/// a flags octet (bit 7 Enrollee MAC Address Present, bit 6 Hash Validity, bits 5-0 reserved),
/// the Destination STA MAC Address only if bit 7 is set, the 1-octet Hash Length and the hash.
/// The flag follows `destination_sta_mac` and the length follows `hash` on encode. The Hash
/// Length and Hash Value are on the wire whatever the Hash Validity says (audit row 248), and
/// the hash may be empty. The hash is not interpreted.
///
/// intentional-difference: upstream writes and reads the Hash Length as 2 octets, big-endian
/// (`ec_util.cpp:337,356-358`, receiver `:300-302`; audit row 247), where the spec has 1 octet
/// (note 0008, D8). The strict decoder follows the spec and rejects upstream's form with
/// `TrailingBytes`. [`TlvBodyLenient::decode_value_lenient`] also accepts upstream's form and
/// gives the same body. It reads all 16 bits of upstream's length, so a hash of 256 or more
/// octets decodes too, and that body cannot be encoded again. The encoder always writes the
/// spec form.
///
/// intentional-difference: the upstream receiver ignores the hash when Hash Validity is 0
/// (`ec_util.cpp:295-298`, audit row 248) and rejects a Hash Length of 0 when it is 1
/// (`ec_util.cpp:303-307`). This body keeps the hash for both validities and accepts a length
/// of 0; those are consumer policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DppChirpValue<'a> {
    /// The Destination STA MAC Address, present when the Enrollee MAC Address Present bit is set.
    pub destination_sta_mac: Option<MacAddr>,
    /// Hash Validity: `true` to establish, `false` to purge the DPP authentication state.
    pub hash_valid: bool,
    /// The hash value, raw. The spec form holds at most 255 octets; a longer hash can only come
    /// from the lenient decoder, and encoding it returns `CountTooLarge`.
    pub hash: &'a [u8],
}

impl<'a> DppChirpValue<'a> {
    /// Reads the flags octet and the optional MAC, which both forms share.
    fn read_head(reader: &mut Reader<'a>) -> Result<(bool, Option<MacAddr>), DecodeError> {
        let flags = reader.u8()?;
        let destination_sta_mac = if flags & MAC_PRESENT != 0 {
            Some(reader.mac()?)
        } else {
            None
        };
        Ok((flags & HASH_VALID != 0, destination_sta_mac))
    }

    /// Reads upstream's form: a 2-octet big-endian Hash Length, which must be non-zero and equal
    /// to the octets left. Upstream forwards the length of a received hash attribute
    /// (`ec_pa_configurator.cpp:54`), so it can be above 255. Returns `None` for any other value.
    fn read_upstream_form(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        let mut reader = Reader::new(value);
        let (hash_valid, destination_sta_mac) = Self::read_head(&mut reader)?;
        let length_high = reader.u8()?;
        let after_high = reader.rest();
        let Some((&length_low, hash)) = after_high.split_first() else {
            return Ok(None);
        };
        let hash_len = usize::from(u16::from_be_bytes([length_high, length_low]));
        if hash_len == 0 || hash_len != hash.len() {
            return Ok(None);
        }
        Ok(Some(Self {
            destination_sta_mac,
            hash_valid,
            hash,
        }))
    }
}

impl<'a> TlvBody<'a> for DppChirpValue<'a> {
    const TLV_TYPE: TlvType = TlvType::DPP_CHIRP_VALUE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let (hash_valid, destination_sta_mac) = Self::read_head(&mut reader)?;
        let hash_len = usize::from(reader.u8()?);
        let hash = reader.bytes(hash_len)?;
        reader.finish()?;
        Ok(Self {
            destination_sta_mac,
            hash_valid,
            hash,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut flags = 0;
        if self.destination_sta_mac.is_some() {
            flags |= MAC_PRESENT;
        }
        if self.hash_valid {
            flags |= HASH_VALID;
        }
        let mut writer = Writer::new();
        writer.u8(flags);
        if let Some(mac) = self.destination_sta_mac {
            writer.mac(mac);
        }
        writer.count_u8(self.hash.len())?;
        writer.bytes(self.hash);
        Ok(writer.into_bytes())
    }
}

impl<'a> TlvBodyLenient<'a> for DppChirpValue<'a> {
    /// Accepts upstream's 2-octet Hash Length as well as the spec form. There is no absent form,
    /// so the result is never `Ok(None)`.
    fn decode_value_lenient(value: &'a [u8]) -> Result<Option<Self>, DecodeError> {
        match Self::decode_value(value) {
            Err(strict_error @ DecodeError::TrailingBytes { .. }) => {
                // Upstream's high length octet reads as the spec's Hash Length, and the rest of
                // its length and hash are left over.
                match Self::read_upstream_form(value)? {
                    Some(body) => Ok(Some(body)),
                    None => Err(strict_error),
                }
            }
            strict => strict.map(Some),
        }
    }
}

/// DPP Message, `EasyMesh` §17.2.86 Table 108; audit rows 257–258.
/// Sender `em_provisioning.cpp:318-351` (`create_dpp_direct_encap_msg`), receiver
/// `em_provisioning.cpp:536-545` (`handle_direct_encap_dpp`). The value is the DPP frame and
/// nothing else: it is the whole value, and may be empty. The frame is not parsed here.
///
/// intentional-difference: the upstream receiver requires a non-zero frame length
/// (`em_provisioning.cpp:540`, audit row 257). The spec has no minimum, so this body accepts an
/// empty value and leaves that policy to the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DppMessage<'a> {
    /// The DPP frame, raw.
    pub frame: &'a [u8],
}

impl<'a> TlvBody<'a> for DppMessage<'a> {
    const TLV_TYPE: TlvType = TlvType::DPP_MESSAGE;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let frame = reader.rest();
        reader.finish()?;
        Ok(Self { frame })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.bytes(self.frame);
        Ok(writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, sweep_row, sweep_row_lenient, truncated_in};
    use core::mem::size_of;

    const GROUP: &str = "security";
    const STA: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4]);
    const OTHER: MacAddr = MacAddr::new([0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f]);
    const THIRD: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]);
    const AL_MAC: MacAddr = MacAddr::new([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    const ZERO_MAC: MacAddr = MacAddr::new([0; 6]);
    const TOO_MANY_BYTE_COUNT: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    const TOO_LONG_FOR_LENGTH: EncodeError = EncodeError::CountTooLarge {
        len: 65_536,
        max: 65_535,
    };
    /// Invalid rows that a lenient decoder accepts; the only lenient form is a kind=lenient row.
    const LENIENT_INPUTS: &[Key] = &[];
    const CANONICAL: &[(Key, Key)] = &[
        (
            ("0xab", "reserved-bits-set"),
            ("0xab", "reserved-bits-clear"),
        ),
        (
            ("0xab", "reserved-bit0-set"),
            ("0xab", "reserved-bits-clear"),
        ),
        (
            ("0xab", "reserved-bit3-set"),
            ("0xab", "reserved-bits-clear"),
        ),
        (("0xab", "all-bits-set"), ("0xab", "all-bits-clear-rsv")),
        (
            ("0xcd", "reserved-bits-set"),
            ("0xcd", "reserved-bits-clear"),
        ),
        (
            ("0xcd", "reserved-bit6-set"),
            ("0xcd", "reserved-bits-clear"),
        ),
        (
            ("0xcd", "reserved-low-bits-set"),
            ("0xcd", "reserved-bits-clear"),
        ),
        (
            ("0xcd", "reserved-bit0-set"),
            ("0xcd", "reserved-bits-clear"),
        ),
        (
            ("0xcd", "reserved-bits-set-no-mac-gas"),
            ("0xcd", "reserved-bits-clear-no-mac-gas"),
        ),
        (
            ("0xd3", "reserved-bits-set"),
            ("0xd3", "reserved-bits-clear"),
        ),
        (
            ("0xd3", "reserved-bit5-set"),
            ("0xd3", "reserved-bits-clear"),
        ),
        (
            ("0xd3", "reserved-bit0-set"),
            ("0xd3", "reserved-bits-clear"),
        ),
        (
            ("0xd3", "reserved-bits-set-no-flags"),
            ("0xd3", "reserved-bits-clear-no-flags"),
        ),
    ];

    const TUNNELED_FRAME: &[u8] = &[
        0x31, 0x04, 0x0a, 0x00, 0x00, 0x0a, 0x4d, 0x65, 0x73, 0x68, 0x4e, 0x65, 0x74, 0x2d, 0x35,
        0x47, 0x01, 0x04, 0x82, 0x84, 0x8b, 0x96,
    ];
    const MIC32: &[u8] = &[
        0xc6, 0xd2, 0xfc, 0x7b, 0x9d, 0x49, 0x4b, 0xb3, 0x4b, 0x1b, 0x5a, 0x88, 0xb0, 0x12, 0x3f,
        0x00, 0xde, 0x38, 0x30, 0x67, 0x96, 0x49, 0x57, 0xf8, 0x8a, 0x5a, 0x8c, 0xb5, 0x86, 0x0a,
        0xd9, 0xe3,
    ];
    const MIC4: &[u8] = &[0xa1, 0xb2, 0xc3, 0xd4];
    const SIV48: &[u8] = &[
        0xd3, 0xd7, 0x18, 0x1a, 0x5e, 0xb6, 0x22, 0x06, 0xab, 0x37, 0xea, 0x51, 0x42, 0x24, 0x78,
        0x1b, 0xb6, 0x11, 0x68, 0x1a, 0x91, 0x7f, 0xce, 0xa6, 0xd0, 0x5b, 0x94, 0x61, 0x54, 0xd2,
        0xaa, 0x07, 0xcd, 0x94, 0xab, 0x31, 0x6c, 0xa3, 0xae, 0x45, 0x75, 0xe7, 0x8e, 0xb4, 0x63,
        0xcf, 0xee, 0x56,
    ];
    const SIV16: &[u8] = &[
        0x28, 0xf0, 0x6e, 0x22, 0x1e, 0x53, 0x40, 0x68, 0x44, 0xa6, 0xee, 0xb8, 0x41, 0x7f, 0xd8,
        0xf8,
    ];
    const ENCAP_FRAME18: &[u8] = &[
        0xd4, 0x02, 0x00, 0x3d, 0x49, 0xf7, 0xbb, 0xd9, 0xd5, 0xa0, 0x59, 0x2f, 0x48, 0xf8, 0x51,
        0x6f, 0x7d, 0xa2,
    ];
    const EAPOL32: &[u8] = &[
        0x02, 0x03, 0x00, 0x5f, 0x02, 0x00, 0x8a, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
        0x0e, 0x0f,
    ];
    const URI: &[u8] = b"DPP:V:2;C:81/1;M:02a0b1c2d3e4;K:MDkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDIgADxyz;;";
    const HASH32: &[u8] = &[
        0xb0, 0x1a, 0x19, 0xa8, 0xa6, 0xdc, 0xae, 0x6e, 0x59, 0xe6, 0x0c, 0x52, 0xb2, 0x87, 0x18,
        0x4e, 0xa9, 0x69, 0x7f, 0x18, 0xd9, 0xf4, 0x5f, 0xbb, 0xc4, 0x08, 0x00, 0x5e, 0x03, 0xbe,
        0xa2, 0x0d,
    ];
    const DPP_FRAME48: &[u8] = &[
        0x04, 0x09, 0x50, 0x6f, 0x9a, 0x1a, 0x01, 0x00, 0x66, 0xd3, 0x0d, 0x5c, 0x20, 0xfe, 0xeb,
        0x75, 0xdb, 0x46, 0x2c, 0x9b, 0xa1, 0x68, 0xa2, 0x35, 0xed, 0x1a, 0x54, 0xca, 0x41, 0x46,
        0x90, 0x18, 0xf9, 0x4b, 0xc1, 0x7b, 0xfc, 0x07, 0x43, 0xad, 0x0b, 0xe5, 0xa4, 0x17, 0x06,
        0x71, 0x56, 0x2f,
    ];
    /// Larger than any 2-octet length can describe.
    static OVERSIZED: [u8; 65_536] = [0; 65_536];

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0xc0" => sweep_row::<SourceInfo>(row, bytes, canonical),
            "0xc1" => sweep_row::<TunneledMessageType>(row, bytes, canonical),
            "0xc2" => sweep_row::<Tunneled<'_>>(row, bytes, canonical),
            "0xa9" => sweep_row::<Ieee1905LayerSecurityCapability>(row, bytes, canonical),
            "0xab" => sweep_row::<Mic<'_>>(row, bytes, canonical),
            "0xac" => sweep_row::<EncryptedPayload<'_>>(row, bytes, canonical),
            "0xcc" => sweep_row::<AkmSuiteCapabilities>(row, bytes, canonical),
            "0xcd" => sweep_row::<Ieee1905EncapDpp<'_>>(row, bytes, canonical),
            "0xce" => sweep_row::<Ieee1905EncapEapol<'_>>(row, bytes, canonical),
            "0xcf" => sweep_row::<DppBootstrappingUriNotification<'_>>(row, bytes, canonical),
            "0xd2" => sweep_row::<DppCceIndication>(row, bytes, canonical),
            "0xd3" => sweep_row_lenient::<DppChirpValue<'_>>(row, bytes, canonical, LENIENT_INPUTS),
            "0xd1" => sweep_row::<DppMessage<'_>>(row, bytes, canonical),
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

    fn counter(value: u64) -> Counter48 {
        Counter48::new(value).unwrap()
    }

    fn mic() -> Mic<'static> {
        Mic {
            gtk_key_id: 2,
            mic_version: 0,
            counter: counter(0x0102_0304_05a6),
            source_al_mac: STA,
            mic: MIC32,
        }
    }

    /// The common body of the `0xab` rows with a 4-octet MIC.
    fn mic4() -> Mic<'static> {
        Mic {
            gtk_key_id: 1,
            mic_version: 0,
            counter: counter(0x0102_0304_05a6),
            source_al_mac: AL_MAC,
            mic: MIC4,
        }
    }

    fn encrypted() -> EncryptedPayload<'static> {
        EncryptedPayload {
            counter: counter(0x0a0b_0c0d_0e0f),
            source_al_mac: STA,
            destination_al_mac: OTHER,
            aes_siv: SIV48,
        }
    }

    const fn suite(oui: [u8; 3], suite_type: u8) -> AkmSuite {
        AkmSuite { oui, suite_type }
    }

    fn akm() -> AkmSuiteCapabilities {
        AkmSuiteCapabilities {
            backhaul: vec![suite([0x00, 0x0f, 0xac], 8), suite([0x50, 0x6f, 0x9a], 2)],
            fronthaul: vec![
                suite([0x00, 0x0f, 0xac], 2),
                suite([0x00, 0x0f, 0xac], 18),
                suite([0x00, 0x10, 0x18], 255),
            ],
        }
    }

    fn encap_dpp() -> Ieee1905EncapDpp<'static> {
        Ieee1905EncapDpp {
            destination_sta_mac: Some(STA),
            gas_frame: false,
            frame_type: 1,
            frame: ENCAP_FRAME18,
        }
    }

    fn uri_notification() -> DppBootstrappingUriNotification<'static> {
        DppBootstrappingUriNotification {
            ruid: STA,
            bssid: OTHER,
            bsta_mac: THIRD,
            uri: URI,
        }
    }

    fn chirp() -> DppChirpValue<'static> {
        DppChirpValue {
            destination_sta_mac: Some(STA),
            hash_valid: true,
            hash: HASH32,
        }
    }

    fn chirp_no_mac() -> DppChirpValue<'static> {
        DppChirpValue {
            destination_sta_mac: None,
            ..chirp()
        }
    }

    fn akm_with_back(count: usize) -> AkmSuiteCapabilities {
        AkmSuiteCapabilities {
            backhaul: vec![suite([0x00, 0x0f, 0xac], 8); count],
            fronthaul: Vec::new(),
        }
    }

    body_checks! {
        mod source_info_checks;
        body: SourceInfo;
        golden: &fixture(("0xc0", "golden")), SourceInfo { sta_mac: STA };
        prefixes: |_| false, truncated_in(&[field(0, 6)]);
        trailing_byte: &fixture(("0xc0", "golden"));
    }
    body_checks! {
        mod tunneled_message_type_checks;
        body: TunneledMessageType;
        golden: &fixture(("0xc1", "golden")), TunneledMessageType { message_type: 4 };
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0xc1", "golden"));
        alternates: [
            (&fixture(("0xc1", "reserved-0x07")), TunneledMessageType { message_type: 7 }, &fixture(("0xc1", "reserved-0x07"))),
            (&fixture(("0xc1", "reserved-0xff")), TunneledMessageType { message_type: 0xff }, &fixture(("0xc1", "reserved-0xff")))
        ];
    }
    body_checks! {
        mod tunneled_checks;
        body: Tunneled<'a>;
        golden: &fixture(("0xc2", "golden")), Tunneled { body: TUNNELED_FRAME };
        // The body is the rest of the value, so every length is complete.
        prefixes: |_| true, truncated_in(&[]);
        alternates: [
            (&fixture(("0xc2", "empty")), Tunneled { body: &[] }, &fixture(("0xc2", "empty"))),
            (&fixture(("0xc2", "one-octet")), Tunneled { body: &[0xa5] }, &fixture(("0xc2", "one-octet")))
        ];
    }
    body_checks! {
        mod layer_security_checks;
        body: Ieee1905LayerSecurityCapability;
        golden: &fixture(("0xa9", "golden")), Ieee1905LayerSecurityCapability { onboarding_protocol: 0, mic_algorithm: 0, encryption_algorithm: 0 };
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 1), field(2, 1)]);
        trailing_byte: &fixture(("0xa9", "golden"));
        alternates: [(&fixture(("0xa9", "reserved-all-distinct")), Ieee1905LayerSecurityCapability { onboarding_protocol: 0x11, mic_algorithm: 0x22, encryption_algorithm: 0x33 }, &fixture(("0xa9", "reserved-all-distinct")))];
    }
    body_checks! {
        mod mic_checks;
        body: Mic<'a>;
        golden: &fixture(("0xab", "golden")), mic();
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 6), field(13, 2), field(15, 32)]);
        trailing_byte: &fixture(("0xab", "golden"));
        count_too_large: Mic { mic: &OVERSIZED, ..mic4() }, TOO_LONG_FOR_LENGTH;
        reserved_bits: [
            (&fixture(("0xab", "reserved-bits-set")), mic4(), &fixture(("0xab", "reserved-bits-clear"))),
            (&fixture(("0xab", "all-bits-set")), Mic { gtk_key_id: 3, mic_version: 3, ..mic4() }, &fixture(("0xab", "all-bits-clear-rsv")))
        ];
        alternates: [
            (&fixture(("0xab", "mic-len-0")), Mic { gtk_key_id: 0, mic: &[], ..mic4() }, &fixture(("0xab", "mic-len-0"))),
            (&fixture(("0xab", "counter-max")), Mic { gtk_key_id: 0, counter: counter(0xffff_ffff_ffff), ..mic4() }, &fixture(("0xab", "counter-max"))),
            (&fixture(("0xab", "version-3")), Mic { gtk_key_id: 0, mic_version: 3, ..mic4() }, &fixture(("0xab", "version-3")))
        ];
    }
    body_checks! {
        mod encrypted_payload_checks;
        body: EncryptedPayload<'a>;
        golden: &fixture(("0xac", "golden")), encrypted();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 6), field(12, 6), field(18, 2), field(20, 48)]);
        trailing_byte: &fixture(("0xac", "golden"));
        count_too_large: EncryptedPayload { aes_siv: &OVERSIZED, ..encrypted() }, TOO_LONG_FOR_LENGTH;
        alternates: [
            (&fixture(("0xac", "siv-len-0")), EncryptedPayload { aes_siv: &[], ..encrypted() }, &fixture(("0xac", "siv-len-0"))),
            (&fixture(("0xac", "counter-max")), EncryptedPayload { counter: counter(0xffff_ffff_ffff), aes_siv: SIV16, ..encrypted() }, &fixture(("0xac", "counter-max")))
        ];
    }
    body_checks! {
        mod akm_checks;
        body: AkmSuiteCapabilities;
        golden: &fixture(("0xcc", "golden")), akm();
        prefixes: |_| false, truncated_in(&[
            field(0, 1), field(1, 3), field(4, 1), field(5, 3), field(8, 1), field(9, 1),
            field(10, 3), field(13, 1), field(14, 3), field(17, 1), field(18, 3), field(21, 1)
        ]);
        trailing_byte: &fixture(("0xcc", "golden"));
        count_too_large: akm_with_back(256), TOO_MANY_BYTE_COUNT;
        alternates: [
            (&fixture(("0xcc", "zero-both")), AkmSuiteCapabilities { backhaul: vec![], fronthaul: vec![] }, &fixture(("0xcc", "zero-both"))),
            (&fixture(("0xcc", "zero-back")), AkmSuiteCapabilities { backhaul: vec![], fronthaul: vec![suite([0x00, 0x0f, 0xac], 2), suite([0x50, 0x6f, 0x9a], 2)] }, &fixture(("0xcc", "zero-back"))),
            (&fixture(("0xcc", "zero-front")), AkmSuiteCapabilities { backhaul: vec![suite([0x00, 0x0f, 0xac], 8), suite([0x00, 0x0f, 0xac], 18)], fronthaul: vec![] }, &fixture(("0xcc", "zero-front")))
        ];
    }
    body_checks! {
        mod encap_dpp_checks;
        body: Ieee1905EncapDpp<'a>;
        golden: &fixture(("0xcd", "golden")), encap_dpp();
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1), field(8, 2), field(10, 18)]);
        trailing_byte: &fixture(("0xcd", "golden"));
        count_too_large: Ieee1905EncapDpp { frame: &OVERSIZED, ..encap_dpp() }, TOO_LONG_FOR_LENGTH;
        reserved_bits: [
            (&fixture(("0xcd", "reserved-bits-set")), Ieee1905EncapDpp { frame_type: 1, frame: MIC4, ..encap_dpp() }, &fixture(("0xcd", "reserved-bits-clear"))),
            (&fixture(("0xcd", "reserved-bits-set-no-mac-gas")), Ieee1905EncapDpp { destination_sta_mac: None, gas_frame: true, frame_type: 255, frame: MIC4 }, &fixture(("0xcd", "reserved-bits-clear-no-mac-gas")))
        ];
        alternates: [
            (&fixture(("0xcd", "public-no-mac")), Ieee1905EncapDpp { destination_sta_mac: None, frame_type: 2, ..encap_dpp() }, &fixture(("0xcd", "public-no-mac"))),
            (&fixture(("0xcd", "gas-with-mac")), Ieee1905EncapDpp { gas_frame: true, frame_type: 255, ..encap_dpp() }, &fixture(("0xcd", "gas-with-mac"))),
            (&fixture(("0xcd", "gas-frame-type-not-255")), Ieee1905EncapDpp { gas_frame: true, frame_type: 0, frame: MIC4, ..encap_dpp() }, &fixture(("0xcd", "gas-frame-type-not-255"))),
            (&fixture(("0xcd", "frame-len-0")), Ieee1905EncapDpp { frame: &[], ..encap_dpp() }, &fixture(("0xcd", "frame-len-0")))
        ];
    }
    body_checks! {
        mod encap_eapol_checks;
        body: Ieee1905EncapEapol<'a>;
        golden: &fixture(("0xce", "golden")), Ieee1905EncapEapol { eapol: EAPOL32 };
        prefixes: |_| true, truncated_in(&[]);
        alternates: [(&fixture(("0xce", "empty")), Ieee1905EncapEapol { eapol: &[] }, &fixture(("0xce", "empty")))];
    }
    body_checks! {
        mod uri_notification_checks;
        body: DppBootstrappingUriNotification<'a>;
        golden: &fixture(("0xcf", "golden")), uri_notification();
        // The URI is the rest of the value, so every length from the fixed head on is complete.
        prefixes: |len| len >= 18, truncated_in(&[field(0, 6), field(6, 6), field(12, 6)]);
        alternates: [
            (&fixture(("0xcf", "empty-uri")), DppBootstrappingUriNotification { uri: &[], ..uri_notification() }, &fixture(("0xcf", "empty-uri"))),
            (&fixture(("0xcf", "short-uri")), DppBootstrappingUriNotification { uri: b"DPP:", ..uri_notification() }, &fixture(("0xcf", "short-uri")))
        ];
    }
    body_checks! {
        mod cce_indication_checks;
        body: DppCceIndication;
        golden: &fixture(("0xd2", "golden")), DppCceIndication { advertise_cce: 1 };
        prefixes: |_| false, truncated_in(&[field(0, 1)]);
        trailing_byte: &fixture(("0xd2", "golden"));
        alternates: [
            (&fixture(("0xd2", "disable")), DppCceIndication { advertise_cce: 0 }, &fixture(("0xd2", "disable"))),
            (&fixture(("0xd2", "reserved-0xff")), DppCceIndication { advertise_cce: 0xff }, &fixture(("0xd2", "reserved-0xff")))
        ];
    }
    body_checks! {
        mod chirp_value_checks;
        body: DppChirpValue<'a>;
        golden: &fixture(("0xd3", "golden")), chirp();
        lenient;
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1), field(8, 32)]);
        trailing_byte: &fixture(("0xd3", "golden"));
        count_too_large: DppChirpValue { hash: &OVERSIZED[..256], ..chirp() }, TOO_MANY_BYTE_COUNT;
        reserved_bits: [
            (&fixture(("0xd3", "reserved-bits-set")), DppChirpValue { hash: MIC4, ..chirp() }, &fixture(("0xd3", "reserved-bits-clear"))),
            (&fixture(("0xd3", "reserved-bits-set-no-flags")), DppChirpValue { destination_sta_mac: None, hash_valid: false, hash: MIC4 }, &fixture(("0xd3", "reserved-bits-clear-no-flags")))
        ];
        alternates: [
            (&fixture(("0xd3", "establish-no-mac")), chirp_no_mac(), &fixture(("0xd3", "establish-no-mac"))),
            (&fixture(("0xd3", "purge-with-mac")), DppChirpValue { destination_sta_mac: Some(THIRD), hash_valid: false, hash: HASH32 }, &fixture(("0xd3", "purge-with-mac"))),
            (&fixture(("0xd3", "hash-len-0")), DppChirpValue { hash: &[], ..chirp() }, &fixture(("0xd3", "hash-len-0"))),
            (&fixture(("0xd3", "hash-len-0-no-mac")), DppChirpValue { hash: &[], ..chirp_no_mac() }, &fixture(("0xd3", "hash-len-0-no-mac")))
        ];
        lenient: [
            (&fixture(("0xd3", "upstream-2-octet-hash-length")), chirp(), &fixture(("0xd3", "golden"))),
            (&fixture(("0xd3", "upstream-2-octet-hash-length-no-mac")), chirp_no_mac(), &fixture(("0xd3", "establish-no-mac")))
        ];
        lenient_rejects: [
            // The upstream form with one hash octet missing: the length says 32, 31 follow.
            (&fixture(("0xd3", "upstream-2-octet-hash-length-no-mac"))[..34], DecodeError::TrailingBytes { remaining: 32 }),
            // The upstream form with an extra octet after the hash.
            (&[fixture(("0xd3", "upstream-2-octet-hash-length-no-mac")), vec![0xa5]].concat(), DecodeError::TrailingBytes { remaining: 34 }),
            // A 2-octet length of 0 is not an upstream form (the length must be above 0).
            (&[0x40, 0x00, 0x00], DecodeError::TrailingBytes { remaining: 1 })
        ];
    }
    body_checks! {
        mod dpp_message_checks;
        body: DppMessage<'a>;
        golden: &fixture(("0xd1", "golden")), DppMessage { frame: DPP_FRAME48 };
        prefixes: |_| true, truncated_in(&[]);
        alternates: [
            (&fixture(("0xd1", "empty")), DppMessage { frame: &[] }, &fixture(("0xd1", "empty"))),
            (&fixture(("0xd1", "one-octet")), DppMessage { frame: &[0xa5] }, &fixture(("0xd1", "one-octet")))
        ];
    }

    #[test]
    fn mic_key_id_and_version_each_use_their_own_bits() {
        let cases = [
            (0x40, 1, 0),
            (0x80, 2, 0),
            (0xc0, 3, 0),
            (0x10, 0, 1),
            (0x20, 0, 2),
            (0x30, 0, 3),
        ];
        for (octet, gtk_key_id, mic_version) in cases {
            let body = Mic {
                gtk_key_id,
                mic_version,
                counter: counter(0),
                source_al_mac: ZERO_MAC,
                mic: &[],
            };
            let mut bytes = vec![0; 15];
            bytes[0] = octet;
            assert_eq!(Mic::decode_value(&bytes), Ok(body), "decode {octet:#04x}");
            assert_eq!(body.encode_value(), Ok(bytes), "encode {octet:#04x}");
        }
    }

    #[test]
    fn mic_fields_wider_than_two_bits_do_not_encode() {
        let wide_key = Mic {
            gtk_key_id: 4,
            ..mic4()
        };
        assert_eq!(
            wide_key.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 4, max: 3 })
        );
        let wide_version = Mic {
            mic_version: 4,
            ..mic4()
        };
        assert_eq!(
            wide_version.encode_value(),
            Err(EncodeError::ValueTooLarge { value: 4, max: 3 })
        );
    }

    #[test]
    fn counters_are_big_endian_48_bit_values() {
        let body = Mic {
            counter: counter(0x0102_0304_0506),
            ..mic4()
        };
        assert_eq!(body.encode_value().unwrap()[1..7], [1, 2, 3, 4, 5, 6]);
        let body = EncryptedPayload {
            counter: counter(0x0102_0304_0506),
            ..encrypted()
        };
        assert_eq!(body.encode_value().unwrap()[0..6], [1, 2, 3, 4, 5, 6]);
        let max = Mic {
            counter: counter(Counter48::MAX),
            ..mic4()
        };
        assert_eq!(max.encode_value().unwrap()[1..7], [0xff; 6]);
    }

    #[test]
    fn counters_between_32_and_48_bits_encode() {
        let body = Mic {
            counter: counter(1 << 32),
            ..mic4()
        };
        assert_eq!(body.encode_value().unwrap()[1..7], [0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn counter48_rejects_values_above_48_bits() {
        assert_eq!(Counter48::MAX, 0xffff_ffff_ffff);
        assert_eq!(
            Counter48::new(Counter48::MAX).map(Counter48::get),
            Some(Counter48::MAX)
        );
        assert_eq!(Counter48::new(Counter48::MAX + 1), None);
        assert_eq!(Counter48::new(u64::MAX), None);
    }

    #[test]
    fn maximum_counter_round_trips() {
        let mic = Mic {
            counter: counter(Counter48::MAX),
            ..mic4()
        };
        let mic_bytes = mic.encode_value().unwrap();
        assert_eq!(mic_bytes[1..7], [0xff; 6]);
        assert_eq!(Mic::decode_value(&mic_bytes), Ok(mic));
        let encrypted = EncryptedPayload {
            counter: counter(Counter48::MAX),
            ..encrypted()
        };
        let encrypted_bytes = encrypted.encode_value().unwrap();
        assert_eq!(encrypted_bytes[0..6], [0xff; 6]);
        assert_eq!(
            EncryptedPayload::decode_value(&encrypted_bytes),
            Ok(encrypted)
        );
    }

    #[test]
    fn lengths_are_derived_from_the_data_and_big_endian() {
        let mic_bytes = vec![0x5a; 258];
        let body = Mic {
            mic: &mic_bytes,
            ..mic4()
        };
        assert_eq!(body.encode_value().unwrap()[13..15], [0x01, 0x02]);
        let siv_bytes = vec![0x5a; 258];
        let body = EncryptedPayload {
            aes_siv: &siv_bytes,
            ..encrypted()
        };
        assert_eq!(body.encode_value().unwrap()[18..20], [0x01, 0x02]);
        let frame_bytes = vec![0x5a; 258];
        let body = Ieee1905EncapDpp {
            frame: &frame_bytes,
            destination_sta_mac: None,
            ..encap_dpp()
        };
        assert_eq!(body.encode_value().unwrap()[2..4], [0x01, 0x02]);
    }

    #[test]
    fn encap_dpp_flags_each_use_their_own_bit() {
        let mac_alone = Ieee1905EncapDpp {
            destination_sta_mac: Some(ZERO_MAC),
            gas_frame: false,
            frame_type: 0,
            frame: &[],
        };
        let mut bytes = vec![0; 10];
        bytes[0] = 0x80;
        assert_eq!(Ieee1905EncapDpp::decode_value(&bytes), Ok(mac_alone));
        assert_eq!(mac_alone.encode_value(), Ok(bytes));
        let gas_alone = Ieee1905EncapDpp {
            destination_sta_mac: None,
            gas_frame: true,
            frame_type: 0,
            frame: &[],
        };
        let bytes = vec![0x20, 0, 0, 0];
        assert_eq!(Ieee1905EncapDpp::decode_value(&bytes), Ok(gas_alone));
        assert_eq!(gas_alone.encode_value(), Ok(bytes));
    }

    #[test]
    fn chirp_flags_each_use_their_own_bit() {
        let mac_alone = DppChirpValue {
            destination_sta_mac: Some(ZERO_MAC),
            hash_valid: false,
            hash: &[],
        };
        let mut bytes = vec![0; 8];
        bytes[0] = 0x80;
        assert_eq!(DppChirpValue::decode_value(&bytes), Ok(mac_alone));
        assert_eq!(mac_alone.encode_value(), Ok(bytes));
        let valid_alone = DppChirpValue {
            destination_sta_mac: None,
            hash_valid: true,
            hash: &[],
        };
        let bytes = vec![0x40, 0];
        assert_eq!(DppChirpValue::decode_value(&bytes), Ok(valid_alone));
        assert_eq!(valid_alone.encode_value(), Ok(bytes));
    }

    #[test]
    fn chirp_hash_length_is_one_octet() {
        // intentional-difference: upstream ec_util.cpp:337,356-358 (receiver :300-302) writes and
        // reads the Hash Length as 2 octets, big-endian (audit row 247); the spec has 1 octet.
        // The strict decoder refuses upstream's form, and the encoder writes the spec form.
        let upstream = fixture(("0xd3", "upstream-2-octet-hash-length"));
        assert_eq!(
            DppChirpValue::decode_value(&upstream),
            Err(DecodeError::TrailingBytes { remaining: 33 })
        );
        let spec = fixture(("0xd3", "golden"));
        assert_eq!(upstream.len(), spec.len() + 1);
        // Upstream's high length octet is inserted before the spec's 1-octet length.
        assert_eq!(upstream[..7], spec[..7]);
        assert_eq!(upstream[7], 0x00);
        assert_eq!(upstream[8..], spec[7..]);
        assert_eq!(chirp().encode_value(), Ok(spec));
    }

    #[test]
    fn chirp_lenient_decoder_reads_upstreams_full_two_octet_length() {
        // Upstream writes a uint16_t (ec_util.cpp:356-358) from a received attribute's length
        // (ec_pa_configurator.cpp:54), so 256 octets is a form its sender can write.
        let hash = [0xAB; 256];
        let mut upstream = vec![HASH_VALID, 0x01, 0x00];
        upstream.extend_from_slice(&hash);
        let body = DppChirpValue::decode_value_lenient(&upstream)
            .unwrap()
            .unwrap();
        assert_eq!(body.destination_sta_mac, None);
        assert!(body.hash_valid);
        assert_eq!(body.hash, hash);
        assert!(DppChirpValue::decode_value(&upstream).is_err());
        assert_eq!(body.encode_value(), Err(TOO_MANY_BYTE_COUNT));

        // The length must still equal the octets left.
        upstream.pop();
        assert!(DppChirpValue::decode_value_lenient(&upstream).is_err());
    }

    #[test]
    fn tunneled_fills_a_tlv_through_the_builder() {
        let bytes = vec![0xa5; 65_535];
        let body = Tunneled { body: &bytes };
        let message = crate::body_testkit::build_message(&body).unwrap();
        let tlv = crate::body_testkit::single_tlv(&message).unwrap();
        let decoded = tlv.decode::<Tunneled<'_>>().unwrap();
        assert_eq!(decoded.body, bytes);
        assert_eq!(decoded.body.as_ptr(), tlv.value.as_ptr());
    }

    #[test]
    fn chirp_lenient_decoder_gives_the_spec_body_for_upstreams_form() {
        let upstream = fixture(("0xd3", "upstream-2-octet-hash-length"));
        assert_eq!(
            DppChirpValue::decode_value_lenient(&upstream),
            Ok(Some(chirp()))
        );
        let upstream = fixture(("0xd3", "upstream-2-octet-hash-length-no-mac"));
        assert_eq!(
            DppChirpValue::decode_value_lenient(&upstream),
            Ok(Some(chirp_no_mac()))
        );
        // The spec form is not touched, whatever its Hash Validity.
        let purge = fixture(("0xd3", "purge-no-mac"));
        assert_eq!(
            DppChirpValue::decode_value_lenient(&purge),
            DppChirpValue::decode_value(&purge).map(Some)
        );
        // An empty value is not an absent form for this TLV.
        assert_eq!(
            DppChirpValue::decode_value_lenient(&[]),
            Err(DecodeError::Truncated {
                needed: 1,
                remaining: 0
            })
        );
    }

    #[test]
    fn chirp_keeps_the_hash_whatever_the_validity_says() {
        // intentional-difference: upstream ec_util.cpp:295-298 ignores the hash when Hash
        // Validity is 0, and :303-307 rejects a Hash Length of 0 when it is 1 (audit rows 248,
        // 250). This body always reads Hash Length and Hash Value and accepts a length of 0.
        let purge = fixture(("0xd3", "purge-with-mac"));
        let body = DppChirpValue::decode_value(&purge).unwrap();
        assert!(!body.hash_valid);
        assert_eq!(body.hash, HASH32);
        assert_eq!(body.encode_value(), Ok(purge));
        let empty = fixture(("0xd3", "hash-len-0-no-mac"));
        let body = DppChirpValue::decode_value(&empty).unwrap();
        assert!(body.hash_valid);
        assert_eq!(body.hash, &[] as &[u8]);
        assert_eq!(body.encode_value(), Ok(empty));
    }

    #[test]
    fn chirp_and_encap_mac_follow_the_flag() {
        // The flag decides the layout; the encoders derive the flag from the Option.
        assert_eq!(chirp().encode_value().unwrap()[0], 0xc0);
        assert_eq!(chirp_no_mac().encode_value().unwrap()[0], 0x40);
        assert_eq!(encap_dpp().encode_value().unwrap()[0], 0x80);
        let no_mac = Ieee1905EncapDpp {
            destination_sta_mac: None,
            ..encap_dpp()
        };
        assert_eq!(no_mac.encode_value().unwrap()[0], 0x00);
    }

    #[test]
    fn encap_dpp_rejects_what_the_upstream_receiver_misreads() {
        // intentional-difference: upstream ec_util.cpp:402 accepts a frame length up to 3 above
        // the data (audit row 242), and :381-388 zeroes the MAC and reads on when bit 7 is set
        // with fewer than 6 octets left (audit row 243). This body reports Truncated.
        assert_eq!(
            Ieee1905EncapDpp::decode_value(&fixture(("0xcd", "frame-overstated-by-one"))),
            Err(DecodeError::Truncated {
                needed: 4,
                remaining: 3
            })
        );
        assert_eq!(
            Ieee1905EncapDpp::decode_value(&fixture(("0xcd", "cut-mac"))),
            Err(DecodeError::Truncated {
                needed: 6,
                remaining: 3
            })
        );
    }

    #[test]
    fn akm_suite_lists_are_not_capped_at_ten() {
        // intentional-difference: upstream em_configuration.cpp:5095,5105 keeps at most
        // EM_MAX_AKMS (10, em_base.h:230) suites per BSS (audit row 240); the spec counts go to
        // 255 and this body keeps every suite.
        let bytes = fixture(("0xcc", "eleven-back"));
        let body = AkmSuiteCapabilities::decode_value(&bytes).unwrap();
        assert_eq!(body.backhaul.len(), 11);
        assert_eq!(body.encode_value(), Ok(bytes));
        let bytes = fixture(("0xcc", "back-255"));
        let body = AkmSuiteCapabilities::decode_value(&bytes).unwrap();
        assert_eq!(body.backhaul.len(), 255);
        assert_eq!(body.encode_value(), Ok(bytes));
    }

    #[test]
    fn akm_count_larger_than_the_data_is_truncated_without_a_large_allocation() {
        assert_eq!(
            AkmSuiteCapabilities::decode_value(&[255]),
            Err(DecodeError::Truncated {
                needed: 3,
                remaining: 0
            })
        );
        assert_eq!(
            AkmSuiteCapabilities::decode_value(&[0, 255]),
            Err(DecodeError::Truncated {
                needed: 3,
                remaining: 0
            })
        );
    }

    #[test]
    fn empty_payloads_are_spec_valid() {
        // intentional-difference: upstream em_provisioning.cpp:392-395 (1905 Encap EAPOL, audit
        // row 244) and :540 (DPP Message, audit row 257) refuse a zero-length frame. The spec
        // has no minimum, so an empty value decodes and encodes; the consumer applies the policy.
        assert_eq!(
            Ieee1905EncapEapol::decode_value(&[]),
            Ok(Ieee1905EncapEapol { eapol: &[] })
        );
        assert_eq!(Ieee1905EncapEapol { eapol: &[] }.encode_value(), Ok(vec![]));
        assert_eq!(DppMessage::decode_value(&[]), Ok(DppMessage { frame: &[] }));
        assert_eq!(DppMessage { frame: &[] }.encode_value(), Ok(vec![]));
        assert_eq!(Tunneled::decode_value(&[]), Ok(Tunneled { body: &[] }));
    }

    #[test]
    fn cce_indication_keeps_every_octet_raw() {
        // intentional-difference: upstream em_provisioning.cpp:416-421 treats every value but 1
        // as an error, so a Disable is never acted on (audit row 246). This body keeps the octet.
        for advertise_cce in [0, 1, 2, 0xff] {
            let body = DppCceIndication { advertise_cce };
            assert_eq!(body.encode_value(), Ok(vec![advertise_cce]));
            assert_eq!(DppCceIndication::decode_value(&[advertise_cce]), Ok(body));
        }
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<SourceInfo>() <= 6);
        assert!(size_of::<TunneledMessageType>() <= 1);
        assert!(size_of::<Tunneled<'_>>() <= 2 * word);
        assert!(size_of::<Ieee1905LayerSecurityCapability>() <= 3);
        assert!(size_of::<Mic<'_>>() <= 16 + 2 * word);
        assert!(size_of::<EncryptedPayload<'_>>() <= 24 + 2 * word);
        assert!(size_of::<AkmSuite>() <= 4);
        assert!(size_of::<AkmSuiteCapabilities>() <= 6 * word);
        assert!(size_of::<Ieee1905EncapDpp<'_>>() <= 16 + 2 * word);
        assert!(size_of::<Ieee1905EncapEapol<'_>>() <= 2 * word);
        assert!(size_of::<DppBootstrappingUriNotification<'_>>() <= 24 + 2 * word);
        assert!(size_of::<DppCceIndication>() <= 1);
        assert!(size_of::<DppChirpValue<'_>>() <= 16 + 2 * word);
        assert!(size_of::<DppMessage<'_>>() <= 2 * word);
    }
}
