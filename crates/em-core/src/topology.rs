// SPDX-License-Identifier: Apache-2.0

//! Synchronous S2 Topology Query and Response messages (note 0013 D4 and D7).

use std::fmt;

use em_msg::bodies::{
    AgentApMldConfiguration, ApOperationalBss, BssConfigurationReport, BssConfigurationReportBss,
    BssConfigurationReportRadio, DeviceInformation, LocalInterface, MultiApProfile, OperationalBss,
    OperationalRadio, ServiceList, SupportedService, TidToLinkMappingPolicy, TidToLinkScope,
    VendorBss, VendorOperationalBss, VendorRadio,
};
use em_msg::{AL_SAP_BUDGET, CmduHeader, MsgBuilder, TlvBody, TlvIter, validate};
use em_types::{FreqBand, MacAddr, MediaType, MsgType, ProfileType, ServiceType, TlvType};

use crate::{AgentState, MsgIdCounter};

/// Topology parsing or construction failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The CMDU, a required TLV, or a TLV body is malformed.
    InvalidMessage,
    /// A reported SSID is absent from the controller's `NetworkSSIDList`.
    SsidMismatch,
    /// Message encoding failed or exceeded the transport budget.
    Construction,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

fn header(msg_type: MsgType, msg_id: u16) -> CmduHeader {
    CmduHeader {
        msg_type,
        msg_id,
        frag_id: 0,
        last_fragment: true,
        relay: false,
    }
}
fn area(cmdu: &[u8], kind: MsgType) -> Result<(CmduHeader, &[u8]), Error> {
    if cmdu.len() > AL_SAP_BUDGET {
        return Err(Error::InvalidMessage);
    }
    let (head, rest) = CmduHeader::parse(cmdu).map_err(|_| Error::InvalidMessage)?;
    if head.msg_type != kind || head.frag_id != 0 || !head.last_fragment {
        return Err(Error::InvalidMessage);
    }
    Ok((head, rest))
}

/// A confirmed platform BSS used to construct a Topology Response.
#[derive(Debug, Clone, Copy)]
pub struct Bss<'a> {
    /// BSSID.
    pub bssid: MacAddr,
    /// SSID, without a terminator.
    pub ssid: &'a [u8],
    /// Whether this BSS is enabled (disabled BSSs are omitted from operational BSS).
    pub enabled: bool,
    /// Interface MAC (BSSID for an AP, station MAC for a STA).
    pub interface_mac: MacAddr,
    /// Radio frequency band, used for upstream's media-specific information.
    pub band: FreqBand,
    /// Comcast haul index for the vendor operational BSS.
    pub haul_type: u16,
    /// Upstream VAP mode for the vendor operational BSS.
    pub vap_mode: u16,
}

/// One radio and its confirmed BSSs, in platform order.
#[derive(Debug, Clone, Copy)]
pub struct Radio<'a> {
    /// Radio unique identifier.
    pub ruid: MacAddr,
    /// BSSs belonging to this radio.
    pub bsses: &'a [Bss<'a>],
}

/// Builds the controller Topology Query (`em_configuration.cpp:944-1004`).
///
/// # Errors
/// Returns an encoding or budget error.
pub fn build_query(ids: &mut MsgIdCounter) -> Result<Vec<u8>, Error> {
    let mut builder = MsgBuilder::new(
        &header(MsgType::TOPOLOGY_QUERY, ids.next_id()),
        AL_SAP_BUDGET,
    );
    builder.body(&MultiApProfile {
        profile: ProfileType::from(3),
    });
    builder.finish().map_err(|_| Error::Construction)
}

// em_configuration.cpp:6786-6825 uses a fixed 802.11ax media type and
// fills these ten media-specific bytes from the interface and band.
fn upstream_media(bss: &Bss<'_>) -> [u8; 10] {
    let mut info = [0_u8; 10];
    info[..6].copy_from_slice(&bss.interface_mac.octets());
    info[6] = if bss.vap_mode == 0 { 0 } else { 0x40 };
    info[7] = match bss.band {
        FreqBand::BAND_5_GHZ => 3,
        FreqBand::BAND_6_GHZ => 4,
        _ => 1,
    };
    info
}

