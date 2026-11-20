// SPDX-License-Identifier: Apache-2.0

//! Steering, client association control, backhaul steering and code bodies (note 0009, B08).

use em_types::{MacAddr, TlvType};

use super::{TlvBody, TlvBodyLenient, bounded_vec};
use crate::body::{Reader, Writer};
use crate::error::{DecodeError, EncodeError};

/// Association Type bit of the Client Association Event octet: set when the client joined.
const ASSOCIATION_JOINED: u8 = 0x80;

/// Request Mode bit of the Steering Request octet: set for a steering mandate.
const REQUEST_MODE_MANDATE: u8 = 0x80;
/// BTM Disassociation Imminent bit of the Steering Request octet.
const BTM_DISASSOCIATION_IMMINENT: u8 = 0x40;
/// BTM Abridged bit of the Steering Request octet.
const BTM_ABRIDGED: u8 = 0x20;
/// BTM Link Removal Imminent bit of the Steering Request octet.
const BTM_LINK_REMOVAL_IMMINENT: u8 = 0x10;

/// Client Association Event, `EasyMesh` §17.2.20 Table 42; audit rows 81–82.
/// Upstream `em_client_assoc_event_t` (`em_base.h:475-480`), sender
/// `em_configuration.cpp:405-436` (`create_client_assoc_event_tlv`), receiver
/// `handle_topology_notification` (`em_configuration.cpp:2305-2361`). Bits 6-0 of the last octet
/// are reserved: decoding ignores
/// them and encoding writes zero. The receiver there does not check the TLV length (audit 82); the
/// strict decoder here requires exactly 13 octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientAssociationEvent {
    /// The client MAC, or the client MLD MAC address.
    pub sta_mac: MacAddr,
    /// The BSSID, or the AP MLD MAC address, where the event happened.
    pub ap_mac: MacAddr,
    /// `true` if the client joined the BSS, `false` if it left.
    pub joined: bool,
}

impl TlvBody<'_> for ClientAssociationEvent {
    const TLV_TYPE: TlvType = TlvType::CLIENT_ASSOCIATION_EVENT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let sta_mac = reader.mac()?;
        let ap_mac = reader.mac()?;
        let event_octet = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            sta_mac,
            ap_mac,
            joined: event_octet & ASSOCIATION_JOINED != 0,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.sta_mac);
        writer.mac(self.ap_mac);
        writer.u8(if self.joined { ASSOCIATION_JOINED } else { 0 });
        Ok(writer.into_bytes())
    }
}

/// The three BTM flags of a Steering Request or Profile-2 Steering Request, set as in the BTM
/// Request frame (IEEE 802.11 §9.6.13.9). The Request Mode bit is the kind of
/// [`SteeringMode`], and bits 3-0 of the octet are reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SteeringFlags {
    /// BTM Disassociation Imminent (bit 6).
    pub disassociation_imminent: bool,
    /// BTM Abridged (bit 5).
    pub abridged: bool,
    /// BTM Link Removal Imminent (bit 4).
    pub link_removal_imminent: bool,
}

impl SteeringFlags {
    fn from_octet(octet: u8) -> Self {
        Self {
            disassociation_imminent: octet & BTM_DISASSOCIATION_IMMINENT != 0,
            abridged: octet & BTM_ABRIDGED != 0,
            link_removal_imminent: octet & BTM_LINK_REMOVAL_IMMINENT != 0,
        }
    }

    /// The flag bits of the octet, without the Request Mode bit.
    fn to_bits(self) -> u8 {
        let mut bits = 0;
        if self.disassociation_imminent {
            bits |= BTM_DISASSOCIATION_IMMINENT;
        }
        if self.abridged {
            bits |= BTM_ABRIDGED;
        }
        if self.link_removal_imminent {
            bits |= BTM_LINK_REMOVAL_IMMINENT;
        }
        bits
    }
}

/// The Request Mode of a Steering Request: a steering opportunity, or a steering mandate with
/// its target list. `T` is the target record of the TLV.
///
/// The spec sets `Target_BSSID_List_Count` to zero in opportunity mode, so an opportunity cannot
/// hold targets here (note 0009, A8). A mandate may list any number of targets, including none.
#[derive(Debug, PartialEq, Eq)]
pub enum SteeringMode<T> {
    /// Request Mode 0: the request is a steering opportunity.
    Opportunity,
    /// Request Mode 1: the request is a steering mandate.
    Mandate {
        /// Target BSSIDs: one for all listed STAs, or one per STA in the same order.
        targets: Vec<T>,
    },
}

impl<T> SteeringMode<T> {
    fn mode_bit(&self) -> u8 {
        match self {
            Self::Opportunity => 0,
            Self::Mandate { .. } => REQUEST_MODE_MANDATE,
        }
    }

    fn targets(&self) -> &[T] {
        match self {
            Self::Opportunity => &[],
            Self::Mandate { targets } => targets,
        }
    }

    /// Builds the mode from the Request Mode bit and the decoded targets. An opportunity drops
    /// them, which only a lenient decode reaches with a non-empty list.
    fn from_octet(octet: u8, targets: Vec<T>) -> Self {
        if octet & REQUEST_MODE_MANDATE == 0 {
            Self::Opportunity
        } else {
            Self::Mandate { targets }
        }
    }
}

/// A target BSS of a Steering Request, `EasyMesh` §17.2.29 Table 51.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteeringTarget {
    /// Target BSSID or AP MLD MAC address; `FF:FF:FF:FF:FF:FF` is the wildcard.
    pub bssid: MacAddr,
    /// Target BSS operating class. A receiver ignores it for the wildcard BSSID.
    pub operating_class: u8,
    /// Target BSS channel number. A receiver ignores it for the wildcard BSSID.
    pub channel: u8,
}

