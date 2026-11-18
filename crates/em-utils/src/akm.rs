// SPDX-License-Identifier: Apache-2.0

//! AKM (authentication and key management) names and suite selectors.
//!
//! Replaces upstream `akm_to_oui`, `oui_to_akm` and `akm_to_bytes`
//! (`src/utils/util.cpp:474-512`). The API works on bytes. Callers that need
//! upstream's text form (eight hex digits) do that conversion themselves.

/// One name and the four bytes of its suite selector.
struct AkmEntry {
    name: &'static str,
    suite: [u8; 4],
}

/// The map, in upstream order.
const AKM_TABLE: [AkmEntry; 7] = [
    AkmEntry {
        name: "wpa-eap",
        suite: [0x00, 0x50, 0xF2, 0x01],
    },
    AkmEntry {
        name: "wpa-psk",
        suite: [0x00, 0x50, 0xF2, 0x02],
    },
    AkmEntry {
        name: "wpa2-eap",
        suite: [0x00, 0x0F, 0xAC, 0x01],
    },
    AkmEntry {
        name: "wpa2-psk",
        suite: [0x00, 0x0F, 0xAC, 0x02],
    },
    AkmEntry {
        name: "psk",
        suite: [0x00, 0x0F, 0xAC, 0x02],
    },
    AkmEntry {
        name: "sae",
        suite: [0x00, 0x0F, 0xAC, 0x08],
    },
    AkmEntry {
        name: "dpp",
        suite: [0x50, 0x6F, 0x9A, 0x02],
    },
];

/// Returns the suite selector for an AKM name.
///
/// The name is compared ignoring ASCII case. Returns `None` for an unknown name.
#[must_use]
pub fn akm_to_suite(name: &str) -> Option<[u8; 4]> {
    for entry in &AKM_TABLE {
        if entry.name.eq_ignore_ascii_case(name) {
            return Some(entry.suite);
        }
    }
    None
}

/// Returns the AKM name for a suite selector.
///
/// `00 0F AC 02` gives `"wpa2-psk"`, not `"psk"`, because `"wpa2-psk"` comes
/// first in the table. Returns `None` for an unknown suite.
#[must_use]
pub fn suite_to_akm(suite: [u8; 4]) -> Option<&'static str> {
    // The first entry wins, so 000FAC02 maps back to "wpa2-psk".
    for entry in &AKM_TABLE {
        if entry.suite == suite {
            return Some(entry.name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{akm_to_suite, suite_to_akm};

    #[test]
    fn every_name_gives_its_suite() {
        assert_eq!(akm_to_suite("wpa-eap"), Some([0x00, 0x50, 0xF2, 0x01]));
        assert_eq!(akm_to_suite("wpa-psk"), Some([0x00, 0x50, 0xF2, 0x02]));
        assert_eq!(akm_to_suite("wpa2-eap"), Some([0x00, 0x0F, 0xAC, 0x01]));
        assert_eq!(akm_to_suite("wpa2-psk"), Some([0x00, 0x0F, 0xAC, 0x02]));
        assert_eq!(akm_to_suite("psk"), Some([0x00, 0x0F, 0xAC, 0x02]));
        assert_eq!(akm_to_suite("sae"), Some([0x00, 0x0F, 0xAC, 0x08]));
        assert_eq!(akm_to_suite("dpp"), Some([0x50, 0x6F, 0x9A, 0x02]));
    }

    #[test]
    fn every_suite_gives_its_name() {
        assert_eq!(suite_to_akm([0x00, 0x50, 0xF2, 0x01]), Some("wpa-eap"));
        assert_eq!(suite_to_akm([0x00, 0x50, 0xF2, 0x02]), Some("wpa-psk"));
        assert_eq!(suite_to_akm([0x00, 0x0F, 0xAC, 0x01]), Some("wpa2-eap"));
        assert_eq!(suite_to_akm([0x00, 0x0F, 0xAC, 0x02]), Some("wpa2-psk"));
        assert_eq!(suite_to_akm([0x00, 0x0F, 0xAC, 0x08]), Some("sae"));
        assert_eq!(suite_to_akm([0x50, 0x6F, 0x9A, 0x02]), Some("dpp"));
    }

    #[test]
    fn names_ignore_case() {
        assert_eq!(akm_to_suite("SAE"), Some([0x00, 0x0F, 0xAC, 0x08]));
        assert_eq!(akm_to_suite("Wpa2-Psk"), Some([0x00, 0x0F, 0xAC, 0x02]));
        assert_eq!(akm_to_suite("PSK"), Some([0x00, 0x0F, 0xAC, 0x02]));
    }

    #[test]
    fn psk_alias_maps_back_to_wpa2_psk() {
        let suite = akm_to_suite("psk").expect("psk is known");
        assert_eq!(suite, [0x00, 0x0F, 0xAC, 0x02]);
        assert_eq!(suite_to_akm(suite), Some("wpa2-psk"));
    }

    #[test]
    fn unknown_values_give_none() {
        assert_eq!(akm_to_suite("wep"), None);
        assert_eq!(akm_to_suite(""), None);
        assert_eq!(akm_to_suite("sae "), None);
        assert_eq!(suite_to_akm([0x00, 0x0F, 0xAC, 0x09]), None);
    }
}
