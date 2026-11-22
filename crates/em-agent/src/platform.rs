// SPDX-License-Identifier: Apache-2.0

//! What the agent asks of the platform (0012 D4, 0013 D4, 0001 D7 rule 3).
//!
//! The platform is a fake that reports the radios given on the command line and
//! confirms the configuration it is given. A `OneWifi` adapter can implement
//! [`Platform`] later without touching the actor.

use em_cmd::RadioSpec;
use em_core::wsc::BssConfig;
use em_types::{FreqBand, MacAddr};
use tokio::sync::mpsc::{self, Receiver, Sender};

/// How many confirmations can wait for the actor. The fake sends at most one per applied
/// configuration, and the actor drains them as they arrive.
const CONFIRMATION_QUEUE: usize = 8;

/// One BSS the platform confirmed, standing in for `OneWifi`'s callback subdoc
/// (`dm_easy_mesh_agent.cpp:309-385`).
#[derive(Debug, PartialEq, Eq)]
pub struct ConfirmedBss {
    /// The radio the BSS runs on.
    pub radio: MacAddr,
    /// The radio's band.
    pub band: FreqBand,
    /// The BSSID the platform chose.
    pub bssid: MacAddr,
    /// The SSID, without a terminator.
    pub ssid: Vec<u8>,
    /// The haul type of the BSS (0 fronthaul, 1 backhaul).
    pub haul_type: u16,
}

/// The platform's confirmation that the configuration of every radio is applied.
#[derive(Debug, PartialEq, Eq)]
pub struct Confirmation {
    /// Every BSS of the device, in radio order.
    pub bss: Vec<ConfirmedBss>,
}

/// The platform the agent runs on.
pub trait Platform {
    /// The radios of this device, one node each.
    fn radios(&self) -> Vec<RadioSpec>;

    /// Opens the channel on which the platform reports confirmations to the actor.
    ///
    /// A later call replaces the channel opened by an earlier one.
    fn confirmations(&mut self) -> Receiver<Confirmation>;

    /// Applies the BSSs of one radio, one per accepted M2 (a Tear Down M2 gives one
    /// entry with `multi_ap.tear_down` set). The confirmation comes later on the channel.
    fn apply_radio_config(&mut self, radio: MacAddr, bss: &[BssConfig]);
}

/// The BSSs a fake radio runs after its configuration was applied.
#[derive(Debug)]
struct AppliedRadio {
    radio: MacAddr,
    bss: Vec<ConfirmedBss>,
}

/// A platform whose radios are fixed at start, from `--radio` arguments.
///
/// It confirms once every radio has a configuration, and again after each later
/// configuration of any radio (0013 D4). A BSS gets a fake BSSID derived from its radio.
#[derive(Debug)]
pub struct FakePlatform {
    radios: Vec<RadioSpec>,
    applied: Vec<AppliedRadio>,
    confirmed: Option<Sender<Confirmation>>,
}

impl FakePlatform {
    /// A platform with exactly these radios.
    #[must_use]
    pub const fn new(radios: Vec<RadioSpec>) -> Self {
        Self {
            radios,
            applied: Vec::new(),
            confirmed: None,
        }
    }

    fn band_of(&self, radio: MacAddr) -> Option<FreqBand> {
        self.radios
            .iter()
            .find(|spec| spec.ruid == radio)
            .map(|spec| spec.band)
    }

    fn all_radios_configured(&self) -> bool {
        self.radios
            .iter()
            .all(|spec| self.applied.iter().any(|done| done.radio == spec.ruid))
    }

    fn confirm(&self) {
        let Some(sender) = &self.confirmed else {
            tracing::warn!("no confirmation channel is open, the confirmation is lost");
            return;
        };
        let mut bss = Vec::new();
        for spec in &self.radios {
            if let Some(done) = self.applied.iter().find(|done| done.radio == spec.ruid) {
                bss.extend(done.bss.iter().map(copy_confirmed));
            }
        }
        if sender.try_send(Confirmation { bss }).is_err() {
            tracing::warn!("the confirmation queue is full or closed, the confirmation is lost");
        }
    }
}

/// Builds an equal value field by field, since `Clone` is not used in this workspace.
fn copy_confirmed(bss: &ConfirmedBss) -> ConfirmedBss {
    ConfirmedBss {
        radio: bss.radio,
        band: bss.band,
        bssid: bss.bssid,
        ssid: Vec::from(bss.ssid.as_slice()),
        haul_type: bss.haul_type,
    }
}

/// A fake BSSID: the radio's MAC with the high nibble of its first octet set to the
/// BSS number, so BSSs of one radio differ and keep the locally administered bit.
fn fake_bssid(radio: MacAddr, index: usize) -> MacAddr {
    let mut octets = radio.octets();
    let number = u8::try_from(index + 1).unwrap_or(0x0f) & 0x0f;
    octets[0] = (octets[0] & 0x0f) | (number << 4);
    MacAddr::new(octets)
}