/// Steering Request, `EasyMesh` §17.2.29 Table 51; audit rows 12–17.
/// Upstream `em_steering_req_t` (`em_base.h:1168-1183`), sender `em_steering.cpp:469-498`
/// (`create_btm_request_tlv`), receiver `em_steering.cpp:543-567`.
///
/// - `Steering_Opportunity_Window` is little-endian, as upstream writes it with a plain assignment
///   (`em_steering.cpp:485`, audit 12, note 0009 D6). `BTM_Disassociation_Timer` is big-endian
///   (`htons`, `:487`, audit 15). Profile-2 Steering Request has no upstream sender, so it takes
///   both fields big-endian.
/// - Upstream reads one STA and one target at fixed offsets (audit 14); this body decodes both
///   lists with their counts, and checks the length (audit 16).
/// - The upstream sender always writes one target, also in opportunity mode (`em_steering.cpp:490`,
///   audit 13). The
///   strict decoder rejects a non-zero target count in opportunity mode with
///   [`DecodeError::ValueOutOfRange`] (note 0009, A8). The lenient decoder accepts it and drops
///   the targets.
/// - Reserved bits 3-0 of the mode octet are ignored on decode and written as zero (audit 17).
///
/// The window is carried in both modes, and is only meaningful for an opportunity.
#[derive(Debug, PartialEq, Eq)]
pub struct SteeringRequest {
    /// BSSID of the source BSS the request applies to.
    pub bssid: MacAddr,
    /// Opportunity or mandate, with the targets of a mandate.
    pub mode: SteeringMode<SteeringTarget>,
    /// The BTM Request flags.
    pub flags: SteeringFlags,
    /// `Steering_Opportunity_Window` in seconds, little-endian on the wire.
    pub opportunity_window: u16,
    /// `BTM_Disassociation_Timer` in TUs, big-endian on the wire.
    pub disassociation_timer: u16,
    /// The STAs the request applies to; none means all associated STAs.
    pub stas: Vec<MacAddr>,
}

impl SteeringRequest {
    /// Decodes the value. A lenient decode accepts a target list in opportunity mode and drops it.
    fn read(value: &[u8], lenient: bool) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let bssid = reader.mac()?;
        let mode_octet = reader.u8()?;
        let opportunity_window = reader.le_u16()?;
        let disassociation_timer = reader.be_u16()?;
        let sta_count = usize::from(reader.u8()?);
        let mut stas = bounded_vec(sta_count, reader.remaining(), 6);
        for _ in 0..sta_count {
            stas.push(reader.mac()?);
        }
        let target_count = reader.u8()?;
        let is_opportunity = mode_octet & REQUEST_MODE_MANDATE == 0;
        if is_opportunity && target_count != 0 && !lenient {
            return Err(DecodeError::ValueOutOfRange {
                value: u32::from(target_count),
            });
        }
        let target_count = usize::from(target_count);
        let mut targets = bounded_vec(target_count, reader.remaining(), 8);
        for _ in 0..target_count {
            targets.push(SteeringTarget {
                bssid: reader.mac()?,
                operating_class: reader.u8()?,
                channel: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            bssid,
            mode: SteeringMode::from_octet(mode_octet, targets),
            flags: SteeringFlags::from_octet(mode_octet),
            opportunity_window,
            disassociation_timer,
            stas,
        })
    }
}

impl TlvBody<'_> for SteeringRequest {
    const TLV_TYPE: TlvType = TlvType::STEERING_REQUEST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        Self::read(value, false)
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.u8(self.mode.mode_bit() | self.flags.to_bits());
        writer.le_u16(self.opportunity_window);
        writer.be_u16(self.disassociation_timer);
        writer.count_u8(self.stas.len())?;
        for sta in &self.stas {
            writer.mac(*sta);
        }
        let targets = self.mode.targets();
        writer.count_u8(targets.len())?;
        for target in targets {
            writer.mac(target.bssid);
            writer.u8(target.operating_class);
            writer.u8(target.channel);
        }
        Ok(writer.into_bytes())
    }
}

impl TlvBodyLenient<'_> for SteeringRequest {
    fn decode_value_lenient(value: &[u8]) -> Result<Option<Self>, DecodeError> {
        Self::read(value, true).map(Some)
    }
}

/// Steering BTM Report, `EasyMesh` §17.2.30 Table 52; audit rows 108–110.
/// Upstream `em_steering_btm_rprt_t` (`em_base.h:1161-1166`), sender
/// `em_steering.cpp:500-524` (`create_btm_report_tlv`), receiver `em_steering.cpp:569-594`.
/// The Target BSSID is 0 or 6 octets, so the value is 13 or 19 octets. The upstream sender never
/// writes the target (audit 108), which is the 13-octet spec form, so there is no lenient decoder.
/// One to five octets after the status are `Truncated`, and more are `TrailingBytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteeringBtmReport {
    /// BSSID, or AP MLD MAC address, of the source BSS.
    pub bssid: MacAddr,
    /// The client MAC, or the client MLD MAC address.
    pub sta_mac: MacAddr,
    /// BTM Status Code from the client's BTM Response (IEEE 802.11 Table 9-428), raw.
    pub status_code: u8,
    /// The BSSID the client intends to roam to, if the BTM Response carried one.
    pub target_bssid: Option<MacAddr>,
}

impl TlvBody<'_> for SteeringBtmReport {
    const TLV_TYPE: TlvType = TlvType::STEERING_BTM_REPORT;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let bssid = reader.mac()?;
        let sta_mac = reader.mac()?;
        let status_code = reader.u8()?;
        let target_bssid = if reader.remaining() == 0 {
            None
        } else {
            Some(reader.mac()?)
        };
        reader.finish()?;
        Ok(Self {
            bssid,
            sta_mac,
            status_code,
            target_bssid,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.mac(self.sta_mac);
        writer.u8(self.status_code);
        if let Some(target_bssid) = self.target_bssid {
            writer.mac(target_bssid);
        }
        Ok(writer.into_bytes())
    }
}

/// Client Association Control Request, `EasyMesh` §17.2.31 Table 53; audit rows 111–114.
/// Upstream `em_client_assoc_ctrl_req_t` (`em_base.h:1153-1159`), sender
/// `em_steering.cpp:92-139`, receiver `em_steering.cpp:629-708`, `em_agent.cpp:760-849`.
///
/// Validity Period is big-endian (`htons` at `em_steering.cpp:79`). The control code is kept raw:
/// 0 block, 1 unblock, 2 timed block, 3 indefinite block, 4-255 reserved. Upstream refuses every
/// code but 0 and 1 (`em_steering.cpp:667-699`, audit 112), and more than 20 STAs
/// (`em_steering.cpp:658`, `em_base.h:122`, audit 113). This body accepts all codes and up to 255
/// STAs. The spec requires one or more STAs, so a count of zero is
/// [`DecodeError::InvalidLength`], and an empty list does not encode.
#[derive(Debug, PartialEq, Eq)]
pub struct ClientAssociationControlRequest {
    /// BSSID of the BSS the request applies to.
    pub bssid: MacAddr,
    /// Association Control code, raw.
    pub control: u8,
    /// Validity Period in seconds.
    pub validity_period: u16,
    /// The STAs, or STA MLD MAC addresses, the request applies to; at least one.
    pub stas: Vec<MacAddr>,
}

