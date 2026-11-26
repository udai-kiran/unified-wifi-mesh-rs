// SPDX-License-Identifier: Apache-2.0

//! The controller's handling of an M1, a Topology Response and the `em_config` proto
//! tick (note 0013 D3, D7).

use std::time::Instant;

use em_cmd::{Command, CommandType, EmConfig, NodeKey, NodeKind};
use em_core::wsc::{self, ControllerEffect, Identity};
use em_core::{CtrlState, Node, configuration, topology};
use em_crypto::rand::RandomSource;
use em_msg::{CmduHeader, TlvIter, readers, validate};
use em_orch::OrchRole;
use em_types::{MacAddr, ProfileType, TlvType};

use crate::rules::RadioLink;
use crate::{CtrlCore, EM_MAX_RADIO_PER_AGENT, Frame, M2Handoff, RenewEpisode};

/// The Renews one retry episode sends, the first included (`EM_MAX_RENEW_TX_THRESH`,
/// `inc/em_base.h:115`).
const MAX_RENEW_TX: u8 = 5;

/// An M2 that was built and passed validation, with the time the build took.
struct BuiltM2 {
    output: wsc::M2Output,
    build_us: u128,
}

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
    ///
    /// The M1 is checked in full before it changes anything (0014 D18 S2-03, S2-06).
    /// The order is: parse it; the routing RUID must equal the RUID of AP Radio Basic
    /// Capabilities; the sender must have a peer model, also for a known radio; a new radio
    /// or a move must leave the sender within [`EM_MAX_RADIO_PER_AGENT`] radios; the owner's
    /// M1 in `wsc_m2_sent` is ignored (nothing changes, model order included); the M2 is
    /// built and validated. A failed check or a failed build logs one line and returns with
    /// no change to any node, link, owner, mismatch flag, chain, deadline, retry counter, age
    /// or state, and does not refresh the sender's peer model in the eviction order. Only
    /// then does the radio change, and the M2 is handed over (the test-only hand-off failure
    /// comes after the changes).
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
        let m1 = match wsc::parse_m1(cmdu) {
            Ok(m1) => m1,
            Err(error) => {
                tracing::warn!(radio = %radio, %remote, %error, "M1 rejected");
                return;
            }
        };
        // intentional-difference:m1-validated-before-state
        // Upstream creates the node from the routing RUID with `em_freq_band_unknown`
        // before the M1 is parsed (`em_ctrl.cpp:1259-1265`); a bad M1 left a node behind.
        // An M1 has no AP Radio Identifier TLV, so the routing RUID is the first TLV that
        // carries one; it must be the radio the M1 describes.
        if m1.radio() != radio {
            tracing::warn!(
                radio = %radio,
                described = %m1.radio(),
                %remote,
                "M1 refused: the routing radio differs from AP Radio Basic Capabilities"
            );
            return;
        }
        // Upstream drops an M1 from an agent without a data model, which a Search creates
        // (`em_ctrl.cpp:1253-1257`). Here it also holds for a known radio (S2-06).
        let Some(peer) = self.models.get(&remote) else {
            tracing::warn!(radio = %radio, %remote, "M1 dropped: no peer model for this agent");
            return;
        };
        let profile = peer.profile;
        let owner = self.nodes.links.get(&key).map(|link| link.agent_al);
        // intentional-difference:radio-cap-per-agent
        // Upstream defines `EM_MAX_RADIO_PER_AGENT` (`inc/em_base.h:90`) but does not
        // enforce it where it creates a radio node.
        if owner != Some(remote) && self.nodes.radios_of(remote).len() >= EM_MAX_RADIO_PER_AGENT {
            tracing::warn!(
                radio = %radio,
                %remote,
                max = EM_MAX_RADIO_PER_AGENT,
                "M1 refused: the agent has the most radios allowed"
            );
            return;
        }
        if owner == Some(remote) && self.nodes.state(key) == Some(CtrlState::WscM2Sent) {
            // A known radio in `wsc_m2_sent` keeps its state and ignores the M1. The rule
            // sits after the checks above, so a bad M1 never reaches it, and it applies to
            // the owner only: a move supersedes the owner's exchange. Nothing is built and
            // nothing changes, model order included: an ignored M1 may be one that could not
            // be answered, so only an M1 whose M2 was built refreshes the model (R3).
            tracing::info!(radio = %key.mac, "M1 ignored: the M2 was sent already");
            return;
        }
        // intentional-difference:m1-validated-before-state
        // The M2 does not read node state (`wsc::build_m2` takes the M1, the identity, the
        // network rows, the message ID counter and the random source), so it is built and
        // validated before the radio changes. An M1 that cannot be answered, such as one
        // with no BSS capacity, then changes no node, link, chain, age or retry episode.
        let Some(built) = self.build_m2_for(key, &m1) else {
            return;
        };
        // The M1 passed every check and can be answered, so the sender is alive: a model that
        // is heard from is evicted last (R3). A refused M1 does not count, or an agent at its
        // radio cap could keep its model by sending M1s; neither does an M1 whose M2 cannot
        // be built.
        self.touch_model(remote);
        if owner.is_some_and(|current| current != remote) {
            self.move_radio(key, remote, profile, m1.band(), now);
        } else if owner.is_none() {
            self.create_radio(key, remote, profile, m1.band(), now);
        }
        // intentional-difference:mismatch-cleared-on-m1
        // Upstream clears a mismatch only when the Topology Response is good; a parsed,
        // accepted M1 from any radio of the agent is a fresh start for all of them (S2-08).
        for agent_radio in self.nodes.radios_of(remote) {
            if let Some(link) = self.nodes.links.get_mut(&agent_radio) {
                link.ssid_mismatch = false;
            }
        }
        if let Some(node) = self.nodes.table.get_mut(key) {
            node.state = CtrlState::WscM1Pending;
        }
        self.reset_agent_em_config_age(remote, now);
        self.answer_m1(key, remote, built, now);
    }

    /// Gives every `em_config` stage of the agent's radios a new start time.
    ///
    /// intentional-difference:em-config-age-per-agent. Upstream keeps one start time per
    /// command type and resets it for the whole type (`em_orch.cpp:72-88,129-138,222`), so
    /// one agent's progress keeps every other agent's 40 s from running out. Here an accepted
    /// M1 restarts the age of this agent's stages only. The M2 built for it is handed over
    /// in the same call at the same `now`, so it needs no second reset.
    fn reset_agent_em_config_age(&mut self, agent_al: MacAddr, now: Instant) {
        let radios = self.nodes.radios_of(agent_al);
        self.orch
            .reset_stage_start(CommandType::EmConfig, &radios, now);
    }

    /// Creates the radio node from the parsed M1 (`em_ctrl.cpp:1259-1269`).
    fn create_radio(
        &mut self,
        key: NodeKey,
        agent_al: MacAddr,
        profile: u8,
        band: em_types::FreqBand,
        now: Instant,
    ) {
        let band = Some(band);
        self.nodes
            .table
            .insert(Node::new(key, CtrlState::WscM1Pending, profile, band));
        self.nodes.links.insert(
            key,
            RadioLink {
                agent_al,
                ssid_mismatch: false,
            },
        );
        self.restart_proto_deadline(key, now);
        tracing::info!(radio = %key.mac, agent = %agent_al, ?band, "radio node created");
    }

    /// Hands a known radio to the agent that sent the M1, in one step (S2-06).
    ///
    /// intentional-difference:radio-owner-move. Upstream sends M2 to the M1's sender but
    /// Topology Query and Renew to the first owner's data model
    /// (`em_configuration.cpp:483,959`); here the owner, profile and band follow the M1, the
    /// old owner's `em_config` for the radio is removed, and the per-radio state restarts.
    /// Any agent with a peer model can take a radio over until Stage 3 binds a radio to its
    /// agent (accepted risk R4).
    fn move_radio(
        &mut self,
        key: NodeKey,
        new_owner: MacAddr,
        profile: u8,
        band: em_types::FreqBand,
        now: Instant,
    ) {
        let old_owner = self.nodes.links.get(&key).map(|link| link.agent_al);
        self.remove_em_config_chain(key, now);
        if let Some(node) = self.nodes.table.get_mut(key) {
            node.profile = profile;
            node.band = Some(band);
            node.state = CtrlState::WscM1Pending;
        }
        self.nodes.links.insert(
            key,
            RadioLink {
                agent_al: new_owner,
                ssid_mismatch: false,
            },
        );
        self.reset_radio_per_radio_state(key);
        self.restart_proto_deadline(key, now);
        tracing::info!(
            radio = %key.mac,
            from = ?old_owner,
            to = %new_owner,
            "radio moved to another agent"
        );
    }

    /// Builds M2 and validates it (profile 3), without touching any node. `None` after one
    /// warn line when it cannot be built or fails validation.
    fn build_m2_for(&mut self, key: NodeKey, m1: &wsc::M1<'_>) -> Option<BuiltM2> {
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
                return None;
            }
        };
        if let Err(error) = validate(&output.cmdu, ProfileType::MULTI_AP_PROFILE_3) {
            tracing::warn!(radio = %key.mac, %error, "M2 failed validation, not sent");
            return None;
        }
        Some(BuiltM2 { output, build_us })
    }

    /// Hands the built M2 over; then, in upstream's order, removes the radio's previous
    /// `em_config`, enters `wsc_m2_sent` and submits `em_config`
    /// (`em_configuration.cpp:6262-6275`). A failed hand-off applies none of them; the
    /// changes `handle_wsc` made before it stay.
    fn answer_m1(&mut self, key: NodeKey, remote: MacAddr, built: BuiltM2, now: Instant) {
        let BuiltM2 { output, build_us } = built;
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
            ControllerEffect::RemoveChainForNode(radio) => self.remove_em_config_chain(radio, now),
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

    /// Removes the `em_config` chain of a radio and sets the radio `unconfigured` when one ran.
    pub(crate) fn remove_em_config_chain(&mut self, radio: NodeKey, now: Instant) {
        if let Some(state_effect) = self
            .orch
            .remove_chain_for_node(radio, CtrlState::Unconfigured)
        {
            let mut out = Vec::new();
            self.rules.apply(&mut self.nodes, state_effect, &mut out);
            self.drain_orch_out(out, now);
        }
    }

    pub(crate) fn submit_em_config(&mut self, config: EmConfig, now: Instant) {
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

    /// The proto tick of a radio node. A `misconfigured` radio gets the next Renew of its
    /// retry episode (see [`Self::start_renew_episode`]). Otherwise it is the Topology Query of
    /// `em_state_ctrl_topo_sync_pending` (`em_configuration.cpp:6869-6914`), only while
    /// `em_config` executes on it.
    pub(crate) fn dispatch_proto_tick(&mut self, key: NodeKey, now: Instant) {
        self.restart_proto_deadline(key, now);
        if key.kind == NodeKind::Radio && self.nodes.state(key) == Some(CtrlState::Misconfigured) {
            self.retry_renew(key);
            return;
        }
        if key.kind != NodeKind::Radio
            || self.nodes.state(key) != Some(CtrlState::TopoSyncPending)
            || !self.em_config_in_progress(key)
        {
            return;
        }
        self.topology_query_tick(key);
    }

    /// Starts a Renew retry episode for a radio whose `em_config` was cancelled: the count
    /// restarts at 0 and the first Renew goes out now.
    ///
    /// Upstream retries `em_configuration.cpp:850-938,6865-6866` up to
    /// `EM_MAX_RENEW_TX_THRESH` (`inc/em_base.h:115`).
    ///
    /// intentional-difference:renew-retry-counter-per-episode. Upstream carries the count
    /// from one episode to the next and clears it only at the threshold and in the
    /// `cfg_renew` cancel hook (`em_configuration.cpp:935-938`, `em_orch_ctrl.cpp:171-175,
    /// 418-420`), so a later cancel could start with a spent budget (R1).
    ///
    /// intentional-difference:renew-retry-without-cfg-renew-command. Upstream sends the
    /// retries only while a `cfg_renew` command is in progress (`em.cpp:546-559`), which a
    /// 10 s type-wide cancel ends (`em_orch_ctrl.cpp:157-163`). The controller has no such
    /// command; the idle proto tick of a misconfigured radio sends them (R2).
    ///
    /// A cancel can ask twice for the same radio in one instant: `transient` asks for every
    /// radio of the agent on the `topo_sync_pending` path, and `pre_process_cancel` asks again
    /// for the radios whose stage was active. A radio whose episode already started at `now`
    /// is left alone, so the first Renew goes out once. A cancel at a later instant starts a
    /// fresh episode (R1).
    pub(crate) fn start_renew_episode(&mut self, key: NodeKey, now: Instant) {
        let started_now = self
            .renew_counts
            .get(&key)
            .is_some_and(|episode| episode.started == now);
        if started_now {
            return;
        }
        self.send_renew(key);
        // The retries are one idle second apart, the first one a second after this Renew.
        self.restart_proto_deadline(key, now);
        self.renew_counts.insert(
            key,
            RenewEpisode {
                sent: 1,
                started: now,
            },
        );
    }

    /// One retry of the episode, on the idle proto tick of a misconfigured radio.
    fn retry_renew(&mut self, key: NodeKey) {
        let Some(episode) = self.renew_counts.get(&key).copied() else {
            return;
        };
        if episode.sent == 0 || episode.sent >= MAX_RENEW_TX {
            return;
        }
        self.send_renew(key);
        self.renew_counts.insert(
            key,
            RenewEpisode {
                sent: episode.sent + 1,
                ..episode
            },
        );
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
        self.touch_model(remote);
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
