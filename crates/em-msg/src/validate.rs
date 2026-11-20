// SPDX-License-Identifier: Apache-2.0

//! Message validation (upstream `em_msg_t::validate`, `em_msg.cpp:643-708`).
//!
//! [`validate`] takes a CMDU starting at its header (no Ethernet header). It
//! checks the framing, then the table of the message type, and keeps no state
//! between calls.
//!
//! What it accepts follows upstream (note 0005 D3):
//!
//! - TLVs that are not in the table are accepted.
//! - Only mandatory rows are length-checked, and only on the first TLV of
//!   their type. The check is `3 + value.len() < min_len`, where `min_len` is
//!   upstream's minimum or, for the 26 rows in [`crate::spec_min`], the spec's.
//! - Optional rows are never a violation, whatever their length.
//! - A message type with an empty table, or with no table, gets framing
//!   checks only. So does AP-Autoconfiguration WSC without an M1 or M2
//!   selector.
//!
//! Intentional differences: validation is stateless (upstream never resets
//! its presence flags or error counter), errors are returned instead of
//! printed, and no length is subtracted before it is checked. For 26 rows
//! where upstream's minimum rejects TLVs the spec allows, the minimum is the
//! spec's ([`crate::spec_min`], note 0008 D8).
//!
//! Stricter checks (length of every instance, length of optional TLVs) are not
//! done, because upstream's own senders would fail them.

use core::fmt;

use em_types::{MsgType, ProfileType, TlvType};

use crate::error::FramingError;
use crate::header::CmduHeader;
use crate::spec_min;
use crate::tables::{Requirement, Row, TABLES, Table, WscKind};
use crate::tlv::{Tlv, TlvIter};
use crate::wsc::{WSC_M1, WSC_M2, wsc_msg_type};

/// What is wrong with a TLV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    /// A mandatory TLV is absent.
    Missing,
    /// A mandatory TLV is shorter than the minimum length checked for its row
    /// (upstream's, or the spec minimum from [`crate::spec_min`]).
    TooShort,
    /// A TLV that must not be present (for this profile) is present.
    NotAllowed,
}

/// One table row that the message breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    /// TLV type of the row.
    pub tlv_type: TlvType,
    /// What is wrong.
    pub kind: ViolationKind,
    /// Specification reference of the row.
    pub spec_ref: &'static str,
}

/// Why validation failed.
#[derive(Debug, PartialEq, Eq)]
pub enum ValidateError {
    /// The CMDU or its TLV area is malformed.
    Framing(FramingError),
    /// The framing is fine, but the table is broken. One entry per row, in row order.
    Table(Vec<Violation>),
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(f, "malformed message: {error}"),
            Self::Table(violations) => {
                write!(f, "{} TLV rule(s) broken", violations.len())
            }
        }
    }
}

impl std::error::Error for ValidateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(error) => Some(error),
            Self::Table(_) => None,
        }
    }
}

/// Checks a CMDU against the table of its message type.
///
/// `cmdu` starts at the CMDU header. The message type comes from the header.
/// `profile` picks the rule of profile-dependent rows; it is compared by its
/// raw number, as upstream does.
///
/// # Errors
///
/// [`ValidateError::Framing`] if the header is truncated, a TLV length runs
/// past the end, or End of Message is missing. [`ValidateError::Table`] with
/// one [`Violation`] per broken row otherwise.
pub fn validate(cmdu: &[u8], profile: ProfileType) -> Result<(), ValidateError> {
    let (header, tlv_area) = CmduHeader::parse(cmdu).map_err(ValidateError::Framing)?;
    for item in TlvIter::new(tlv_area) {
        item.map_err(ValidateError::Framing)?;
    }
    let Some(table) = choose_table(header.msg_type, tlv_area) else {
        return Ok(());
    };
    let mut violations = Vec::new();
    for (index, row) in table.rows.iter().enumerate() {
        let min_len = spec_min::min_len(table, index, row);
        if let Some(violation) = check_row(row, min_len, tlv_area, profile) {
            violations.push(violation);
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ValidateError::Table(violations))
    }
}

/// The table for `msg_type`, or `None` if there is none.
fn choose_table(msg_type: MsgType, tlv_area: &[u8]) -> Option<&'static Table> {
    let wanted = if msg_type == MsgType::AP_AUTOCONFIGURATION_WSC {
        match wsc_msg_type(tlv_area) {
            Some(WSC_M1) => Some(WscKind::M1),
            Some(WSC_M2) => Some(WscKind::M2),
            _ => return None,
        }
    } else {
        None
    };
    TABLES
        .iter()
        .find(|table| table.msg_type == msg_type && table.wsc == wanted)
}

/// Checks one row against the first TLV of its type. `min_len` is the length to
/// check for, which may be lower than `row.min_len` (see [`spec_min`]).
fn check_row(row: &Row, min_len: u16, tlv_area: &[u8], profile: ProfileType) -> Option<Violation> {
    let found = first_tlv_of_type(tlv_area, row.tlv_type);
    let kind = match (row.rule.resolve(profile), found) {
        (Requirement::Mandatory, None) => ViolationKind::Missing,
        (Requirement::Mandatory, Some(tlv)) if 3 + tlv.value.len() < usize::from(min_len) => {
            ViolationKind::TooShort
        }
        (Requirement::Bad, Some(_)) => ViolationKind::NotAllowed,
        _ => return None,
    };
    Some(Violation {
        tlv_type: row.tlv_type,
        kind,
        spec_ref: row.spec_ref,
    })
}