impl TlvBody<'_> for ClientAssociationControlRequest {
    const TLV_TYPE: TlvType = TlvType::CLIENT_ASSOCIATION_CONTROL_REQUEST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let bssid = reader.mac()?;
        let control = reader.u8()?;
        let validity_period = reader.be_u16()?;
        let sta_count = usize::from(reader.u8()?);
        if sta_count == 0 {
            return Err(DecodeError::InvalidLength { len: 0 });
        }
        let mut stas = bounded_vec(sta_count, reader.remaining(), 6);
        for _ in 0..sta_count {
            stas.push(reader.mac()?);
        }
        reader.finish()?;
        Ok(Self {
            bssid,
            control,
            validity_period,
            stas,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        if self.stas.is_empty() {
            return Err(EncodeError::CountTooSmall { len: 0, min: 1 });
        }
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.u8(self.control);
        writer.be_u16(self.validity_period);
        writer.count_u8(self.stas.len())?;
        for sta in &self.stas {
            writer.mac(*sta);
        }
        Ok(writer.into_bytes())
    }
}

/// Backhaul Steering Request, `EasyMesh` §17.2.32 Table 54; audit row 115.
/// Upstream `em_bh_steering_req_t` (`em_base.h:1146-1151`); there is no production sender or
/// receiver, only a test generator (`validation_test.cpp:1232-1262`). The value is exactly 14
/// octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackhaulSteeringRequest {
    /// MAC of the associated backhaul STA, or bSTA MLD, of the Multi-AP Agent.
    pub mac: MacAddr,
    /// BSSID of the target BSS, or its AP MLD MAC address.
    pub target_bssid: MacAddr,
    /// Target BSS operating class.
    pub operating_class: u8,
    /// Channel the target BSS sends Beacon frames on.
    pub channel: u8,
}

impl TlvBody<'_> for BackhaulSteeringRequest {
    const TLV_TYPE: TlvType = TlvType::BACKHAUL_STEERING_REQUEST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let mac = reader.mac()?;
        let target_bssid = reader.mac()?;
        let operating_class = reader.u8()?;
        let channel = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            mac,
            target_bssid,
            operating_class,
            channel,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.mac);
        writer.mac(self.target_bssid);
        writer.u8(self.operating_class);
        writer.u8(self.channel);
        Ok(writer.into_bytes())
    }
}

/// Backhaul Steering Response, `EasyMesh` §17.2.33 Table 55; audit row 116.
/// Upstream `em_bh_steering_resp_t` (`em_base.h:1140-1144`); there is no production sender or
/// receiver, only a test generator (`validation_test.cpp:1188-1230`). The value is exactly 13
/// octets. The result code is kept raw: 0 success, 1 failure, 2-255 reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackhaulSteeringResponse {
    /// MAC of the associated backhaul STA, or bSTA MLD, of the Multi-AP Agent.
    pub mac: MacAddr,
    /// BSSID of the target BSS, or its AP MLD MAC address.
    pub target_bssid: MacAddr,
    /// Result code, raw.
    pub result: u8,
}

impl TlvBody<'_> for BackhaulSteeringResponse {
    const TLV_TYPE: TlvType = TlvType::BACKHAUL_STEERING_RESPONSE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let mac = reader.mac()?;
        let target_bssid = reader.mac()?;
        let result = reader.u8()?;
        reader.finish()?;
        Ok(Self {
            mac,
            target_bssid,
            result,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.mac);
        writer.mac(self.target_bssid);
        writer.u8(self.result);
        Ok(writer.into_bytes())
    }
}

/// Higher Layer Data, `EasyMesh` §17.2.34 Table 56 and Appendix A.1 Table 130; audit rows 117–118.
/// Upstream has no production sender or receiver (`em_msg.cpp:928-931`, test generator
/// `validation_test.cpp:1120-1150`); its `em_higher_layer_data_t` cannot describe an inline
/// payload (audit 118). The payload is the rest of the value, and may be empty. The protocol is
/// kept raw: 1 is the TR-181 transport protocol, 0 and 2-255 are reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HigherLayerData<'a> {
    /// Higher layer protocol, raw.
    pub protocol: u8,
    /// Protocol payload, raw.
    pub payload: &'a [u8],
}

impl<'a> TlvBody<'a> for HigherLayerData<'a> {
    const TLV_TYPE: TlvType = TlvType::HIGHER_LAYER_DATA;

    fn decode_value(value: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let protocol = reader.u8()?;
        let payload = reader.rest();
        reader.finish()?;
        Ok(Self { protocol, payload })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.protocol);
        writer.bytes(self.payload);
        Ok(writer.into_bytes())
    }
}

/// Error Code, `EasyMesh` §17.2.36 Table 58; audit rows 122–126.
/// Upstream `em_error_code_t` (`em_base.h:1133-1138`), senders `em_metrics.cpp:2384-2411`,
/// `em_capability.cpp:602-617`, `em_steering.cpp:527-541`, `em_configuration.cpp:3847-3869`,
/// receiver `em_metrics.cpp:261-269`. The value is exactly 7 octets, and the STA MAC is always
/// present, even though a recipient ignores it unless the reason is 1 or 2. The reason is kept
/// raw: 0 and 7-255 are reserved, and upstream sends 0 with a zero MAC (audit 123).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCode {
    /// Reason code, raw.
    pub reason: u8,
    /// The MAC of the STA the reason refers to.
    pub sta_mac: MacAddr,
}

impl TlvBody<'_> for ErrorCode {
    const TLV_TYPE: TlvType = TlvType::ERROR_CODE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let reason = reader.u8()?;
        let sta_mac = reader.mac()?;
        reader.finish()?;
        Ok(Self { reason, sta_mac })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.u8(self.reason);
        writer.mac(self.sta_mac);
        Ok(writer.into_bytes())
    }
}

/// A BSS and its association allowance, in Association Status Notification,
/// `EasyMesh` §17.2.53 Table 75.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssociationStatusEntry {
    /// BSSID of a BSS operated by the Multi-AP device.
    pub bssid: MacAddr,
    /// Association allowance, raw: 0 no more associations allowed, 1 allowed, 2-255 reserved.
    pub status: u8,
}

/// Association Status Notification, `EasyMesh` §17.2.53 Table 75; audit rows 186–187.
/// Upstream has no sender or receiver (`em_msg.cpp:1125-1128`, test generator
/// `validation_test.cpp:876-884`), and its `em_assoc_sts_notif_t` has the wrong layout
/// (audit 187). The count may be 0, which the spec does not forbid.
#[derive(Debug, PartialEq, Eq)]
pub struct AssociationStatusNotification {
    /// The BSSs and their statuses, in wire order.
    pub bsses: Vec<AssociationStatusEntry>,
}

