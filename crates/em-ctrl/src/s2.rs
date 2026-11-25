// SPDX-License-Identifier: Apache-2.0

//! The controller's handling of an M1, a Topology Response and the `em_config` proto
//! tick (note 0013 D3, D7).

use std::time::Instant;

use em_cmd::{Command, EmConfig, NodeKey, NodeKind};
use em_core::wsc::{self, ControllerEffect, Identity};
use em_core::{CtrlState, Node, configuration, topology};
use em_crypto::rand::RandomSource;
use em_msg::{CmduHeader, TlvIter, readers, validate};
use em_orch::OrchRole;
use em_types::{MacAddr, ProfileType, TlvType};

use crate::rules::RadioLink;
use crate::{CtrlCore, Frame, M2Handoff};

impl<R: RandomSource> CtrlCore<R> {
    /// The registrar identity in M2. The first 12 bytes of the UUID are the controller's AL
    /// MAC repeated twice; the last 4 are zero.
    fn identity(&self) -> Identity<'static> {
        let mut uuid = [0_u8; 16];
        if let Some(al_mac) = self.al_mac {
            uuid[..6].copy_from_slice(&al_mac.octets());
            uuid[6..12].copy_from_slice(&al_mac.octets());
        }
        Identity {
            uuid,
            manufacturer: b"em-ctrl",
            model: b"controller",
            serial: b"0001",
            primary_device_type: [0, 6, 0, 0x50, 0xf2, 4, 0, 1],
        }
    }

    /// An AP-Autoconfiguration WSC message: routed by the radio ID it carries
    /// (`em_ctrl.cpp:1240-1270`), then handled as an M1 when the radio waits for one
    /// (`em_configuration.cpp:6553-6556`).
    pub(crate) fn handle_wsc(&mut self, remote: MacAddr, cmdu: &[u8], now: Instant) {
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
        let m1 = wsc::parse_m1(cmdu);
        if !self.route_to_radio(key, remote, m1.as_ref().ok().map(wsc::M1::band), now) {
            return;
        }
        if self.nodes.state(key) != Some(CtrlState::WscM1Pending) {
            return;
        }
        match m1 {
            Ok(m1) => self.answer_m1(key, remote, &m1, now),
            Err(error) => tracing::warn!(radio = %radio, %remote, %error, "M1 rejected"),
        }
    }

    /// Resets an existing radio node to `ctrl_wsc_m1_pending` unless it is in
    /// `wsc_m2_sent`, or creates it from the M1 (`em_ctrl.cpp:1245-1269`). A radio in
    /// `wsc_m2_sent` keeps its state, so the caller ignores the M1. Returns false when the
    /// M1 is dropped because the agent has no peer model.
    fn route_to_radio(
        &mut self,
        key: NodeKey,
        remote: MacAddr,
        band: Option<em_types::FreqBand>,
        now: Instant,
    ) -> bool {
        if let Some(state) = self.nodes.state(key) {
            if state == CtrlState::WscM2Sent {
                tracing::info!(radio = %key.mac, "M1 ignored: the M2 was sent already");
            } else if let Some(node) = self.nodes.table.get_mut(key) {
                node.state = CtrlState::WscM1Pending;
            }
            return true;
        }
        // Upstream drops an M1 from an agent without a data model, which a Search creates
        // (`em_ctrl.cpp:1253-1257`).
        let Some(peer) = self.models.get(&remote) else {
            tracing::warn!(radio = %key.mac, %remote, "M1 dropped: no peer model for this agent");
            return false;
        };
        let profile = peer.profile;
        self.nodes
            .table
            .insert(Node::new(key, CtrlState::WscM1Pending, profile, band));
        self.nodes.links.insert(
            key,
            RadioLink {
                agent_al: remote,
                ssid_mismatch: false,
            },
        );
        self.restart_proto_deadline(key, now);
        tracing::info!(radio = %key.mac, agent = %remote, ?band, "radio node created");
        true
    }

    /// Builds M2 and hands it over; then, in upstream's order, removes the radio's
    /// previous `em_config`, enters `wsc_m2_sent` and submits `em_config`
    /// (`em_configuration.cpp:6262-6275`). A failed hand-off applies none of them.
    fn answer_m1(&mut self, key: NodeKey, remote: MacAddr, m1: &wsc::M1<'_>, now: Instant) {
        let identity = self.identity();
        let rows = self.networks.views();
        // The build runs on the loop task: its time is the loop's stall for the DH key
        // generation, the shared secret, the KDF and the AES encryptions (0013 D9).
        let started = (self.measure_now)();
        let built = wsc::build_m2(m1, &identity, &rows, &mut self.msg_ids, &mut self.random);
        let build_us = (self.measure_now)()
            .saturating_duration_since(started)
            .as_micros();
        let output = match built {
            Ok(output) => output,
            Err(error) => {
                tracing::warn!(radio = %key.mac, %error, "M2 could not be built");
                return;
            }
        };
        if let Err(error) = validate(&output.cmdu, ProfileType::MULTI_AP_PROFILE_3) {
            tracing::warn!(radio = %key.mac, %error, "M2 failed validation, not sent");
            return;
        }
        let mid = CmduHeader::parse(&output.cmdu).map_or(0, |(header, _)| header.msg_id);
        let m2_count = count_wsc_tlvs(&output.cmdu);
        let effects = output.post_handoff_effects(remote);
        if !self.hand_off_m2(remote, output.cmdu) {
            tracing::warn!(radio = %key.mac, "M2 hand-off failed, no state change");
            return;
        }
        tracing::info!(radio = %key.mac, mid, m2s = m2_count, build_us, "M2 sent");
        for effect in effects {
            self.apply_controller_effect(effect, now);
        }
        self.run_event_pass(now);
    }

    /// Hands the M2 to the transport, or, with the test switch, does nothing and reports
    /// success (0013 D10).
    fn hand_off_m2(&mut self, remote: MacAddr, cmdu: Vec<u8>) -> bool {
        match self.m2_handoff {
            M2Handoff::Deliver => {
                self.outbox.push(Frame { remote, cmdu });
                true
            }
            M2Handoff::DropNext => {
                self.m2_handoff = M2Handoff::Deliver;
                tracing::warn!("M2 dropped by test switch");
                true
            }
            #[cfg(test)]
            M2Handoff::Fail => false,
        }
    }

    fn apply_controller_effect(&mut self, effect: ControllerEffect, now: Instant) {
        match effect {
            ControllerEffect::RemoveChainForNode(radio) => {
                if let Some(state_effect) = self
                    .orch
                    .remove_chain_for_node(radio, CtrlState::Unconfigured)
                {
                    let mut out = Vec::new();
                    self.rules.apply(&mut self.nodes, state_effect, &mut out);
                    self.drain_orch_out(out, now);
                }
            }
            ControllerEffect::SetState(radio, state) => {
                if let Some(node) = self.nodes.table.get_mut(radio) {
                    node.state = state;
                }
            }
            ControllerEffect::SubmitEmConfig { radio, agent_al } => {
                self.submit_em_config(EmConfig { radio, agent_al }, now);
            }
        }
    }

    fn submit_em_config(&mut self, config: EmConfig, now: Instant) {
        let (chain, effects) = self.orch.submit(
            &mut self.rules,
            &mut self.nodes,
            Command::EmConfig(config),
            now,
        );
        let mut out = Vec::new();
        for effect in effects {
            self.rules.apply(&mut self.nodes, effect, &mut out);
        }
        self.drain_orch_out(out, now);
        if !self.orch.admit(&mut self.rules, &mut self.nodes, chain) {
            tracing::warn!(radio = %config.radio.mac, "em_config has no candidates: not ready");
        }
    }

    /// The proto tick of a radio node: the Topology Query of `em_state_ctrl_topo_sync_pending`
    /// (`em_configuration.cpp:6869-6914`), only while `em_config` executes on it.
    pub(crate) fn dispatch_proto_tick(&mut self, key: NodeKey, now: Instant) {
        self.restart_proto_deadline(key, now);
        if key.kind != NodeKind::Radio
            || self.nodes.state(key) != Some(CtrlState::TopoSyncPending)
            || !self.em_config_in_progress(key)
        {
            return;
        }
        self.topology_query_tick(key);
    }

    fn topology_query_tick(&mut self, key: NodeKey) {
        let Some(link) = self.nodes.links.get(&key).copied() else {
            return;
        };
        let radios = self.nodes.radios_of(link.agent_al);
        // The mismatch path (a Query every 10 s, then misconfigured after 120 s) is S3.
        if radios
            .iter()
            .any(|radio| self.nodes.links.get(radio).is_some_and(|l| l.ssid_mismatch))
        {
            return;
        }
        // The Query is per device: every radio of the agent must wait for it, and only the
        // first radio sends it (`em_configuration.cpp:6881-6910`).
        if radios
            .iter()
            .any(|radio| self.nodes.state(*radio) != Some(CtrlState::TopoSyncPending))
        {
            tracing::debug!(radio = %key.mac, "a radio of this agent is not in topo_sync_pending");
            return;
        }
        if radios.first() != Some(&key) {
            return;
        }
        let Some(al_mac) = self.al_mac else {
            return;
        };
        let al_key = NodeKey {
            mac: al_mac,
            kind: NodeKind::Al,
        };
        if let Some(node) = self.nodes.table.get_mut(al_key) {
            node.state = CtrlState::TopoSyncPending;
        }
        match topology::build_query(&mut self.msg_ids) {
            Ok(cmdu) => {
                tracing::info!(agent = %link.agent_al, radio = %key.mac, "Topology Query sent");
                self.outbox.push(Frame {
                    remote: link.agent_al,
                    cmdu,
                });
            }
            Err(error) => tracing::warn!(%error, "Topology Query could not be built"),
        }
    }

    /// A Topology Response: accepted while any radio of the agent waits for it
    /// (`em_configuration.cpp:6597-6632`). Success moves every radio of the agent to
    /// `topo_synchronized`; an unknown SSID flags a mismatch.
    pub(crate) fn handle_topology_response(&mut self, remote: MacAddr, cmdu: &[u8], now: Instant) {
        let radios = self.nodes.radios_of(remote);
        let waiting = radios
            .iter()
            .any(|radio| self.nodes.state(*radio) == Some(CtrlState::TopoSyncPending));
        if !waiting {
            tracing::debug!(%remote, "Topology Response ignored: no radio of this agent waits");
            return;
        }
        let ssids = self.networks.ssids();
        let result = topology::parse_response(cmdu, &ssids, 3).map(|_| ());
        match result {
            Ok(()) => {
                tracing::info!(agent = %remote, "Topology Response handled");
                for radio in radios {
                    if let Some(node) = self.nodes.table.get_mut(radio) {
                        node.state = CtrlState::TopoSynchronized;
                    }
                    if let Some(link) = self.nodes.links.get_mut(&radio) {
                        link.ssid_mismatch = false;
                    }
                    tracing::info!(radio = %radio.mac, "topo_synchronized");
                }
                self.run_event_pass(now);
            }
            Err(topology::Error::SsidMismatch) => {
                // Upstream flags the radio that handled the response; the first radio
                // sent the Query.
                if let Some(link) = radios
                    .first()
                    .and_then(|radio| self.nodes.links.get_mut(radio))
                {
                    link.ssid_mismatch = true;
                }
                tracing::warn!(agent = %remote, "ssid mismatch");
            }
            Err(error) => {
                tracing::warn!(agent = %remote, %error, "Topology Response rejected");
            }
        }
    }

    /// Sends one AP-Autoconfiguration Renew for a radio (`em_configuration.cpp:850-926`).
    pub(crate) fn send_renew(&mut self, key: NodeKey) {
        let (Some(al_mac), Some(link)) = (self.al_mac, self.nodes.links.get(&key).copied()) else {
            return;
        };
        let Some(band) = self.nodes.table.get(key).and_then(|node| node.band) else {
            tracing::warn!(radio = %key.mac, "renew not sent: the radio has no band");
            return;
        };
        let cmdu = match configuration::build_renew(al_mac, band, self.msg_ids.next_id()) {
            Ok(cmdu) => cmdu,
            Err(error) => {
                tracing::warn!(radio = %key.mac, %error, "renew could not be built");
                return;
            }
        };
        if let Err(error) = validate(&cmdu, ProfileType::MULTI_AP_PROFILE_3) {
            tracing::warn!(radio = %key.mac, %error, "renew failed validation, not sent");
            return;
        }
        tracing::info!(radio = %key.mac, agent = %link.agent_al, ?band, "renew sent");
        self.outbox.push(Frame {
            remote: link.agent_al,
            cmdu,
        });
    }
}

/// The number of WSC TLVs, one per M2.
fn count_wsc_tlvs(cmdu: &[u8]) -> usize {
    let Ok((_, area)) = CmduHeader::parse(cmdu) else {
        return 0;
    };
    TlvIter::new(area)
        .filter_map(Result::ok)
        .filter(|tlv| tlv.tlv_type == TlvType::WSC)
        .count()
}