/// The first TLV of `tlv_type`. The area has already passed the framing walk.
fn first_tlv_of_type(tlv_area: &[u8], tlv_type: TlvType) -> Option<Tlv<'_>> {
    TlvIter::new(tlv_area)
        .flatten()
        .find(|tlv| tlv.tlv_type == tlv_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{AL_SAP_BUDGET, MsgBuilder};
    use crate::tables::Rule;

    const PROFILES: [u8; 5] = [0, 1, 2, 3, 4];

    const SPEC_AL_MAC: &str = "table 6-8 of IEEE-1905-1";
    const SPEC_SEARCHED_ROLE: &str = "table 6-22 of IEEE-1905-1";
    const SPEC_FREQ_BAND: &str = "table 6-23 of IEEE-1905-1";
    const SPEC_PROFILE: &str = "17.2.47 of Wi-Fi Easy Mesh 5.0";
    const SPEC_DPP: &str = "17.2.83 of Wi-Fi Easy Mesh 5.0";
    const SPEC_SECURITY_CAP: &str = "17.2.67 of Wi-Fi Easy Mesh 5.0";
    const SPEC_WSC_M1: &str = "table 8 of WSC v2.0.7";
    const SPEC_WSC_M2: &str = "table 9 of WSC v2.0.7";

    /// A TLV with a zero-filled value, `total` bytes long including the TLV header.
    fn z(tlv_type: TlvType, total: usize) -> (TlvType, Vec<u8>) {
        (tlv_type, vec![0; total - 3])
    }

    /// A WSC TLV, `total` bytes long, whose first attribute is Message Type `selector`.
    fn wsc_tlv(selector: u8, total: usize) -> (TlvType, Vec<u8>) {
        let mut value = vec![0x10, 0x22, 0x00, 0x01, selector];
        value.resize(total - 3, 0);
        (TlvType::WSC, value)
    }

    fn header_for(msg_type: MsgType) -> CmduHeader {
        CmduHeader {
            msg_type,
            msg_id: 1,
            frag_id: 0,
            last_fragment: true,
            relay: false,
        }
    }

    fn build(msg_type: MsgType, tlvs: &[(TlvType, Vec<u8>)]) -> Vec<u8> {
        let mut builder = MsgBuilder::new(&header_for(msg_type), AL_SAP_BUDGET);
        for (tlv_type, value) in tlvs {
            builder.tlv(*tlv_type, value);
        }
        builder.finish().unwrap()
    }

    fn check(
        msg_type: MsgType,
        tlvs: &[(TlvType, Vec<u8>)],
        profile: u8,
    ) -> Result<(), ValidateError> {
        validate(&build(msg_type, tlvs), ProfileType(profile))
    }

    fn violation(tlv_type: TlvType, kind: ViolationKind, spec_ref: &'static str) -> Violation {
        Violation {
            tlv_type,
            kind,
            spec_ref,
        }
    }

    fn table_error(violations: Vec<Violation>) -> Result<(), ValidateError> {
        Err(ValidateError::Table(violations))
    }

    // AP-Autoconfiguration Search ------------------------------------------

    fn search_base() -> Vec<(TlvType, Vec<u8>)> {
        vec![
            z(TlvType::AL_MAC_ADDRESS, 9),
            z(TlvType::SEARCHED_ROLE, 4),
            z(TlvType::AUTOCONFIG_FREQ_BAND, 4),
        ]
    }

    fn search_with(extra: Vec<(TlvType, Vec<u8>)>) -> Vec<(TlvType, Vec<u8>)> {
        let mut tlvs = search_base();
        tlvs.extend(extra);
        tlvs
    }

    #[test]
    fn search_valid_message_passes_at_every_profile() {
        for profile in PROFILES {
            let mut tlvs = search_base();
            if profile > 1 {
                tlvs.push(z(TlvType::MULTI_AP_PROFILE, 4));
            }
            let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &tlvs, profile);
            assert_eq!(result, Ok(()), "profile {profile}");
        }
    }

    #[test]
    fn search_profile_tlv_is_mandatory_above_profile_1() {
        let missing = violation(
            TlvType::MULTI_AP_PROFILE,
            ViolationKind::Missing,
            SPEC_PROFILE,
        );
        for profile in PROFILES {
            let result = check(
                MsgType::AP_AUTOCONFIGURATION_SEARCH,
                &search_base(),
                profile,
            );
            let expected = if profile > 1 {
                table_error(vec![missing])
            } else {
                Ok(())
            };
            assert_eq!(result, expected, "profile {profile}");
        }
    }

    #[test]
    fn search_profile_tlv_is_not_allowed_up_to_profile_1() {
        let not_allowed = violation(
            TlvType::MULTI_AP_PROFILE,
            ViolationKind::NotAllowed,
            SPEC_PROFILE,
        );
        let tlvs = search_with(vec![z(TlvType::MULTI_AP_PROFILE, 4)]);
        for profile in PROFILES {
            let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &tlvs, profile);
            let expected = if profile > 1 {
                Ok(())
            } else {
                table_error(vec![not_allowed])
            };
            assert_eq!(result, expected, "profile {profile}");
        }
    }

    #[test]
    fn search_chirp_value_is_optional_above_profile_2_and_bad_below() {
        let tlvs = search_with(vec![
            z(TlvType::MULTI_AP_PROFILE, 4),
            z(TlvType::DPP_CHIRP_VALUE, 4),
        ]);
        for profile in PROFILES {
            let mut expected_violations = Vec::new();
            if profile <= 1 {
                expected_violations.push(violation(
                    TlvType::MULTI_AP_PROFILE,
                    ViolationKind::NotAllowed,
                    SPEC_PROFILE,
                ));
            }
            if profile <= 2 {
                expected_violations.push(violation(
                    TlvType::DPP_CHIRP_VALUE,
                    ViolationKind::NotAllowed,
                    SPEC_DPP,
                ));
            }
            let expected = if expected_violations.is_empty() {
                Ok(())
            } else {
                table_error(expected_violations)
            };
            let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &tlvs, profile);
            assert_eq!(result, expected, "profile {profile}");
        }
    }

    #[test]
    fn search_reports_missing_in_row_order_once_per_row() {
        let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &[], 3);
        let expected = table_error(vec![
            violation(TlvType::AL_MAC_ADDRESS, ViolationKind::Missing, SPEC_AL_MAC),
            violation(
                TlvType::SEARCHED_ROLE,
                ViolationKind::Missing,
                SPEC_SEARCHED_ROLE,
            ),
            violation(
                TlvType::AUTOCONFIG_FREQ_BAND,
                ViolationKind::Missing,
                SPEC_FREQ_BAND,
            ),
            violation(
                TlvType::MULTI_AP_PROFILE,
                ViolationKind::Missing,
                SPEC_PROFILE,
            ),
        ]);
        assert_eq!(result, expected);
    }

    #[test]
    fn search_reports_too_short() {
        let tlvs = vec![
            z(TlvType::AL_MAC_ADDRESS, 8),
            z(TlvType::SEARCHED_ROLE, 3),
            z(TlvType::AUTOCONFIG_FREQ_BAND, 4),
            z(TlvType::MULTI_AP_PROFILE, 3),
        ];
        let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &tlvs, 2);
        let expected = table_error(vec![
            violation(
                TlvType::AL_MAC_ADDRESS,
                ViolationKind::TooShort,
                SPEC_AL_MAC,
            ),
            violation(
                TlvType::SEARCHED_ROLE,
                ViolationKind::TooShort,
                SPEC_SEARCHED_ROLE,
            ),
            violation(
                TlvType::MULTI_AP_PROFILE,
                ViolationKind::TooShort,
                SPEC_PROFILE,
            ),
        ]);
        assert_eq!(result, expected);
    }

    #[test]
    fn search_not_allowed_is_reported_for_any_length() {
        let tlvs = search_with(vec![z(TlvType::MULTI_AP_PROFILE, 3)]);
        let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &tlvs, 1);
        let expected = table_error(vec![violation(
            TlvType::MULTI_AP_PROFILE,
            ViolationKind::NotAllowed,
            SPEC_PROFILE,
        )]);
        assert_eq!(result, expected);
    }

    #[test]
    fn search_duplicate_violating_rows_are_reported_once_each_in_row_order() {
        // Wire order is the reverse of table order: violations follow the table.
        let tlvs = vec![
            z(TlvType::MULTI_AP_PROFILE, 3),
            z(TlvType::MULTI_AP_PROFILE, 3),
            z(TlvType::AL_MAC_ADDRESS, 8),
            z(TlvType::AL_MAC_ADDRESS, 8),
            z(TlvType::SEARCHED_ROLE, 4),
            z(TlvType::AUTOCONFIG_FREQ_BAND, 4),
        ];
        let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &tlvs, 1);
        let expected = table_error(vec![
            violation(
                TlvType::AL_MAC_ADDRESS,
                ViolationKind::TooShort,
                SPEC_AL_MAC,
            ),
            violation(
                TlvType::MULTI_AP_PROFILE,
                ViolationKind::NotAllowed,
                SPEC_PROFILE,
            ),
        ]);
        assert_eq!(result, expected);
    }

    #[test]
    fn mandatory_length_is_checked_on_the_first_instance_only() {
        let long_then_short = search_with(vec![z(TlvType::AL_MAC_ADDRESS, 3)]);
        assert_eq!(
            check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &long_then_short, 1),
            Ok(())
        );

        let short_then_long = vec![
            z(TlvType::AL_MAC_ADDRESS, 3),
            z(TlvType::AL_MAC_ADDRESS, 9),
            z(TlvType::SEARCHED_ROLE, 4),
            z(TlvType::AUTOCONFIG_FREQ_BAND, 4),
        ];
        let result = check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &short_then_long, 1);
        let expected = table_error(vec![violation(
            TlvType::AL_MAC_ADDRESS,
            ViolationKind::TooShort,
            SPEC_AL_MAC,
        )]);
        assert_eq!(result, expected);
    }

    #[test]
    fn unknown_tlv_types_pass() {
        let unknown = TlvType(0x77);
        assert_eq!(unknown.name(), None);
        let tlvs = search_with(vec![z(unknown, 3), z(unknown, 40)]);
        assert_eq!(
            check(MsgType::AP_AUTOCONFIGURATION_SEARCH, &tlvs, 1),
            Ok(())
        );
    }

    // AP-Autoconfiguration Response -----------------------------------------

    #[test]
    fn response_security_capability_flips_at_profile_3() {
        for profile in PROFILES {
            let mut with = vec![
                z(TlvType::SUPPORTED_ROLE, 4),
                z(TlvType::SUPPORTED_FREQ_BAND, 4),
            ];
            if profile > 1 {
                with.push(z(TlvType::MULTI_AP_PROFILE, 4));
            }
            // Without the security capability TLV.
            let missing = check(MsgType::AP_AUTOCONFIGURATION_RESPONSE, &with, profile);
            let expected = if profile > 2 {
                table_error(vec![violation(
                    TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY,
                    ViolationKind::Missing,
                    SPEC_SECURITY_CAP,
                )])
            } else {
                Ok(())
            };
            assert_eq!(missing, expected, "missing, profile {profile}");
            // With it.
            with.push(z(TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY, 6));
            let present = check(MsgType::AP_AUTOCONFIGURATION_RESPONSE, &with, profile);
            let expected = if profile > 2 {
                Ok(())
            } else {
                table_error(vec![violation(
                    TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY,
                    ViolationKind::NotAllowed,
                    SPEC_SECURITY_CAP,
                )])
            };
            assert_eq!(present, expected, "present, profile {profile}");
        }
    }

    #[test]
    fn optional_rows_are_not_length_checked() {
        let tlvs = vec![
            z(TlvType::SUPPORTED_ROLE, 4),
            z(TlvType::SUPPORTED_FREQ_BAND, 4),
            z(TlvType::SUPPORTED_SERVICE, 3),
            z(TlvType::CONTROLLER_CAPABILITY, 3),
        ];
        assert_eq!(
            check(MsgType::AP_AUTOCONFIGURATION_RESPONSE, &tlvs, 1),
            Ok(())
        );
    }

    // WSC M1 and M2 -----------------------------------------------------------

    fn m1_base() -> Vec<(TlvType, Vec<u8>)> {
        vec![z(TlvType::AP_RADIO_BASIC_CAPABILITIES, 3), wsc_tlv(4, 264)]
    }

    fn m2_base() -> Vec<(TlvType, Vec<u8>)> {
        vec![z(TlvType::AP_RADIO_IDENTIFIER, 9), wsc_tlv(5, 264)]
    }

    #[test]
    fn wsc_m1_profile_rows_flip_at_profile_2() {
        let spec_p2_cap = "17.2.48 of Wi-Fi Easy Mesh 5.0";
        let spec_advanced = "17.2.52 of Wi-Fi Easy Mesh 5.0";
        let mut both = m1_base();
        both.push(z(TlvType::PROFILE_2_AP_CAPABILITY, 3));
        both.push(z(TlvType::AP_RADIO_ADVANCED_CAPABILITIES, 3));
        for profile in PROFILES {
            let without = check(MsgType::AP_AUTOCONFIGURATION_WSC, &m1_base(), profile);
            let expected = if profile > 1 {
                table_error(vec![
                    violation(
                        TlvType::PROFILE_2_AP_CAPABILITY,
                        ViolationKind::Missing,
                        spec_p2_cap,
                    ),
                    violation(
                        TlvType::AP_RADIO_ADVANCED_CAPABILITIES,
                        ViolationKind::Missing,
                        spec_advanced,
                    ),
                ])
            } else {
                Ok(())
            };
            assert_eq!(without, expected, "without, profile {profile}");

            let with = check(MsgType::AP_AUTOCONFIGURATION_WSC, &both, profile);
            let expected = if profile > 1 {
                Ok(())
            } else {
                table_error(vec![
                    violation(
                        TlvType::PROFILE_2_AP_CAPABILITY,
                        ViolationKind::NotAllowed,
                        spec_p2_cap,
                    ),
                    violation(
                        TlvType::AP_RADIO_ADVANCED_CAPABILITIES,
                        ViolationKind::NotAllowed,
                        spec_advanced,
                    ),
                ])
            };
            assert_eq!(with, expected, "with, profile {profile}");
        }
    }

    #[test]
    fn wsc_m1_wsc_tlv_too_short_and_missing() {
        let short = vec![z(TlvType::AP_RADIO_BASIC_CAPABILITIES, 3), wsc_tlv(4, 263)];
        let result = check(MsgType::AP_AUTOCONFIGURATION_WSC, &short, 1);
        let expected = table_error(vec![violation(
            TlvType::WSC,
            ViolationKind::TooShort,
            SPEC_WSC_M1,
        )]);
        assert_eq!(result, expected);

        // A selector needs a WSC TLV, so the M1 table cannot be chosen without one: framing only.
        let none = vec![z(TlvType::AP_RADIO_BASIC_CAPABILITIES, 3)];
        assert_eq!(check(MsgType::AP_AUTOCONFIGURATION_WSC, &none, 1), Ok(()));
    }

    #[test]
    fn wsc_m1_zero_length_basic_capabilities_passes() {
        // Minimum length 2, below the 3-byte TLV header: a zero-length TLV passes.
        assert_eq!(
            check(MsgType::AP_AUTOCONFIGURATION_WSC, &m1_base(), 1),
            Ok(())
        );
    }

    #[test]
    fn wsc_m1_missing_mandatory_rows() {
        let tlvs = vec![wsc_tlv(4, 264)];
        let result = check(MsgType::AP_AUTOCONFIGURATION_WSC, &tlvs, 1);
        let expected = table_error(vec![violation(
            TlvType::AP_RADIO_BASIC_CAPABILITIES,
            ViolationKind::Missing,
            "17.2.7 of Wi-Fi Easy Mesh 5.0",
        )]);
        assert_eq!(result, expected);
    }

    #[test]
    fn wsc_m2_uses_its_own_table() {
        let spec_8021q = "17.2.49 of Wi-Fi Easy Mesh 5.0";
        let spec_separation = "17.2.50 of Wi-Fi Easy Mesh 5.0";
        let mut with_optional = m2_base();
        with_optional.push(z(TlvType::DEFAULT_8021Q_SETTINGS, 3));
        with_optional.push(z(TlvType::TRAFFIC_SEPARATION_POLICY, 3));
        for profile in PROFILES {
            assert_eq!(
                check(MsgType::AP_AUTOCONFIGURATION_WSC, &m2_base(), profile),
                Ok(()),
                "base, profile {profile}"
            );
            let result = check(MsgType::AP_AUTOCONFIGURATION_WSC, &with_optional, profile);
            let expected = if profile > 1 {
                Ok(())
            } else {
                table_error(vec![
                    violation(
                        TlvType::DEFAULT_8021Q_SETTINGS,
                        ViolationKind::NotAllowed,
                        spec_8021q,
                    ),
                    violation(
                        TlvType::TRAFFIC_SEPARATION_POLICY,
                        ViolationKind::NotAllowed,
                        spec_separation,
                    ),
                ])
            };
            assert_eq!(result, expected, "optional rows, profile {profile}");
        }
        // M2 needs the radio id, not M1's basic capabilities.
        let m2_without_radio = vec![wsc_tlv(5, 264)];
        let result = check(MsgType::AP_AUTOCONFIGURATION_WSC, &m2_without_radio, 1);
        let expected = table_error(vec![violation(
            TlvType::AP_RADIO_IDENTIFIER,
            ViolationKind::Missing,
            "17.2.3 of Wi-Fi Easy Mesh 5.0",
        )]);
        assert_eq!(result, expected);
        let short = vec![z(TlvType::AP_RADIO_IDENTIFIER, 9), wsc_tlv(5, 100)];
        let result = check(MsgType::AP_AUTOCONFIGURATION_WSC, &short, 1);
        let expected = table_error(vec![violation(
            TlvType::WSC,
            ViolationKind::TooShort,
            SPEC_WSC_M2,
        )]);
        assert_eq!(result, expected);
    }

    #[test]
    fn wsc_without_a_selector_or_with_another_value_gets_framing_checks_only() {
        // No Message Type attribute at all.
        let no_selector = vec![(TlvType::WSC, vec![0x10, 0x4A, 0x00, 0x01, 0x10])];
        assert_eq!(
            check(MsgType::AP_AUTOCONFIGURATION_WSC, &no_selector, 3),
            Ok(())
        );
        // No TLVs at all.
        assert_eq!(check(MsgType::AP_AUTOCONFIGURATION_WSC, &[], 3), Ok(()));
        // Selector values other than 4 and 5, among them M2D (6).
        for selector in [0, 1, 3, 6, 7, 0xFF] {
            let tlvs = vec![wsc_tlv(selector, 10)];
            let result = check(MsgType::AP_AUTOCONFIGURATION_WSC, &tlvs, 3);
            assert_eq!(result, Ok(()), "selector {selector}");
        }
    }

    #[test]
    fn wsc_selector_6_still_gets_framing_checks() {
        let mut bytes = build(MsgType::AP_AUTOCONFIGURATION_WSC, &[wsc_tlv(6, 10)]);
        bytes.truncate(bytes.len() - 1);
        let result = validate(&bytes, ProfileType(3));
        assert_eq!(
            result,
            Err(ValidateError::Framing(FramingError::MissingEndOfMessage))
        );
    }

    // Topology -------------------------------------------------------------------

    fn topology_response_base() -> Vec<(TlvType, Vec<u8>)> {
        vec![
            z(TlvType::DEVICE_INFORMATION, 19),
            z(TlvType::AP_OPERATIONAL_BSS, 18),
        ]
    }

    #[test]
    fn topology_response_valid_at_every_profile() {
        for profile in PROFILES {
            let mut tlvs = topology_response_base();
            if profile > 1 {
                tlvs.push(z(TlvType::MULTI_AP_PROFILE, 4));
            }
            if profile > 2 {
                tlvs.push(z(TlvType::BSS_CONFIGURATION_REPORT, 17));
            }
            assert_eq!(
                check(MsgType::TOPOLOGY_RESPONSE, &tlvs, profile),
                Ok(()),
                "profile {profile}"
            );
        }
    }

    #[test]
    fn topology_response_bss_configuration_report_flips_at_profile_3() {
        let spec = "17.2.75 of Wi-Fi Easy Mesh 5.0";
        let mut tlvs = topology_response_base();
        tlvs.push(z(TlvType::MULTI_AP_PROFILE, 4));
        for profile in [2, 3, 4] {
            let result = check(MsgType::TOPOLOGY_RESPONSE, &tlvs, profile);
            let expected = if profile > 2 {
                table_error(vec![violation(
                    TlvType::BSS_CONFIGURATION_REPORT,
                    ViolationKind::Missing,
                    spec,
                )])
            } else {
                Ok(())
            };
            assert_eq!(result, expected, "profile {profile}");
        }
        tlvs.push(z(TlvType::BSS_CONFIGURATION_REPORT, 17));
        let result = check(MsgType::TOPOLOGY_RESPONSE, &tlvs, 2);
        let expected = table_error(vec![violation(
            TlvType::BSS_CONFIGURATION_REPORT,
            ViolationKind::NotAllowed,
            spec,
        )]);
        assert_eq!(result, expected);
    }

    #[test]
    fn topology_response_too_short_device_information() {
        let tlvs = vec![
            z(TlvType::DEVICE_INFORMATION, 18),
            z(TlvType::AP_OPERATIONAL_BSS, 18),
        ];
        let result = check(MsgType::TOPOLOGY_RESPONSE, &tlvs, 1);
        let expected = table_error(vec![violation(
            TlvType::DEVICE_INFORMATION,
            ViolationKind::TooShort,
            "table 6-10 of IEEE-1905-1",
        )]);
        assert_eq!(result, expected);
    }

    #[test]
    fn upstream_sender_topology_response_with_one_byte_ap_mld_configuration_passes() {
        for profile in PROFILES {
            let mut tlvs = topology_response_base();
            if profile > 1 {
                tlvs.push(z(TlvType::MULTI_AP_PROFILE, 4));
            }
            if profile > 2 {
                tlvs.push(z(TlvType::BSS_CONFIGURATION_REPORT, 17));
            }
            // Value of one byte: 4 bytes in total, far below the row's minimum of 64.
            tlvs.push(z(TlvType::AGENT_AP_MLD_CONFIGURATION, 4));
            let result = check(MsgType::TOPOLOGY_RESPONSE, &tlvs, profile);
            assert_eq!(result, Ok(()), "profile {profile}");
        }
    }

    #[test]
    fn topology_discovery_has_the_same_rules_at_every_profile() {
        let spec_mac = "table 6-9 of IEEE-1905-1";
        let valid = vec![z(TlvType::AL_MAC_ADDRESS, 9), z(TlvType::MAC_ADDRESS, 9)];
        let missing_mac = vec![z(TlvType::AL_MAC_ADDRESS, 9)];
        let short_al_mac = vec![z(TlvType::AL_MAC_ADDRESS, 4), z(TlvType::MAC_ADDRESS, 9)];
        for profile in PROFILES {
            assert_eq!(
                check(MsgType::TOPOLOGY_DISCOVERY, &valid, profile),
                Ok(()),
                "profile {profile}"
            );
            assert_eq!(
                check(MsgType::TOPOLOGY_DISCOVERY, &missing_mac, profile),
                table_error(vec![violation(
                    TlvType::MAC_ADDRESS,
                    ViolationKind::Missing,
                    spec_mac
                )]),
                "profile {profile}"
            );
            assert_eq!(
                check(MsgType::TOPOLOGY_DISCOVERY, &short_al_mac, profile),
                table_error(vec![violation(
                    TlvType::AL_MAC_ADDRESS,
                    ViolationKind::TooShort,
                    SPEC_AL_MAC
                )]),
                "profile {profile}"
            );
        }
    }

    // AP Metrics Response ---------------------------------------------------------

    #[test]
    fn ap_metrics_response_profile_rows() {
        let spec_extended = "17.2.61 of Wi-Fi Easy Mesh 5.0";
        let spec_radio = "17.2.60 of Wi-Fi Easy Mesh 5.0";
        for profile in PROFILES {
            let mut tlvs = vec![z(TlvType::AP_METRICS, 16)];
            if profile > 1 {
                tlvs.push(z(TlvType::AP_EXTENDED_METRICS, 33));
            }
            assert_eq!(
                check(MsgType::AP_METRICS_RESPONSE, &tlvs, profile),
                Ok(()),
                "valid, profile {profile}"
            );

            // Radio Metrics is optional above profile 1 and not allowed up to it.
            tlvs.push(z(TlvType::RADIO_METRICS, 13));
            let result = check(MsgType::AP_METRICS_RESPONSE, &tlvs, profile);
            let expected = if profile > 1 {
                Ok(())
            } else {
                table_error(vec![violation(
                    TlvType::RADIO_METRICS,
                    ViolationKind::NotAllowed,
                    spec_radio,
                )])
            };
            assert_eq!(result, expected, "radio metrics, profile {profile}");
        }
        let only_ap_metrics = vec![z(TlvType::AP_METRICS, 15)];
        let result = check(MsgType::AP_METRICS_RESPONSE, &only_ap_metrics, 2);
        let expected = table_error(vec![
            violation(
                TlvType::AP_METRICS,
                ViolationKind::TooShort,
                "17.2.22 of Wi-Fi Easy Mesh 5.0",
            ),
            violation(
                TlvType::AP_EXTENDED_METRICS,
                ViolationKind::Missing,
                spec_extended,
            ),
        ]);
        assert_eq!(result, expected);
    }

    #[test]
    fn upstream_sender_ap_metrics_response_with_zero_length_second_ap_metrics_passes() {
        for profile in PROFILES {
            let mut tlvs = vec![z(TlvType::AP_METRICS, 16)];
            if profile > 1 {
                tlvs.push(z(TlvType::AP_EXTENDED_METRICS, 33));
            }
            tlvs.push(z(TlvType::AP_METRICS, 3));
            let result = check(MsgType::AP_METRICS_RESPONSE, &tlvs, profile);
            assert_eq!(result, Ok(()), "profile {profile}");
        }
    }

    #[test]
    fn ap_metrics_response_checks_the_first_ap_metrics_not_the_later_one() {
        let tlvs = vec![z(TlvType::AP_METRICS, 3), z(TlvType::AP_METRICS, 16)];
        let result = check(MsgType::AP_METRICS_RESPONSE, &tlvs, 1);
        let expected = table_error(vec![violation(
            TlvType::AP_METRICS,
            ViolationKind::TooShort,
            "17.2.22 of Wi-Fi Easy Mesh 5.0",
        )]);
        assert_eq!(result, expected);
    }

    // Other cases ---------------------------------------------------------------------

    #[test]
    fn duplicate_rows_are_separate_rows() {
        let mut duplicated = 0;
        for table in TABLES {
            if table.msg_type == MsgType::COMBINED_INFRASTRUCTURE_METRICS {
                assert_eq!(table.rows.len(), 5);
                assert_eq!(table.rows[1], table.rows[2]);
                assert_eq!(table.rows[3], table.rows[4]);
                duplicated += 1;
            }
        }
        assert_eq!(duplicated, 1);
        // Absent, each duplicate row reports once.
        let result = check(MsgType::COMBINED_INFRASTRUCTURE_METRICS, &[], 1);
        let Err(ValidateError::Table(violations)) = result else {
            panic!("expected table violations, got {result:?}");
        };
        let found: Vec<(TlvType, ViolationKind)> =
            violations.iter().map(|v| (v.tlv_type, v.kind)).collect();
        assert_eq!(
            found,
            [
                (TlvType::AP_METRICS, ViolationKind::Missing),
                (TlvType::TRANSMITTER_LINK_METRIC, ViolationKind::Missing),
                (TlvType::TRANSMITTER_LINK_METRIC, ViolationKind::Missing),
                (TlvType::RECEIVER_LINK_METRIC, ViolationKind::Missing),
                (TlvType::RECEIVER_LINK_METRIC, ViolationKind::Missing),
            ]
        );
    }

    #[test]
    fn present_zero_length_mandatory_tlv_with_minimum_3_passes() {
        // Tunneled: the Tunneled TLV has minimum 3, and a zero-length TLV is exactly 3 bytes.
        let tlvs = vec![
            z(TlvType::SOURCE_INFO, 9),
            z(TlvType::TUNNELED_MESSAGE_TYPE, 4),
            z(TlvType::TUNNELED, 3),
        ];
        assert_eq!(check(MsgType::TUNNELED, &tlvs, 1), Ok(()));
        let mut found = false;
        for table in TABLES {
            if table.msg_type == MsgType::TUNNELED {
                assert_eq!(table.rows[2].tlv_type, TlvType::TUNNELED);
                assert_eq!(table.rows[2].min_len, 3);
                assert_eq!(table.rows[2].rule, Rule::Fixed(Requirement::Mandatory));
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn empty_table_gets_framing_checks_only() {
        assert_eq!(check(MsgType::AP_CAPABILITY_QUERY, &[], 3), Ok(()));
        let tlvs = vec![z(TlvType::MULTI_AP_PROFILE, 10), z(TlvType(0x77), 3)];
        assert_eq!(check(MsgType::AP_CAPABILITY_QUERY, &tlvs, 3), Ok(()));
        let mut bytes = build(MsgType::AP_CAPABILITY_QUERY, &[]);
        bytes.truncate(bytes.len() - 3);
        assert_eq!(
            validate(&bytes, ProfileType(3)),
            Err(ValidateError::Framing(FramingError::MissingEndOfMessage))
        );
    }

    #[test]
    fn unlisted_message_type_gets_framing_checks_only() {
        for msg_type in [MsgType::BSTA_MLD_CONFIGURATION_REQUEST, MsgType(0xFFFF)] {
            assert_eq!(check(msg_type, &[], 3), Ok(()), "{msg_type:?}");
            let tlvs = vec![z(TlvType::AL_MAC_ADDRESS, 3)];
            assert_eq!(check(msg_type, &tlvs, 3), Ok(()), "{msg_type:?}");
        }
        let mut bytes = build(
            MsgType::BSTA_MLD_CONFIGURATION_REQUEST,
            &[z(TlvType::AL_MAC_ADDRESS, 9)],
        );
        bytes.truncate(bytes.len() - 1);
        assert_eq!(
            validate(&bytes, ProfileType(3)),
            Err(ValidateError::Framing(FramingError::MissingEndOfMessage))
        );
    }

    #[test]
    fn framing_errors_come_before_table_checks() {
        // Topology Discovery with nothing in it would give Missing violations, but the framing fails first.
        let header_only = build(MsgType::TOPOLOGY_DISCOVERY, &[]);
        assert_eq!(
            validate(&header_only, ProfileType(1)),
            Err(ValidateError::Table(vec![
                violation(TlvType::AL_MAC_ADDRESS, ViolationKind::Missing, SPEC_AL_MAC),
                violation(
                    TlvType::MAC_ADDRESS,
                    ViolationKind::Missing,
                    "table 6-9 of IEEE-1905-1"
                ),
            ]))
        );

        let mut no_end = build(MsgType::TOPOLOGY_DISCOVERY, &[]);
        no_end.truncate(CmduHeader::LEN);
        assert_eq!(
            validate(&no_end, ProfileType(1)),
            Err(ValidateError::Framing(FramingError::MissingEndOfMessage))
        );

        no_end.extend_from_slice(&[0x42, 0x00, 0x09, 0x01]);
        assert_eq!(
            validate(&no_end, ProfileType(1)),
            Err(ValidateError::Framing(FramingError::TlvLengthPastEnd {
                tlv_type: 0x42
            }))
        );

        assert_eq!(
            validate(&header_only[..5], ProfileType(1)),
            Err(ValidateError::Framing(FramingError::TruncatedCmduHeader))
        );
        assert_eq!(
            validate(&[], ProfileType(1)),
            Err(ValidateError::Framing(FramingError::TruncatedCmduHeader))
        );
    }

    #[test]
    fn trailing_bytes_after_end_of_message_are_accepted() {
        let mut bytes = build(
            MsgType::TOPOLOGY_DISCOVERY,
            &[z(TlvType::AL_MAC_ADDRESS, 9), z(TlvType::MAC_ADDRESS, 9)],
        );
        bytes.extend_from_slice(&[0xFF; 20]);
        assert_eq!(validate(&bytes, ProfileType(1)), Ok(()));
    }

    #[test]
    fn validate_keeps_no_state_between_calls() {
        let good = build(
            MsgType::TOPOLOGY_DISCOVERY,
            &[z(TlvType::AL_MAC_ADDRESS, 9), z(TlvType::MAC_ADDRESS, 9)],
        );
        let bad = build(
            MsgType::TOPOLOGY_DISCOVERY,
            &[z(TlvType::AL_MAC_ADDRESS, 9)],
        );
        let expected_bad = table_error(vec![violation(
            TlvType::MAC_ADDRESS,
            ViolationKind::Missing,
            "table 6-9 of IEEE-1905-1",
        )]);
        // Many failures in a row never overflow anything, and a pass after a failure stays a pass.
        for _ in 0..200 {
            assert_eq!(validate(&bad, ProfileType(1)), expected_bad);
            assert_eq!(validate(&good, ProfileType(1)), Ok(()));
        }
        // The same bytes at another profile are judged afresh.
        let search = build(MsgType::AP_AUTOCONFIGURATION_SEARCH, &search_base());
        assert_eq!(validate(&search, ProfileType(1)), Ok(()));
        assert_eq!(
            validate(&search, ProfileType(2)),
            table_error(vec![violation(
                TlvType::MULTI_AP_PROFILE,
                ViolationKind::Missing,
                SPEC_PROFILE
            )])
        );
        assert_eq!(validate(&search, ProfileType(1)), Ok(()));
    }

    #[test]
    fn message_type_comes_from_the_header() {
        let tlvs = vec![z(TlvType::AL_MAC_ADDRESS, 9)];
        // The same TLV area, under two headers.
        assert_eq!(check(MsgType::AP_CAPABILITY_QUERY, &tlvs, 1), Ok(()));
        assert_eq!(
            check(MsgType::TOPOLOGY_DISCOVERY, &tlvs, 1),
            table_error(vec![violation(
                TlvType::MAC_ADDRESS,
                ViolationKind::Missing,
                "table 6-9 of IEEE-1905-1"
            )])
        );
    }

    #[test]
    fn error_display_and_source() {
        use std::error::Error;
        let framing = ValidateError::Framing(FramingError::MissingEndOfMessage);
        assert_eq!(
            framing.to_string(),
            "malformed message: the End of Message TLV is missing"
        );
        assert_eq!(
            framing.source().map(ToString::to_string).as_deref(),
            Some("the End of Message TLV is missing")
        );
        let table = ValidateError::Table(vec![violation(
            TlvType::AL_MAC_ADDRESS,
            ViolationKind::Missing,
            SPEC_AL_MAC,
        )]);
        assert_eq!(table.to_string(), "1 TLV rule(s) broken");
        assert!(table.source().is_none());
    }

    // Spec minimum lengths ---------------------------------------------------

    #[test]
    fn link_metric_query_validates_the_two_octet_minimum() {
        let query = crate::bodies::LinkMetricQuery::new(0, None, 0).unwrap();
        let mut builder = MsgBuilder::new(&header_for(MsgType::LINK_METRIC_QUERY), AL_SAP_BUDGET);
        builder.body(&query);
        let cmdu = builder.finish().unwrap();
        let (_, tlv_area) = CmduHeader::parse(&cmdu).unwrap();
        assert_eq!(
            TlvIter::new(tlv_area).next().unwrap().unwrap().value,
            &[0, 0]
        );
        let one_octet = [(TlvType::LINK_METRIC_QUERY, vec![0])];
        for profile in PROFILES {
            assert_eq!(validate(&cmdu, ProfileType(profile)), Ok(()));
            assert_eq!(
                check(MsgType::LINK_METRIC_QUERY, &one_octet, profile),
                table_error(vec![violation(
                    TlvType::LINK_METRIC_QUERY,
                    ViolationKind::TooShort,
                    "table 6-16 of IEEE-1905-1",
                )])
            );
        }
    }

    /// The violation kinds that `tlv_type` gets in a message of `msg_type`.
    fn kinds_for(
        msg_type: MsgType,
        tlvs: &[(TlvType, Vec<u8>)],
        profile: u8,
        tlv_type: TlvType,
    ) -> Vec<ViolationKind> {
        match check(msg_type, tlvs, profile) {
            Ok(()) => Vec::new(),
            Err(ValidateError::Table(violations)) => violations
                .into_iter()
                .filter(|violation| violation.tlv_type == tlv_type)
                .map(|violation| violation.kind)
                .collect(),
            Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn spec_minimum_is_checked_for_every_listed_row() {
        for entry in spec_min::SPEC_MIN_LENS {
            let table = TABLES
                .iter()
                .find(|table| table.upstream_name == entry.table)
                .unwrap_or_else(|| panic!("no table {}", entry.table));
            let row = table.rows[entry.row];
            let profile = PROFILES
                .into_iter()
                .find(|&profile| row.rule.resolve(ProfileType(profile)) == Requirement::Mandatory)
                .unwrap_or_else(|| panic!("{} row {} is never mandatory", entry.table, entry.row));

            let at_minimum = [z(entry.tlv_type, usize::from(entry.min_len))];
            let kinds = kinds_for(table.msg_type, &at_minimum, profile, entry.tlv_type);
            assert!(
                !kinds.contains(&ViolationKind::TooShort),
                "{} row {} at spec minimum",
                entry.table,
                entry.row
            );

            if entry.min_len > 3 {
                let one_short = [z(entry.tlv_type, usize::from(entry.min_len) - 1)];
                let kinds = kinds_for(table.msg_type, &one_short, profile, entry.tlv_type);
                assert!(
                    kinds.contains(&ViolationKind::TooShort),
                    "{} row {} one octet short of spec minimum",
                    entry.table,
                    entry.row
                );
            }
        }
    }

    #[test]
    fn combined_infra_metrics_one_transmitter_link_metric_passes_both_rows() {
        let tlvs = [z(TlvType::TRANSMITTER_LINK_METRIC, 44)];
        let kinds = kinds_for(
            MsgType::COMBINED_INFRASTRUCTURE_METRICS,
            &tlvs,
            1,
            TlvType::TRANSMITTER_LINK_METRIC,
        );
        assert_eq!(kinds, Vec::new());

        // One octet short: both rows report it.
        let short = [z(TlvType::TRANSMITTER_LINK_METRIC, 43)];
        let kinds = kinds_for(
            MsgType::COMBINED_INFRASTRUCTURE_METRICS,
            &short,
            1,
            TlvType::TRANSMITTER_LINK_METRIC,
        );
        assert_eq!(
            kinds,
            vec![ViolationKind::TooShort, ViolationKind::TooShort]
        );
    }

    #[test]
    fn overrides_are_keyed_by_row_not_by_tlv_type() {
        // AP Radio Basic Capabilities is lowered to 11 in three tables, but the
        // WSC M1 row keeps upstream's laxer minimum of 2.
        for total in 3..=10 {
            let tlvs = [
                z(TlvType::AP_RADIO_BASIC_CAPABILITIES, total),
                wsc_tlv(4, 264),
            ];
            let kinds = kinds_for(
                MsgType::AP_AUTOCONFIGURATION_WSC,
                &tlvs,
                1,
                TlvType::AP_RADIO_BASIC_CAPABILITIES,
            );
            assert_eq!(kinds, Vec::new(), "total {total}");
        }
    }

    #[test]
    fn unlisted_mandatory_row_keeps_upstream_minimum() {
        // topo_resp row 0, DEVICE_INFORMATION, is not in spec_min: upstream's 19 applies.
        let short = [z(TlvType::DEVICE_INFORMATION, 18)];
        assert_eq!(
            kinds_for(
                MsgType::TOPOLOGY_RESPONSE,
                &short,
                1,
                TlvType::DEVICE_INFORMATION
            ),
            vec![ViolationKind::TooShort]
        );
        let at_minimum = [z(TlvType::DEVICE_INFORMATION, 19)];
        assert_eq!(
            kinds_for(
                MsgType::TOPOLOGY_RESPONSE,
                &at_minimum,
                1,
                TlvType::DEVICE_INFORMATION
            ),
            Vec::new()
        );
    }
}
