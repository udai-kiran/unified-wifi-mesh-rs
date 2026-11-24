// SPDX-License-Identifier: Apache-2.0

//! The controller's `NetworkSSIDList`, loaded from the reset file (0013 D2).

use em_core::wsc::{self, NetworkSsid};
use em_dm::NetworkSsidInfo;
use em_types::FreqBand;

/// One row with the fields the M2 builder needs, converted once at start.
#[derive(Debug)]
struct Row {
    info: Box<NetworkSsidInfo>,
    bands: Vec<FreqBand>,
    haul_types: Vec<u8>,
    authentication_type: u16,
}

/// The rows in `NetworkSSIDList` order.
#[derive(Debug, Default)]
pub(crate) struct Networks {
    rows: Vec<Row>,
}

impl Networks {
    pub(crate) fn new(infos: impl IntoIterator<Item = Box<NetworkSsidInfo>>) -> Self {
        let rows = infos
            .into_iter()
            .map(|info| {
                let bands = info
                    .band
                    .as_slice()
                    .iter()
                    .filter_map(|band| wsc::network_band(band.as_bytes()))
                    .collect();
                let haul_types = info
                    .haul_type
                    .as_slice()
                    .iter()
                    .filter_map(|haul| u8::try_from(u16::from(*haul)).ok())
                    .collect();
                let authentication_type = wsc::authentication_type(info.auth_type.as_bytes());
                Row {
                    info,
                    bands,
                    haul_types,
                    authentication_type,
                }
            })
            .collect();
        Self { rows }
    }

    /// The rows as the M2 builder takes them. Passphrases are borrowed, not copied.
    pub(crate) fn views(&self) -> Vec<NetworkSsid<'_>> {
        self.rows
            .iter()
            .map(|row| NetworkSsid {
                enabled: row.info.enable,
                ssid: row.info.ssid.as_bytes(),
                network_key: row.info.pass_phrase.expose().as_bytes(),
                bands: &row.bands,
                haul_types: &row.haul_types,
                authentication_type: row.authentication_type,
            })
            .collect()
    }

    /// Every SSID of the list, the set a Topology Response is checked against
    /// (`dm->is_ssid_match`, `em_configuration.cpp:2270-2286`).
    pub(crate) fn ssids(&self) -> Vec<&[u8]> {
        self.rows
            .iter()
            .map(|row| row.info.ssid.as_bytes())
            .collect()
    }
}