/// Builds an agent Topology Response with the supplied BSS list.
///
/// The response retains the Query MID. The default empty MLD and TID policy
/// reflect the S2 fake platform's lack of MLD configuration.
///
/// # Errors
/// Returns an encoding error for invalid SSIDs, counts or message size.
pub fn build_response(
    al_mac: MacAddr,
    radios: &[Radio<'_>],
    msg_id: u16,
    is_emplus: bool,
) -> Result<Vec<u8>, Error> {
    let mut ssids = Vec::new();
    let mut media = Vec::new();
    for radio in radios {
        for bss in radio.bsses {
            if bss.ssid.len() > 32 || bss.ssid.contains(&0) {
                return Err(Error::Construction);
            }
            let mut ssid = bss.ssid.to_vec();
            ssid.push(0);
            ssids.push(ssid);
            media.push(upstream_media(bss));
        }
    }
    let mut index = 0;
    let mut op_radios = Vec::new();
    let mut report_radios = Vec::new();
    let mut vendor_radios = Vec::new();
    let mut interfaces = Vec::new();
    for radio in radios {
        let mut op_bsses = Vec::new();
        let mut report_bsses = Vec::new();
        let mut vendor_bsses = Vec::new();
        for bss in radio.bsses {
            let ssid = ssids[index].as_slice();
            let media_info = media[index].as_slice();
            index += 1;
            if bss.enabled {
                op_bsses.push(OperationalBss {
                    bssid: bss.bssid,
                    ssid,
                });
            }
            report_bsses.push(BssConfigurationReportBss {
                bssid: bss.bssid,
                ssid,
                backhaul_not_in_use: false,
                fronthaul_not_in_use: false,
                r1_disallowed: false,
                r2_disallowed: false,
                multiple_bssid: false,
                transmitted_bssid: false,
            });
            vendor_bsses.push(VendorBss {
                bssid: bss.bssid,
                haul_type: bss.haul_type,
                vap_mode: bss.vap_mode,
            });
            interfaces.push(LocalInterface {
                mac: bss.interface_mac,
                media_type: MediaType::from(0x0108),
                media_info,
            });
        }
        op_radios.push(OperationalRadio {
            ruid: radio.ruid,
            bsses: op_bsses,
        });
        report_radios.push(BssConfigurationReportRadio {
            ruid: radio.ruid,
            bsses: report_bsses,
        });
        vendor_radios.push(VendorRadio {
            ruid: radio.ruid,
            bsses: vendor_bsses,
        });
    }
    let mut services = vec![ServiceType::MULTI_AP_AGENT];
    if is_emplus {
        services.push(ServiceType::EMPLUS_AGENT);
    }
    let mut builder = MsgBuilder::new(&header(MsgType::TOPOLOGY_RESPONSE, msg_id), AL_SAP_BUDGET);
    builder
        .body(&DeviceInformation { al_mac, interfaces })
        .body(&SupportedService(ServiceList { services }))
        .body(&ApOperationalBss { radios: op_radios })
        .body(&MultiApProfile {
            profile: ProfileType::from(3),
        })
        .body(&BssConfigurationReport {
            radios: report_radios,
        })
        .body(&AgentApMldConfiguration {
            ap_mlds: Vec::new(),
        })
        .body(&TidToLinkMappingPolicy {
            mld_mac: MacAddr::ZERO,
            negotiation_enabled: false,
            scope: TidToLinkScope::ApMld {
                mappings: Vec::new(),
            },
        })
        .body(&VendorOperationalBss {
            radios: vendor_radios,
        });
    builder.finish().map_err(|_| Error::Construction)
}

/// Applies the device-wide Query gate and returns a response when every radio is ready.
///
/// `WscM2Processed` is the upstream exception to the normal state threshold
/// (`em_configuration.cpp:6572-6577`). State mutation is the role core's job.
///
/// # Errors
/// Returns an error for a malformed Query or when the Response cannot be built.
pub fn handle_query(
    cmdu: &[u8],
    states: &[AgentState],
    al_mac: MacAddr,
    radios: &[Radio<'_>],
    is_emplus: bool,
) -> Result<Option<Vec<u8>>, Error> {
    let (head, rest) = area(cmdu, MsgType::TOPOLOGY_QUERY)?;
    if states.len() != radios.len() {
        return Err(Error::InvalidMessage);
    }
    for item in TlvIter::new(rest) {
        item.map_err(|_| Error::InvalidMessage)?;
    }
    if states.iter().any(|state| {
        *state < AgentState::OneWifiBssconfigInd && *state != AgentState::WscM2Processed
    }) {
        return Ok(None);
    }
    build_response(al_mac, radios, head.msg_id, is_emplus).map(Some)
}

/// The controller's parsed view of a Topology Response, borrowing the CMDU.
#[derive(Debug)]
pub struct Response<'a> {
    /// Parsed device information.
    pub device: DeviceInformation<'a>,
    /// Parsed radios and BSSs.
    pub operational_bss: ApOperationalBss<'a>,
}

/// Parses a Topology Response, requiring AP Operational BSS and checking all SSIDs.
///
/// # Errors
/// Returns [`Error::SsidMismatch`] for a reported SSID absent from `known_ssids`;
/// returns [`Error::InvalidMessage`] for malformed or missing required data.
pub fn parse_response<'a>(
    cmdu: &'a [u8],
    known_ssids: &[&[u8]],
    profile: u8,
) -> Result<Response<'a>, Error> {
    let (_, rest) = area(cmdu, MsgType::TOPOLOGY_RESPONSE)?;
    validate(cmdu, ProfileType::from(profile)).map_err(|_| Error::InvalidMessage)?;
    let (mut device, mut bss) = (None, None);
    for item in TlvIter::new(rest) {
        let tlv = item.map_err(|_| Error::InvalidMessage)?;
        if tlv.tlv_type == TlvType::DEVICE_INFORMATION {
            if device.is_some() {
                return Err(Error::InvalidMessage);
            }
            device = Some(
                DeviceInformation::decode_value(tlv.value).map_err(|_| Error::InvalidMessage)?,
            );
        } else if tlv.tlv_type == TlvType::AP_OPERATIONAL_BSS {
            if bss.is_some() {
                return Err(Error::InvalidMessage);
            }
            bss =
                Some(ApOperationalBss::decode_value(tlv.value).map_err(|_| Error::InvalidMessage)?);
        }
    }
    let device = device.ok_or(Error::InvalidMessage)?;
    let operational_bss = bss.ok_or(Error::InvalidMessage)?;
    for radio in &operational_bss.radios {
        for bss in &radio.bsses {
            let ssid = bss.ssid.strip_suffix(&[0]).unwrap_or(bss.ssid);
            if ssid.len() > 32 || ssid.contains(&0) {
                return Err(Error::InvalidMessage);
            }
            if !known_ssids.contains(&ssid) {
                return Err(Error::SsidMismatch);
            }
        }
    }
    Ok(Response {
        device,
        operational_bss,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AL: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
    const RADIO1: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
    const RADIO2: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 3]);
    const BSSID1: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 4]);
    const BSSID2: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 5]);
    fn bss(bssid: MacAddr, ssid: &'static [u8]) -> Bss<'static> {
        Bss {
            bssid,
            ssid,
            enabled: true,
            interface_mac: bssid,
            band: FreqBand::BAND_5_GHZ,
            haul_type: 0,
            vap_mode: 0,
        }
    }

    #[test]
    fn device_gate_response_content_and_ssid_mismatch() {
        let mut ids = MsgIdCounter::new();
        let query = build_query(&mut ids).unwrap();
        let first = [bss(BSSID1, b"front")];
        let second = [bss(BSSID2, b"back")];
        let radios = [
            Radio {
                ruid: RADIO1,
                bsses: &first,
            },
            Radio {
                ruid: RADIO2,
                bsses: &second,
            },
        ];
        assert!(
            handle_query(
                &query,
                &[AgentState::OneWifiBssconfigInd, AgentState::OwconfigPending],
                AL,
                &radios,
                false
            )
            .unwrap()
            .is_none()
        );
        let response = handle_query(
            &query,
            &[
                AgentState::OneWifiBssconfigInd,
                AgentState::OneWifiBssconfigInd,
            ],
            AL,
            &radios,
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            CmduHeader::parse(&response).unwrap().0.msg_id,
            CmduHeader::parse(&query).unwrap().0.msg_id
        );
        let parsed = parse_response(&response, &[b"front", b"back"], 3).unwrap();
        assert_eq!(parsed.device.al_mac, AL);
        assert_eq!(parsed.device.interfaces.len(), 2);
        assert_eq!(
            parsed.device.interfaces[0].media_type,
            MediaType::from(0x0108)
        );
        assert_eq!(
            parsed.device.interfaces[0].media_info,
            &[2, 0, 0, 0, 0, 4, 0, 3, 0, 0]
        );
        assert_eq!(parsed.operational_bss.radios.len(), 2);
        assert_eq!(parsed.operational_bss.radios[0].ruid, RADIO1);
        assert_eq!(parsed.operational_bss.radios[1].bsses[0].ssid, b"back\0");
        assert_eq!(
            parse_response(&response, &[b"front"], 3).unwrap_err(),
            Error::SsidMismatch
        );
        let (_, area) = CmduHeader::parse(&response).unwrap();
        let kinds: Vec<_> = TlvIter::new(area)
            .map(Result::unwrap)
            .map(|t| t.tlv_type)
            .collect();
        assert_eq!(
            kinds,
            [
                TlvType::DEVICE_INFORMATION,
                TlvType::SUPPORTED_SERVICE,
                TlvType::AP_OPERATIONAL_BSS,
                TlvType::MULTI_AP_PROFILE,
                TlvType::BSS_CONFIGURATION_REPORT,
                TlvType::AGENT_AP_MLD_CONFIGURATION,
                TlvType::TID_TO_LINK_MAPPING_POLICY,
                TlvType::VENDOR_OPERATIONAL_BSS
            ]
        );
        assert!(
            handle_query(
                &query,
                &[AgentState::WscM2Processed, AgentState::WscM2Processed],
                AL,
                &radios,
                false
            )
            .unwrap()
            .is_some()
        );
    }

    #[test]
    fn missing_operational_bss_and_truncation_are_rejected() {
        let first = [bss(BSSID1, b"front")];
        let radios = [Radio {
            ruid: RADIO1,
            bsses: &first,
        }];
        let response = build_response(AL, &radios, 42, false).unwrap();
        for len in 0..response.len() {
            assert!(parse_response(&response[..len], &[b"front"], 3).is_err());
        }
        for byte in 0..=255_u8 {
            let frame = [byte; 53];
            assert!(parse_response(&frame, &[], 3).is_err());
            assert!(handle_query(&frame, &[], AL, &radios, false).is_err());
        }
        let (_, area) = CmduHeader::parse(&response).unwrap();
        let mut b = MsgBuilder::new(&header(MsgType::TOPOLOGY_RESPONSE, 42), AL_SAP_BUDGET);
        for item in TlvIter::new(area) {
            let tlv = item.unwrap();
            if tlv.tlv_type != TlvType::AP_OPERATIONAL_BSS {
                b.tlv(tlv.tlv_type, tlv.value);
            }
        }
        assert_eq!(
            parse_response(&b.finish().unwrap(), &[b"front"], 3).unwrap_err(),
            Error::InvalidMessage
        );
    }
}