impl TlvBody<'_> for AssociationStatusNotification {
    const TLV_TYPE: TlvType = TlvType::ASSOCIATION_STATUS_NOTIFICATION;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let count = usize::from(reader.u8()?);
        let mut bsses = bounded_vec(count, reader.remaining(), 7);
        for _ in 0..count {
            bsses.push(AssociationStatusEntry {
                bssid: reader.mac()?,
                status: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self { bsses })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.count_u8(self.bsses.len())?;
        for bss in &self.bsses {
            writer.mac(bss.bssid);
            writer.u8(bss.status);
        }
        Ok(writer.into_bytes())
    }
}

/// A target BSS of a Profile-2 Steering Request, `EasyMesh` §17.2.57 Table 79.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile2SteeringTarget {
    /// Target BSSID or AP MLD MAC address; `FF:FF:FF:FF:FF:FF` is the wildcard.
    pub bssid: MacAddr,
    /// Target BSS operating class. A receiver ignores it for the wildcard BSSID.
    pub operating_class: u8,
    /// Target BSS channel number. A receiver ignores it for the wildcard BSSID.
    pub channel: u8,
    /// Steering reason code (Table 18 of the Wi-Fi Agile Multiband specification), raw.
    pub reason_code: u8,
}

/// Profile-2 Steering Request, `EasyMesh` §17.2.57 Table 79; audit rows 191–192.
/// Upstream `em_profile2_steering_req_t` (`em_base.h:1212-1215`) has no builder and no parser; it
/// is used only by a test generator (`validation_test.cpp:1366-1373`). The layout is that of
/// [`SteeringRequest`] with one more octet in each target, and without the 0x9B byte-order quirk:
/// the window and the timer are both big-endian (§3.1.2; audit 191 is `SPEC`, and no upstream
/// sender exists, so note 0009 D6 does not apply).
///
/// A non-zero target count in opportunity mode is [`DecodeError::ValueOutOfRange`] (note 0009,
/// A8), and there is no lenient decoder. Reserved bits 3-0 of the mode octet are ignored on
/// decode and written as zero.
#[derive(Debug, PartialEq, Eq)]
pub struct Profile2SteeringRequest {
    /// BSSID of the source BSS the request applies to.
    pub bssid: MacAddr,
    /// Opportunity or mandate, with the targets of a mandate.
    pub mode: SteeringMode<Profile2SteeringTarget>,
    /// The BTM Request flags.
    pub flags: SteeringFlags,
    /// `Steering_Opportunity_Window` in seconds, big-endian on the wire.
    pub opportunity_window: u16,
    /// `BTM_Disassociation_Timer` in TUs, big-endian on the wire.
    pub disassociation_timer: u16,
    /// The Agile Multiband capable STAs the request applies to; none means all of them.
    pub stas: Vec<MacAddr>,
}

impl TlvBody<'_> for Profile2SteeringRequest {
    const TLV_TYPE: TlvType = TlvType::PROFILE_2_STEERING_REQUEST;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let bssid = reader.mac()?;
        let mode_octet = reader.u8()?;
        let opportunity_window = reader.be_u16()?;
        let disassociation_timer = reader.be_u16()?;
        let sta_count = usize::from(reader.u8()?);
        let mut stas = bounded_vec(sta_count, reader.remaining(), 6);
        for _ in 0..sta_count {
            stas.push(reader.mac()?);
        }
        let target_count = reader.u8()?;
        if mode_octet & REQUEST_MODE_MANDATE == 0 && target_count != 0 {
            return Err(DecodeError::ValueOutOfRange {
                value: u32::from(target_count),
            });
        }
        let target_count = usize::from(target_count);
        let mut targets = bounded_vec(target_count, reader.remaining(), 9);
        for _ in 0..target_count {
            targets.push(Profile2SteeringTarget {
                bssid: reader.mac()?,
                operating_class: reader.u8()?,
                channel: reader.u8()?,
                reason_code: reader.u8()?,
            });
        }
        reader.finish()?;
        Ok(Self {
            bssid,
            mode: SteeringMode::from_octet(mode_octet, targets),
            flags: SteeringFlags::from_octet(mode_octet),
            opportunity_window,
            disassociation_timer,
            stas,
        })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.mac(self.bssid);
        writer.u8(self.mode.mode_bit() | self.flags.to_bits());
        writer.be_u16(self.opportunity_window);
        writer.be_u16(self.disassociation_timer);
        writer.count_u8(self.stas.len())?;
        for sta in &self.stas {
            writer.mac(*sta);
        }
        let targets = self.mode.targets();
        writer.count_u8(targets.len())?;
        for target in targets {
            writer.mac(target.bssid);
            writer.u8(target.operating_class);
            writer.u8(target.channel);
            writer.u8(target.reason_code);
        }
        Ok(writer.into_bytes())
    }
}

/// Status Code, `EasyMesh` §17.2.63 Table 85; audit rows 204–205.
/// Upstream `em_status_code_t` (`em_base.h:1671-1673`), sender
/// `em_configuration.cpp:626-637` (`htons`), receiver `em_ctrl.cpp:678-683` (`ntohs`). The value is
/// exactly 2 octets, big-endian, and is kept raw (IEEE 802.11 Table 9-50). The spec table labels
/// the length row `tlvValue`; that is the length (audit 204).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode {
    /// The status code, raw.
    pub code: u16,
}

impl TlvBody<'_> for StatusCode {
    const TLV_TYPE: TlvType = TlvType::STATUS_CODE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let code = reader.be_u16()?;
        reader.finish()?;
        Ok(Self { code })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.be_u16(self.code);
        Ok(writer.into_bytes())
    }
}

/// Reason Code, `EasyMesh` §17.2.64 Table 86; audit row 206.
/// Upstream `em_reason_code_t` (`em_base.h:1675-1677`), senders `em_configuration.cpp:519-528`
/// and `:639-652` (`htons`), receiver `em_ctrl.cpp:684-692` (`ntohs`). The value is exactly 2
/// octets, big-endian, and is kept raw (IEEE 802.11 Table 9-49).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasonCode {
    /// The reason code, raw.
    pub code: u16,
}