impl Platform for FakePlatform {
    fn radios(&self) -> Vec<RadioSpec> {
        Vec::from(self.radios.as_slice())
    }

    fn confirmations(&mut self) -> Receiver<Confirmation> {
        let (sender, receiver) = mpsc::channel(CONFIRMATION_QUEUE);
        self.confirmed = Some(sender);
        receiver
    }

    fn apply_radio_config(&mut self, radio: MacAddr, bss: &[BssConfig]) {
        let Some(band) = self.band_of(radio) else {
            tracing::warn!(%radio, "configuration for a radio the platform does not have");
            return;
        };
        let mut confirmed = Vec::new();
        for (index, config) in bss.iter().enumerate() {
            if config.multi_ap.tear_down {
                continue;
            }
            confirmed.push(ConfirmedBss {
                radio,
                band,
                bssid: fake_bssid(radio, index),
                ssid: Vec::from(config.ssid.as_slice()),
                haul_type: u16::from(config.haul_type),
            });
        }
        tracing::info!(%radio, bss = confirmed.len(), "platform applied a radio configuration");
        let entry = AppliedRadio {
            radio,
            bss: confirmed,
        };
        match self.applied.iter_mut().find(|done| done.radio == radio) {
            Some(done) => *done = entry,
            None => self.applied.push(entry),
        }
        if self.all_radios_configured() {
            self.confirm();
        }
    }
}

#[cfg(test)]
mod tests {
    use em_cmd::RadioSpec;
    use em_core::wsc::BssConfig;
    use em_crypto::SecretBytes;
    use em_msg::wsc::MultiApExtension;
    use em_types::{FreqBand, MacAddr};

    use super::{FakePlatform, Platform, fake_bssid};

    const RADIO_24: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 1]);
    const RADIO_5: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 2]);

    fn platform() -> FakePlatform {
        FakePlatform::new(vec![
            RadioSpec {
                ruid: RADIO_24,
                band: FreqBand::BAND_2_4_GHZ,
            },
            RadioSpec {
                ruid: RADIO_5,
                band: FreqBand::BAND_5_GHZ,
            },
        ])
    }

    fn bss(radio: MacAddr, ssid: &[u8], haul_type: u8, tear_down: bool) -> BssConfig {
        BssConfig {
            ssid: Vec::from(ssid),
            authentication_type: 0x0020,
            network_key: SecretBytes::from_slice(b"key"),
            multi_ap: MultiApExtension {
                tear_down,
                ..MultiApExtension::default()
            },
            mac: radio,
            haul_type,
        }
    }

    #[test]
    fn it_confirms_once_every_radio_has_a_configuration() {
        let mut platform = platform();
        let mut confirmations = platform.confirmations();
        platform.apply_radio_config(
            RADIO_24,
            &[
                bss(RADIO_24, b"front", 0, false),
                bss(RADIO_24, b"back", 1, false),
            ],
        );
        assert!(confirmations.try_recv().is_err());

        platform.apply_radio_config(RADIO_5, &[bss(RADIO_5, b"front", 0, false)]);
        let confirmation = confirmations.try_recv().unwrap();
        assert!(confirmations.try_recv().is_err());
        let seen: Vec<(MacAddr, &[u8], u16)> = confirmation
            .bss
            .iter()
            .map(|bss| (bss.radio, bss.ssid.as_slice(), bss.haul_type))
            .collect();
        assert_eq!(
            seen,
            [
                (RADIO_24, &b"front"[..], 0),
                (RADIO_24, &b"back"[..], 1),
                (RADIO_5, &b"front"[..], 0)
            ]
        );
        assert_eq!(confirmation.bss[0].band, FreqBand::BAND_2_4_GHZ);
        assert_eq!(confirmation.bss[0].bssid, fake_bssid(RADIO_24, 0));
        assert_ne!(confirmation.bss[0].bssid, confirmation.bss[1].bssid);
    }

    #[test]
    fn a_later_configuration_confirms_again_and_a_tear_down_has_no_bss() {
        let mut platform = platform();
        let mut confirmations = platform.confirmations();
        platform.apply_radio_config(RADIO_24, &[bss(RADIO_24, b"", 0, true)]);
        platform.apply_radio_config(RADIO_5, &[bss(RADIO_5, b"front", 0, false)]);
        assert_eq!(confirmations.try_recv().unwrap().bss.len(), 1);

        platform.apply_radio_config(RADIO_24, &[bss(RADIO_24, b"new", 0, false)]);
        let second = confirmations.try_recv().unwrap();
        assert_eq!(second.bss.len(), 2);
        assert_eq!(second.bss[0].ssid, b"new");
    }

    #[test]
    fn an_unknown_radio_is_ignored() {
        let mut platform = platform();
        let mut confirmations = platform.confirmations();
        platform.apply_radio_config(MacAddr::new([9; 6]), &[]);
        platform.apply_radio_config(RADIO_24, &[]);
        platform.apply_radio_config(RADIO_5, &[]);
        assert_eq!(confirmations.try_recv().unwrap().bss.len(), 0);
    }
}
