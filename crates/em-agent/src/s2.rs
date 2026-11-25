// SPDX-License-Identifier: Apache-2.0

//! The agent's handling of an M2, a Topology Query and a Renew (note 0013 D4).

use std::time::Instant;

use em_cmd::{CfgRenew, Command, CommandType, NodeKey, NodeKind};
use em_core::topology::{self, Bss, Radio};
use em_core::{AgentState, Outputs, configuration, wsc};
use em_msg::{CmduHeader, readers};
use em_types::MacAddr;

use crate::{AgentCore, ApplyRadioConfig, Frame};

/// The states in which upstream refuses a renew (`em_agent.cpp:2438-2447`).
fn renew_is_refused_in(state: AgentState) -> bool {
    matches!(
        state,
        AgentState::AutoconfigRenewPending | AgentState::WscM2Pending | AgentState::WscM2Processed
    )
}

impl AgentCore {
    /// An AP-Autoconfiguration WSC message: an M2 for the radio its AP Radio Identifier
    /// names (`em_agent.cpp:2423-2433`, `em_configuration.cpp:6553-6556`).
    pub(crate) fn handle_wsc(
        &mut self,
        remote: MacAddr,
        cmdu: &[u8],
        now: Instant,
        frames: &mut Vec<Frame>,
    ) {
        let radio = match CmduHeader::parse(cmdu) {
            Ok((_, area)) => readers::radio_id(area).ok().flatten(),
            Err(_) => None,
        };
        let Some(radio) = radio else {
            tracing::debug!(%remote, "WSC message without a radio identifier ignored");
            return;
        };
        let key = NodeKey {
            mac: radio,
            kind: NodeKind::Radio,
        };
        let Some(state) = self.nodes.get(key).map(|node| node.state) else {
            tracing::debug!(%radio, "WSC message for an unknown radio ignored");
            return;
        };
        let mid = CmduHeader::parse(cmdu).map_or(0, |(header, _)| header.msg_id);
        let session = self.rules.wsc.session_mut(key);
        // The check runs on the loop task: its time is the loop's stall for the shared
        // secret, the KDF and the AES decryption (0013 D9).
        let check_started = Instant::now();
        let accepted = wsc::accept_m2(session, state, remote, cmdu);
        let check_us = check_started.elapsed().as_micros();
        let configs = match accepted {
            Ok(configs) => configs,
            Err(error) => {
                // A rejected M2 keeps the session; the 10 s wait still bounds the radio
                // (0013 D4).
                tracing::warn!(%radio, mid, reason = %error, "M2 rejected");
                return;
            }
        };
        tracing::info!(%radio, mid, bss = configs.len(), check_us, "M2 accepted");
        self.rules.wsc.clear(key);
        self.platform_requests.push(ApplyRadioConfig {
            radio,
            bss: configs,
        });
        let mut out = Outputs::new();
        self.set_state(key, AgentState::OwconfigPending, &mut out);
        self.finish_event(key, now, out, frames);
    }

    /// A Topology Query is answered for the whole device, and only when every radio is at
    /// least `onewifi_bssconfig_ind` (`em_configuration.cpp:6567-6592`).
    pub(crate) fn handle_topology_query(
        &mut self,
        remote: MacAddr,
        cmdu: &[u8],
        now: Instant,
        frames: &mut Vec<Frame>,
    ) {
        let (Some(al_mac), Some(confirmed)) = (self.al_mac, self.confirmed.as_ref()) else {
            tracing::debug!(%remote, "Topology Query ignored: the platform has not confirmed");
            return;
        };
        let keys = self.radio_keys();
        let states: Vec<AgentState> = keys
            .iter()
            .filter_map(|key| self.nodes.get(*key).map(|node| node.state))
            .collect();
        let bss_per_radio: Vec<Vec<Bss<'_>>> = keys
            .iter()
            .map(|key| {
                confirmed
                    .iter()
                    .filter(|bss| bss.radio == key.mac)
                    .map(|bss| Bss {
                        bssid: bss.bssid,
                        ssid: &bss.ssid,
                        enabled: true,
                        interface_mac: bss.bssid,
                        band: bss.band,
                        haul_type: bss.haul_type,
                        vap_mode: 0,
                    })
                    .collect()
            })
            .collect();
        let radios: Vec<Radio<'_>> = keys
            .iter()
            .zip(&bss_per_radio)
            .map(|(key, bsses)| Radio {
                ruid: key.mac,
                bsses,
            })
            .collect();
        let response = match topology::handle_query(cmdu, &states, al_mac, &radios, false) {
            Ok(Some(response)) => response,
            Ok(None) => {
                tracing::debug!(%remote, "Topology Query ignored: a radio is not ready");
                return;
            }
            Err(error) => {
                tracing::warn!(%error, %remote, "Topology Query rejected");
                return;
            }
        };
        tracing::info!(controller = %remote, "Topology Query answered");
        frames.push(Frame {
            remote,
            cmdu: response,
        });
        for key in keys {
            if let Some(node) = self.nodes.get_mut(key) {
                node.state = AgentState::TopoSynchronized;
                tracing::info!(radio = %key.mac, "topo_synchronized");
            }
        }
        self.run_event_pass(now);
    }

    /// A Renew picks one radio by band and starts `cfg_renew` for it
    /// (`em_agent.cpp:2400-2447`, `em_agent.cpp:376-387`).
    pub(crate) fn handle_renew(&mut self, cmdu: &[u8], now: Instant) {
        let route = match configuration::route_renew(cmdu) {
            Ok(route) => route,
            Err(error) => {
                tracing::warn!(%error, "Renew rejected");
                return;
            }
        };
        if self.orch.is_type_in_progress(CommandType::CfgRenew) {
            tracing::info!(band = ?route.band, "renew refused: a cfg_renew is in progress");
            return;
        }
        // Upstream skips a radio whose recorded controller is another AL MAC
        // (`em_agent.cpp:2411-2430`) and takes the first radio of the band.
        let radio = self.nodes.iter().find(|node| {
            node.key.kind == NodeKind::Radio
                && node.band == Some(route.band)
                && node
                    .controller
                    .is_none_or(|controller| controller.al_mac == route.al_mac)
        });
        let Some(radio) = radio else {
            tracing::info!(band = ?route.band, "renew ignored: no radio of this band");
            return;
        };
        let (key, state, profile) = (radio.key, radio.state, radio.profile);
        if renew_is_refused_in(state) {
            tracing::info!(radio = %key.mac, ?state, "renew refused: the radio is in this state");
            return;
        }
        if let Some(node) = self.nodes.get_mut(key) {
            if node.controller.is_none() {
                node.controller = Some(em_core::ControllerPeer {
                    al_mac: route.al_mac,
                    profile,
                });
            }
        }
        tracing::info!(radio = %key.mac, "renew accepted");
        self.submit(Command::CfgRenew(CfgRenew { radio: key }), now);
    }
}