impl TlvBody<'_> for ReasonCode {
    const TLV_TYPE: TlvType = TlvType::REASON_CODE;

    fn decode_value(value: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(value);
        let code = reader.be_u16()?;
        reader.finish()?;
        Ok(Self { code })
    }

    fn encode_value(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new();
        writer.be_u16(self.code);
        Ok(writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_fixtures::{self, Key, Row};
    use crate::body_testkit::{Field, sweep_row, sweep_row_lenient, truncated_in};
    use core::mem::size_of;

    const GROUP: &str = "steering";
    const BSSID: MacAddr = MacAddr::new([0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4]);
    const STA1: MacAddr = MacAddr::new([0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51]);
    const STA2: MacAddr = MacAddr::new([0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x21]);
    const TARGET1: MacAddr = MacAddr::new([0x0e, 0x1f, 0x2a, 0x3b, 0x4c, 0x5d]);
    const TARGET2: MacAddr = MacAddr::new([0x5a, 0x4b, 0x3c, 0x2d, 0x1e, 0x0f]);
    const TOO_MANY: EncodeError = EncodeError::CountTooLarge { len: 256, max: 255 };
    /// Invalid rows that a lenient decoder accepts; the only lenient form is a kind=lenient row.
    const LENIENT_INPUTS: &[Key] = &[];
    const CANONICAL: &[(Key, Key)] = &[
        (
            ("0x92", "reserved-bits-set"),
            ("0x92", "reserved-bits-clear"),
        ),
        (("0x92", "reserved-bits-set-left"), ("0x92", "left")),
        (
            ("0x9b", "reserved-bits-set"),
            ("0x9b", "reserved-bits-clear"),
        ),
        (
            ("0xc3", "reserved-bits-set"),
            ("0xc3", "reserved-bits-clear"),
        ),
    ];

    fn fixture(key: Key) -> Vec<u8> {
        body_fixtures::value(GROUP, key)
    }

    fn dispatch_row(row: &Row<'_>, bytes: &[u8], canonical: Option<&[u8]>) -> Result<(), String> {
        match row.tlv {
            "0x92" => sweep_row::<ClientAssociationEvent>(row, bytes, canonical),
            "0x9b" => sweep_row_lenient::<SteeringRequest>(row, bytes, canonical, LENIENT_INPUTS),
            "0x9c" => sweep_row::<SteeringBtmReport>(row, bytes, canonical),
            "0x9d" => sweep_row::<ClientAssociationControlRequest>(row, bytes, canonical),
            "0x9e" => sweep_row::<BackhaulSteeringRequest>(row, bytes, canonical),
            "0x9f" => sweep_row::<BackhaulSteeringResponse>(row, bytes, canonical),
            "0xa0" => sweep_row::<HigherLayerData<'_>>(row, bytes, canonical),
            "0xa3" => sweep_row::<ErrorCode>(row, bytes, canonical),
            "0xbf" => sweep_row::<AssociationStatusNotification>(row, bytes, canonical),
            "0xc3" => sweep_row::<Profile2SteeringRequest>(row, bytes, canonical),
            "0xc9" => sweep_row::<StatusCode>(row, bytes, canonical),
            "0xca" => sweep_row::<ReasonCode>(row, bytes, canonical),
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

    fn event() -> ClientAssociationEvent {
        ClientAssociationEvent {
            sta_mac: STA1,
            ap_mac: BSSID,
            joined: true,
        }
    }

    fn two_targets() -> Vec<SteeringTarget> {
        vec![
            SteeringTarget {
                bssid: TARGET1,
                operating_class: 115,
                channel: 36,
            },
            SteeringTarget {
                bssid: TARGET2,
                operating_class: 81,
                channel: 6,
            },
        ]
    }

    fn steering_request() -> SteeringRequest {
        SteeringRequest {
            bssid: BSSID,
            mode: SteeringMode::Mandate {
                targets: two_targets(),
            },
            flags: SteeringFlags {
                disassociation_imminent: true,
                abridged: false,
                link_removal_imminent: true,
            },
            opportunity_window: 0x012c,
            disassociation_timer: 0x00c8,
            stas: vec![STA1, STA2],
        }
    }

    fn profile2_request() -> Profile2SteeringRequest {
        Profile2SteeringRequest {
            bssid: BSSID,
            mode: SteeringMode::Mandate {
                targets: vec![
                    Profile2SteeringTarget {
                        bssid: TARGET1,
                        operating_class: 115,
                        channel: 36,
                        reason_code: 5,
                    },
                    Profile2SteeringTarget {
                        bssid: TARGET2,
                        operating_class: 81,
                        channel: 6,
                        reason_code: 19,
                    },
                ],
            },
            flags: SteeringFlags {
                disassociation_imminent: true,
                abridged: false,
                link_removal_imminent: true,
            },
            opportunity_window: 0x012c,
            disassociation_timer: 0x00c8,
            stas: vec![STA1, STA2],
        }
    }

    fn btm_report() -> SteeringBtmReport {
        SteeringBtmReport {
            bssid: BSSID,
            sta_mac: STA1,
            status_code: 0,
            target_bssid: Some(TARGET1),
        }
    }

    fn association_control() -> ClientAssociationControlRequest {
        ClientAssociationControlRequest {
            bssid: BSSID,
            control: 0,
            validity_period: 0x0e10,
            stas: vec![STA1, STA2],
        }
    }

    fn backhaul_request() -> BackhaulSteeringRequest {
        BackhaulSteeringRequest {
            mac: STA1,
            target_bssid: TARGET1,
            operating_class: 115,
            channel: 36,
        }
    }

    fn backhaul_response() -> BackhaulSteeringResponse {
        BackhaulSteeringResponse {
            mac: STA1,
            target_bssid: TARGET1,
            result: 0,
        }
    }

    fn higher_layer() -> HigherLayerData<'static> {
        HigherLayerData {
            protocol: 1,
            payload: &[0x01, 0x02, 0x03, 0x04, 0x05, 0xa0, 0xb1, 0xc2],
        }
    }

    fn error_code() -> ErrorCode {
        ErrorCode {
            reason: 1,
            sta_mac: STA1,
        }
    }

    fn association_status() -> AssociationStatusNotification {
        AssociationStatusNotification {
            bsses: vec![
                AssociationStatusEntry {
                    bssid: BSSID,
                    status: 1,
                },
                AssociationStatusEntry {
                    bssid: TARGET1,
                    status: 0,
                },
            ],
        }
    }

    /// A fixture with its octet at `offset` replaced.
    fn with_octet(key: Key, offset: usize, octet: u8) -> Vec<u8> {
        let mut bytes = fixture(key);
        bytes[offset] = octet;
        bytes
    }

    /// The Steering Request (0x9B or 0xC3) prefix fields up to and including the STA list count,
    /// then two STAs.
    const REQUEST_HEAD_FIELDS: [Field; 7] = [
        field(0, 6),
        field(6, 1),
        field(7, 2),
        field(9, 2),
        field(11, 1),
        field(12, 6),
        field(18, 6),
    ];

    fn steering_request_prefix_error(len: usize) -> crate::error::DecodeError {
        let mut fields = REQUEST_HEAD_FIELDS.to_vec();
        fields.extend([
            field(24, 1),
            field(25, 6),
            field(31, 1),
            field(32, 1),
            field(33, 6),
            field(39, 1),
            field(40, 1),
        ]);
        truncated_in(&fields)(len)
    }

    fn profile2_prefix_error(len: usize) -> crate::error::DecodeError {
        let mut fields = REQUEST_HEAD_FIELDS.to_vec();
        fields.extend([
            field(24, 1),
            field(25, 6),
            field(31, 1),
            field(32, 1),
            field(33, 1),
            field(34, 6),
            field(40, 1),
            field(41, 1),
            field(42, 1),
        ]);
        truncated_in(&fields)(len)
    }

    /// The 19-octet golden report is also complete at 13 octets, without a target.
    fn btm_report_prefix_error(len: usize) -> crate::error::DecodeError {
        if len > 13 {
            crate::error::DecodeError::Truncated {
                needed: 6,
                remaining: len - 13,
            }
        } else {
            truncated_in(&[field(0, 6), field(6, 6), field(12, 1)])(len)
        }
    }

    fn opportunity_with_target() -> Vec<u8> {
        fixture(("0x9b", "opportunity-with-target"))
    }

    /// What the lenient 0x9B decoder's body encodes to: the fixture without its target entry,
    /// and with a target count of zero.
    fn opportunity_with_target_reencoded() -> Vec<u8> {
        let mut bytes = opportunity_with_target();
        bytes.truncate(18);
        bytes.push(0);
        bytes
    }

    fn opportunity_without_targets() -> SteeringRequest {
        SteeringRequest {
            bssid: BSSID,
            mode: SteeringMode::Opportunity,
            flags: SteeringFlags::default(),
            opportunity_window: 0x003c,
            disassociation_timer: 0x0001,
            stas: vec![STA1],
        }
    }

    fn too_many_stas_request() -> SteeringRequest {
        let mut body = steering_request();
        body.stas.resize(256, STA1);
        body
    }

    fn too_many_targets_request() -> SteeringRequest {
        let mut body = steering_request();
        body.mode = SteeringMode::Mandate {
            targets: vec![two_targets()[0]; 256],
        };
        body
    }

    fn too_many_stas_profile2() -> Profile2SteeringRequest {
        let mut body = profile2_request();
        body.stas.resize(256, STA1);
        body
    }

    fn too_many_targets_profile2() -> Profile2SteeringRequest {
        let mut body = profile2_request();
        let target = Profile2SteeringTarget {
            bssid: TARGET1,
            operating_class: 115,
            channel: 36,
            reason_code: 5,
        };
        body.mode = SteeringMode::Mandate {
            targets: vec![target; 256],
        };
        body
    }

    fn many_stas_control(count: usize) -> ClientAssociationControlRequest {
        let mut body = association_control();
        body.stas.resize(count, STA1);
        body
    }

    body_checks! {
        mod event_checks;
        body: ClientAssociationEvent;
        golden: &fixture(("0x92", "golden")), event();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 6), field(12, 1)]);
        trailing_byte: &fixture(("0x92", "golden"));
        reserved_bits: [
            (&fixture(("0x92", "reserved-bits-set")), event(), &fixture(("0x92", "reserved-bits-clear"))),
            (&fixture(("0x92", "reserved-bits-set-left")), ClientAssociationEvent { joined: false, ..event() }, &fixture(("0x92", "left")))
        ];
    }
    body_checks! {
        mod steering_request_checks;
        body: SteeringRequest;
        golden: &fixture(("0x9b", "golden")), steering_request();
        lenient;
        prefixes: |_| false, steering_request_prefix_error;
        trailing_byte: &fixture(("0x9b", "golden"));
        count_too_large: too_many_stas_request(), TOO_MANY;
        reserved_bits: [
            (&fixture(("0x9b", "reserved-bits-set")), SteeringRequest::decode_value(&fixture(("0x9b", "reserved-bits-clear"))).unwrap(), &fixture(("0x9b", "reserved-bits-clear"))),
            (&fixture(("0x9b", "reserved-bits-set-zero-flags")), SteeringRequest::decode_value(&with_octet(("0x9b", "reserved-bits-set-zero-flags"), 6, 0)).unwrap(), &with_octet(("0x9b", "reserved-bits-set-zero-flags"), 6, 0))
        ];
        lenient: [(&opportunity_with_target(), opportunity_without_targets(), &opportunity_with_target_reencoded())];
        lenient_rejects: [
            (&opportunity_with_target()[..22], DecodeError::Truncated { needed: 6, remaining: 3 }),
            (&[opportunity_with_target(), vec![0xa5]].concat(), DecodeError::TrailingBytes { remaining: 1 })
        ];
    }
    body_checks! {
        mod btm_report_checks;
        body: SteeringBtmReport;
        golden: &fixture(("0x9c", "golden")), btm_report();
        prefixes: |len| len == 13, btm_report_prefix_error;
        trailing_byte: &fixture(("0x9c", "golden"));
        alternates: [(&fixture(("0x9c", "no-target")), SteeringBtmReport { status_code: 6, target_bssid: None, ..btm_report() }, &fixture(("0x9c", "no-target")))];
    }
    body_checks! {
        mod association_control_checks;
        body: ClientAssociationControlRequest;
        golden: &fixture(("0x9d", "golden")), association_control();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 1), field(7, 2), field(9, 1), field(10, 6), field(16, 6)]);
        trailing_byte: &fixture(("0x9d", "golden"));
        count_too_large: many_stas_control(256), TOO_MANY;
    }
    body_checks! {
        mod backhaul_request_checks;
        body: BackhaulSteeringRequest;
        golden: &fixture(("0x9e", "golden")), backhaul_request();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 6), field(12, 1), field(13, 1)]);
        trailing_byte: &fixture(("0x9e", "golden"));
    }
    body_checks! {
        mod backhaul_response_checks;
        body: BackhaulSteeringResponse;
        golden: &fixture(("0x9f", "golden")), backhaul_response();
        prefixes: |_| false, truncated_in(&[field(0, 6), field(6, 6), field(12, 1)]);
        trailing_byte: &fixture(("0x9f", "golden"));
    }
    body_checks! {
        mod higher_layer_checks;
        body: HigherLayerData<'a>;
        golden: &fixture(("0xa0", "golden")), higher_layer();
        // Every octet after the protocol is payload, so every non-empty prefix is complete.
        prefixes: |len| len >= 1, truncated_in(&[field(0, 1)]);
        alternates: [(&fixture(("0xa0", "empty-payload")), HigherLayerData { protocol: 1, payload: &[] }, &fixture(("0xa0", "empty-payload")))];
    }
    body_checks! {
        mod error_code_checks;
        body: ErrorCode;
        golden: &fixture(("0xa3", "golden")), error_code();
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6)]);
        trailing_byte: &fixture(("0xa3", "golden"));
    }
    body_checks! {
        mod association_status_checks;
        body: AssociationStatusNotification;
        golden: &fixture(("0xbf", "golden")), association_status();
        prefixes: |_| false, truncated_in(&[field(0, 1), field(1, 6), field(7, 1), field(8, 6), field(14, 1)]);
        trailing_byte: &fixture(("0xbf", "golden"));
        count_too_large: AssociationStatusNotification { bsses: vec![AssociationStatusEntry { bssid: BSSID, status: 0 }; 256] }, TOO_MANY;
        alternates: [(&fixture(("0xbf", "zero-bssids")), AssociationStatusNotification { bsses: vec![] }, &fixture(("0xbf", "zero-bssids")))];
    }
    body_checks! {
        mod profile2_request_checks;
        body: Profile2SteeringRequest;
        golden: &fixture(("0xc3", "golden")), profile2_request();
        prefixes: |_| false, profile2_prefix_error;
        trailing_byte: &fixture(("0xc3", "golden"));
        count_too_large: too_many_stas_profile2(), TOO_MANY;
        reserved_bits: [
            (&fixture(("0xc3", "reserved-bits-set")), Profile2SteeringRequest::decode_value(&fixture(("0xc3", "reserved-bits-clear"))).unwrap(), &fixture(("0xc3", "reserved-bits-clear"))),
            (&fixture(("0xc3", "reserved-bits-set-zero-flags")), Profile2SteeringRequest::decode_value(&with_octet(("0xc3", "reserved-bits-set-zero-flags"), 6, 0)).unwrap(), &with_octet(("0xc3", "reserved-bits-set-zero-flags"), 6, 0))
        ];
    }
    body_checks! {
        mod status_code_checks;
        body: StatusCode;
        golden: &fixture(("0xc9", "golden")), StatusCode { code: 0x0011 };
        prefixes: |_| false, truncated_in(&[field(0, 2)]);
        trailing_byte: &fixture(("0xc9", "golden"));
    }
    body_checks! {
        mod reason_code_checks;
        body: ReasonCode;
        golden: &fixture(("0xca", "golden")), ReasonCode { code: 0x0011 };
        prefixes: |_| false, truncated_in(&[field(0, 2)]);
        trailing_byte: &fixture(("0xca", "golden"));
    }

    #[test]
    fn target_lists_over_255_do_not_encode() {
        assert_eq!(too_many_targets_request().encode_value(), Err(TOO_MANY));
        assert_eq!(too_many_targets_profile2().encode_value(), Err(TOO_MANY));
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

    #[test]
    fn association_type_uses_bit_7_alone() {
        let flags: &[Flag<ClientAssociationEvent>] = &[(12, 0x80, |b| b.joined = true)];
        assert_each_flag_alone(
            &[
                0x3c, 0x1d, 0x2e, 0x3f, 0x4a, 0x51, 0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0,
            ],
            || ClientAssociationEvent {
                joined: false,
                ..event()
            },
            flags,
        );
    }

    #[test]
    fn steering_request_flags_each_use_their_own_bit() {
        // Mode octet: bit 7 Request Mode, 6 disassociation imminent, 5 abridged, 4 link removal.
        let flags: &[Flag<SteeringRequest>] = &[
            (6, 0x80, |b| {
                b.mode = SteeringMode::Mandate {
                    targets: Vec::new(),
                };
            }),
            (6, 0x40, |b| b.flags.disassociation_imminent = true),
            (6, 0x20, |b| b.flags.abridged = true),
            (6, 0x10, |b| b.flags.link_removal_imminent = true),
        ];
        assert_each_flag_alone(
            &[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0, 0, 0, 0, 0, 0, 0],
            || SteeringRequest {
                bssid: BSSID,
                mode: SteeringMode::Opportunity,
                flags: SteeringFlags::default(),
                opportunity_window: 0,
                disassociation_timer: 0,
                stas: Vec::new(),
            },
            flags,
        );
    }

    #[test]
    fn profile2_steering_request_flags_each_use_their_own_bit() {
        let flags: &[Flag<Profile2SteeringRequest>] = &[
            (6, 0x80, |b| {
                b.mode = SteeringMode::Mandate {
                    targets: Vec::new(),
                };
            }),
            (6, 0x40, |b| b.flags.disassociation_imminent = true),
            (6, 0x20, |b| b.flags.abridged = true),
            (6, 0x10, |b| b.flags.link_removal_imminent = true),
        ];
        assert_each_flag_alone(
            &[0x02, 0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0, 0, 0, 0, 0, 0, 0],
            || Profile2SteeringRequest {
                bssid: BSSID,
                mode: SteeringMode::Opportunity,
                flags: SteeringFlags::default(),
                opportunity_window: 0,
                disassociation_timer: 0,
                stas: Vec::new(),
            },
            flags,
        );
    }

    #[test]
    fn the_two_steering_requests_write_the_window_in_different_byte_orders() {
        // Steering Request: window little-endian (audit 12, note 0009 D6), timer big-endian.
        let mut request = steering_request();
        request.opportunity_window = 0x0102;
        request.disassociation_timer = 0x0304;
        let bytes = request.encode_value().unwrap();
        assert_eq!(bytes[7..9], [0x02, 0x01]);
        assert_eq!(bytes[9..11], [0x03, 0x04]);
        // Profile-2 Steering Request: both big-endian (audit 191).
        let mut request = profile2_request();
        request.opportunity_window = 0x0102;
        request.disassociation_timer = 0x0304;
        let bytes = request.encode_value().unwrap();
        assert_eq!(bytes[7..9], [0x01, 0x02]);
        assert_eq!(bytes[9..11], [0x03, 0x04]);
    }

    #[test]
    fn opportunity_with_a_target_is_value_out_of_range_for_both_requests() {
        // intentional-difference: upstream em_steering.cpp:490 always sends one target, also in
        // opportunity mode (audit row 13); the strict decoders refuse it (the spec count is zero).
        // Note 0009 A8. The count is reported before the target entries are read, so a cut
        // entry gives the same error.
        let steering = opportunity_with_target();
        let profile2 = fixture(("0xc3", "opportunity-with-target"));
        assert_eq!(
            SteeringRequest::decode_value(&steering),
            Err(DecodeError::ValueOutOfRange { value: 1 })
        );
        assert_eq!(
            SteeringRequest::decode_value(&steering[..22]),
            Err(DecodeError::ValueOutOfRange { value: 1 })
        );
        assert_eq!(
            Profile2SteeringRequest::decode_value(&profile2),
            Err(DecodeError::ValueOutOfRange { value: 1 })
        );
    }

    #[test]
    fn lenient_steering_request_drops_the_targets_of_an_opportunity() {
        // Upstream always sends one target, also for an opportunity (audit 13).
        let bytes = opportunity_with_target();
        assert_eq!(
            SteeringRequest::decode_value_lenient(&bytes),
            Ok(Some(opportunity_without_targets()))
        );
        // A mandate keeps its targets when decoded leniently.
        let golden = fixture(("0x9b", "golden"));
        assert_eq!(
            SteeringRequest::decode_value_lenient(&golden),
            Ok(Some(steering_request()))
        );
    }

    #[test]
    fn opportunity_cannot_hold_targets_and_a_mandate_may_hold_none() {
        // The spec sets the target count to zero for an opportunity, so the type has no target
        // list there.
        let mandate = SteeringRequest {
            mode: SteeringMode::Mandate {
                targets: Vec::new(),
            },
            ..opportunity_without_targets()
        };
        let bytes = mandate.encode_value().unwrap();
        assert_eq!(SteeringRequest::decode_value(&bytes), Ok(mandate));
        assert_eq!(bytes[6], 0x80);
    }

    #[test]
    fn btm_report_target_of_one_to_five_octets_is_truncated() {
        for extra in 1..=5 {
            let mut bytes = fixture(("0x9c", "upstream-form"));
            bytes.extend(vec![0x0e; extra]);
            assert_eq!(
                SteeringBtmReport::decode_value(&bytes),
                Err(DecodeError::Truncated {
                    needed: 6,
                    remaining: extra
                })
            );
        }
        let mut seven = fixture(("0x9c", "golden"));
        seven.push(0xa5);
        assert_eq!(
            SteeringBtmReport::decode_value(&seven),
            Err(DecodeError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn btm_report_without_a_target_has_no_lenient_form() {
        // The 13-octet upstream form is the spec form (audit 108), so the body is strict only.
        let bytes = fixture(("0x9c", "upstream-form"));
        assert_eq!(bytes.len(), 13);
        let body = SteeringBtmReport::decode_value(&bytes).unwrap();
        assert_eq!(body.target_bssid, None);
        assert_eq!(body.encode_value(), Ok(bytes));
    }

    #[test]
    fn association_control_with_no_stas_is_invalid_and_does_not_encode() {
        assert_eq!(
            ClientAssociationControlRequest::decode_value(&fixture(("0x9d", "zero-stas"))),
            Err(DecodeError::InvalidLength { len: 0 })
        );
        assert_eq!(
            many_stas_control(0).encode_value(),
            Err(EncodeError::CountTooSmall { len: 0, min: 1 })
        );
    }

    #[test]
    fn association_control_accepts_more_than_twenty_stas() {
        // intentional-difference: upstream em_steering.cpp:658 (MAX_STA_LIST, em_base.h:122)
        // refuses more than 20 STAs in a Client Association Control Request (audit row 113).
        // Upstream refuses more than 20 (audit 113, MAX_STA_LIST); the spec allows 255.
        for count in [21, 255] {
            let body = many_stas_control(count);
            let bytes = body.encode_value().unwrap();
            assert_eq!(bytes.len(), 10 + 6 * count);
            assert_eq!(
                ClientAssociationControlRequest::decode_value(&bytes),
                Ok(body)
            );
        }
    }

    #[test]
    fn reserved_codes_are_kept_raw() {
        // intentional-difference: upstream em_steering.cpp:667-699 refuses every association
        // control code but 0 and 1 (audit 112); this body keeps the code raw (note 0009 D2).
        let control = fixture(("0x9d", "control-reserved-ff"));
        let body = ClientAssociationControlRequest::decode_value(&control).unwrap();
        assert_eq!(body.control, 0xff);
        assert_eq!(body.encode_value(), Ok(control));
        let result = fixture(("0x9f", "result-reserved-ff"));
        let body = BackhaulSteeringResponse::decode_value(&result).unwrap();
        assert_eq!(body.result, 0xff);
        assert_eq!(body.encode_value(), Ok(result));
        let reason = fixture(("0xa3", "reason-reserved-ff"));
        let body = ErrorCode::decode_value(&reason).unwrap();
        assert_eq!(body.reason, 0xff);
        assert_eq!(body.encode_value(), Ok(reason));
        let status = fixture(("0xbf", "status-reserved"));
        let body = AssociationStatusNotification::decode_value(&status).unwrap();
        assert_eq!(body.bsses[0].status, 2);
        assert_eq!(body.bsses[1].status, 0xff);
        assert_eq!(body.encode_value(), Ok(status));
    }

    #[test]
    fn status_and_reason_codes_are_big_endian() {
        assert_eq!(StatusCode { code: 0x0102 }.encode_value(), Ok(vec![1, 2]));
        assert_eq!(ReasonCode { code: 0x0102 }.encode_value(), Ok(vec![1, 2]));
    }

    #[test]
    fn association_control_validity_period_is_big_endian() {
        let mut body = association_control();
        body.validity_period = 0x0102;
        assert_eq!(body.encode_value().unwrap()[7..9], [0x01, 0x02]);
    }

    #[test]
    fn higher_layer_payload_runs_to_the_end_of_the_value() {
        let body = HigherLayerData::decode_value(&[1]).unwrap();
        assert_eq!(body.payload, &[] as &[u8]);
        let body = HigherLayerData::decode_value(&[0xff, 1, 2, 3]).unwrap();
        assert_eq!((body.protocol, body.payload), (0xff, &[1, 2, 3][..]));
    }

    #[test]
    fn bodies_and_records_have_bounded_stack_sizes() {
        let word = size_of::<usize>();
        assert!(size_of::<ClientAssociationEvent>() <= 13);
        assert!(size_of::<SteeringFlags>() <= 3);
        assert!(size_of::<SteeringTarget>() <= 8);
        assert!(size_of::<Profile2SteeringTarget>() <= 9);
        assert!(size_of::<SteeringRequest>() <= 9 * word);
        assert!(size_of::<Profile2SteeringRequest>() <= 9 * word);
        assert!(size_of::<SteeringBtmReport>() <= 20);
        assert!(size_of::<ClientAssociationControlRequest>() <= 5 * word);
        assert!(size_of::<BackhaulSteeringRequest>() <= 14);
        assert!(size_of::<BackhaulSteeringResponse>() <= 13);
        assert!(size_of::<HigherLayerData<'_>>() <= 3 * word);
        assert!(size_of::<ErrorCode>() <= 7);
        assert!(size_of::<AssociationStatusEntry>() <= 7);
        assert!(size_of::<AssociationStatusNotification>() <= 3 * word);
        assert!(size_of::<StatusCode>() <= 2);
        assert!(size_of::<ReasonCode>() <= 2);
    }
}
