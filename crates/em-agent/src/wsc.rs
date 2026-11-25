// SPDX-License-Identifier: Apache-2.0

//! The agent's WSC sessions and the content of its M1 (0013 D4, D6).
//!
//! The fake platform has no real device identity or radio capabilities, so this module
//! supplies fixed values. A `OneWifi` adapter would read them from the platform.

use std::time::Instant;

use em_cmd::NodeKey;
use em_core::wsc::{Identity, M1Params, Session};
use em_msg::bodies::{
    ApRadioAdvancedCapabilities, ApRadioBasicCapabilities, BasicOperatingClass,
    Profile2ApCapability,
};
use em_types::{FreqBand, MacAddr};
use em_utils::SortedVecMap;

/// BSSs a fake radio can carry: one fronthaul and one backhaul, with room for more.
const FAKE_MAX_BSS: u8 = 4;

/// One radio's outstanding M1 exchange and the time its M1 left (0013 D5).
#[derive(Debug, Default)]
struct RadioWsc {
    session: Session,
    m1_handed_off: Option<Instant>,
}

/// The WSC session of every radio, cleared on every terminal path (0013 D6).
#[derive(Debug, Default)]
pub(crate) struct WscSessions {
    radios: SortedVecMap<NodeKey, RadioWsc>,
}

impl WscSessions {
    /// The session of a radio, created empty on first use.
    pub(crate) fn session_mut(&mut self, key: NodeKey) -> &mut Session {
        &mut self
            .radios
            .get_or_insert_with(key, RadioWsc::default)
            .session
    }

    /// Records the instant the radio's M1 was handed to the transport.
    pub(crate) fn record_m1_handoff(&mut self, key: NodeKey, now: Instant) {
        self.radios
            .get_or_insert_with(key, RadioWsc::default)
            .m1_handed_off = Some(now);
    }

    /// When the radio's M1 was handed off, if its exchange is outstanding.
    pub(crate) fn m1_handed_off(&self, key: NodeKey) -> Option<Instant> {
        self.radios.get(&key).and_then(|radio| radio.m1_handed_off)
    }

    /// Whether the radio holds a live M1 exchange.
    #[cfg(test)]
    pub(crate) fn is_active(&self, key: NodeKey) -> bool {
        self.radios
            .get(&key)
            .is_some_and(|radio| radio.session.is_active())
    }

    /// Wipes the radio's session and forgets the hand-off time.
    pub(crate) fn clear(&mut self, key: NodeKey) {
        if let Some(radio) = self.radios.get_mut(&key) {
            radio.session.clear();
            radio.m1_handed_off = None;
        }
    }
}

/// The 802.11 operating class a fake radio of this band advertises.
fn operating_class(band: FreqBand) -> u8 {
    match band {
        FreqBand::BAND_5_GHZ => 115,
        FreqBand::BAND_6_GHZ => 131,
        FreqBand::BAND_60_GHZ => 180,
        _ => 81,
    }
}

/// The inputs of one radio's M1.
///
/// The enrollee MAC is the agent's AL MAC (0013 D6, `m1-mac-al`). The first 12 bytes of the
/// UUID are the AL MAC repeated twice and the last 4 are zero, so it is stable across
/// restarts.
pub(crate) fn m1_params(
    al_mac: MacAddr,
    controller: MacAddr,
    radio: MacAddr,
    band: FreqBand,
) -> M1Params<'static> {
    let mut uuid = [0_u8; 16];
    uuid[..6].copy_from_slice(&al_mac.octets());
    uuid[6..12].copy_from_slice(&al_mac.octets());
    M1Params {
        al_mac,
        controller,
        band,
        identity: Identity {
            uuid,
            manufacturer: b"em-agent",
            model: b"fake-radio",
            serial: b"0001",
            primary_device_type: [0, 6, 0, 0x50, 0xf2, 4, 0, 1],
        },
        basic: ApRadioBasicCapabilities {
            ruid: radio,
            max_bss: FAKE_MAX_BSS,
            operating_classes: vec![BasicOperatingClass {
                operating_class: operating_class(band),
                max_tx_power: 20,
                non_operable_channels: &[],
            }],
        },
        profile2: Profile2ApCapability {
            max_prioritization_rules: 0,
            byte_counter_units: 0,
            prioritization: false,
            dpp_onboarding: false,
            traffic_separation: false,
            max_vids: 0,
        },
        advanced: ApRadioAdvancedCapabilities {
            ruid: radio,
            combined_front_back: false,
            combined_profile_1_2: false,
            mscs: false,
            scs: false,
            qos_map: false,
            dscp_policy: false,
            qm_scs_traffic_description: false,
        },
    }
}
