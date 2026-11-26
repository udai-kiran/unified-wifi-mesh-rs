// SPDX-License-Identifier: Apache-2.0

//! Tests of the controller's S2 flow in virtual time (note 0013 D3, D7, D10).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use em_cmd::{ChainId, Command, CommandType, CommandView, EmConfig, NodeKey, NodeKind};
use em_core::configuration::build_search;
use em_core::topology::{self, Bss, Radio};
use em_core::wsc::{self, Identity, M1Params, Session};
use em_core::{AgentState, CtrlState, MsgIdCounter};
use em_crypto::rand::SeededRandom;
use em_db::load_reset_str;
use em_msg::bodies::{
    ApRadioAdvancedCapabilities, ApRadioBasicCapabilities, BasicOperatingClass,
    Profile2ApCapability,
};
use em_msg::{CmduHeader, TlvIter, readers, validate};
use em_types::{FreqBand, MacAddr, MsgType, ProfileType, TlvType};

use crate::{CtrlCore, Frame, M2Handoff};

const CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
const AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
const OTHER_AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 7]);
const RADIO_24: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 1]);
const RADIO_5: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 2]);
const OTHER_RADIO: MacAddr = MacAddr::new([2, 0, 0, 0, 2, 1]);
const SECOND: Duration = Duration::from_secs(1);

/// The core's random seed in the harness (and in the replay runs that must match).
const CORE_SEED: u64 = 0x5eed_0001;
/// The test agent's random seed.
const PEER_SEED: u64 = 0x5eed_0002;
/// A different core seed, for the replay run that must differ.
const OTHER_CORE_SEED: u64 = 0x5eed_0003;

/// The core's measurement clock and the start of virtual time: one instant, read once.
fn fixed_now() -> Instant {
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

/// The shape of the Docker reset file: a WPA2 fronthaul row and a SAE backhaul row.
const RESET: &str = r#"{
  "wfa-dataelements:Reset": {
    "NetworkSSIDList": [
      {"SSID": "front", "PassPhrase": "front-pass", "Band": ["2.4", "5"], "Enable": true,
       "HaulType": ["Fronthaul"], "AuthType": "WPA2 Personal"},
      {"SSID": "back", "PassPhrase": "back-pass", "Band": ["2.4", "5"], "Enable": true,
       "HaulType": ["Backhaul"], "AuthType": "WPA3 Personal"}
    ]
  }
}"#;

fn key(mac: MacAddr) -> NodeKey {
    NodeKey {
        mac,
        kind: NodeKind::Radio,
    }
}

fn header_of(cmdu: &[u8]) -> CmduHeader {
    CmduHeader::parse(cmdu).unwrap().0
}

/// What an agent's radio holds after it sent M1.
struct AgentRadio {
    session: Session,
}

/// Drives a core with virtual time and records every frame with its offset from the start.
struct Harness {
    core: CtrlCore<SeededRandom>,
    /// The test agent's random source, for the M1 it builds.
    peer_random: SeededRandom,
    start: Instant,
    now: Instant,
    frames: Vec<(Duration, Frame)>,
    ids: MsgIdCounter,
}

impl Harness {
    fn with(drop_first_m2: bool) -> Self {
        Self::seeded(drop_first_m2, CORE_SEED, PEER_SEED)
    }

    /// A core and a test agent with the given random seeds, on the fixed clock.
    fn seeded(drop_first_m2: bool, core_seed: u64, peer_seed: u64) -> Self {
        let start = fixed_now();
        let mut core = CtrlCore::with_sources(
            load_reset_str(RESET).unwrap(),
            drop_first_m2,
            SeededRandom::new(core_seed),
            fixed_now,
        );
        core.registered(CONTROLLER).unwrap();
        Self {
            core,
            peer_random: SeededRandom::new(peer_seed),
            start,
            now: start,
            frames: Vec::new(),
            ids: MsgIdCounter::new(),
        }
    }

    fn new() -> Self {
        Self::with(false)
    }

    fn offset(&self) -> Duration {
        self.now - self.start
    }

    fn send(&mut self, remote: MacAddr, cmdu: &[u8]) {
        let offset = self.offset();
        for frame in self.core.received(remote, cmdu, self.now) {
            self.frames.push((offset, frame));
        }
    }

    /// Fires every deadline up to `span` from now, in order.
    fn run_for(&mut self, span: Duration) {
        let end = self.now + span;
        while let Some(due) = self.core.next_deadline() {
            if due > end {
                break;
            }
            self.now = self.now.max(due);
            let offset = self.offset();
            for frame in self.core.tick(self.now) {
                self.frames.push((offset, frame));
            }
        }
        self.now = self.now.max(end);
    }

    /// A Search from the agent, so the controller has a peer model for it.
    fn search(&mut self, agent: MacAddr) {
        let search = build_search(agent, FreqBand::BAND_5_GHZ, 3, 1).unwrap();
        self.send(agent, &search);
    }

    /// An M1 from one of the agent's radios, sent to the controller.
    fn m1(&mut self, agent: MacAddr, radio: MacAddr, band: FreqBand) -> AgentRadio {
        let (cmdu, session) = self.build_m1(agent, radio, band);
        self.send(agent, &cmdu);
        AgentRadio { session }
    }

    /// An M1 as the agent builds it, not yet sent.
    fn build_m1(&mut self, agent: MacAddr, radio: MacAddr, band: FreqBand) -> (Vec<u8>, Session) {
        let mut session = Session::new();
        let params = m1_params(agent, radio, band);
        let cmdu =
            wsc::build_m1(&params, &mut self.ids, &mut self.peer_random, &mut session).unwrap();
        (cmdu, session)
    }

    fn frames_of(&self, msg_type: MsgType) -> Vec<(Duration, &Frame)> {
        self.frames
            .iter()
            .filter(|(_, frame)| header_of(&frame.cmdu).msg_type == msg_type)
            .map(|(offset, frame)| (*offset, frame))
            .collect()
    }

    /// The Renews sent to `agent` for the radio of this band.
    fn renews_for(&self, agent: MacAddr, band: FreqBand) -> usize {
        self.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW)
            .iter()
            .filter(|(_, frame)| {
                let (_, area) = CmduHeader::parse(&frame.cmdu).unwrap();
                frame.remote == agent && readers::freq_band(area).unwrap() == Some(band)
            })
            .count()
    }

    fn state(&self, radio: MacAddr) -> CtrlState {
        self.core.nodes.state(key(radio)).unwrap()
    }

    /// The agent the radio's active `em_config` chain was submitted for.
    fn chain_agent(&self, radio: MacAddr) -> Option<MacAddr> {
        let active = self.core.orch.active(key(radio))?;
        match self.core.orch.command(active.chain)? {
            Command::EmConfig(config) => Some(config.agent_al),
            _ => None,
        }
    }

    /// Whether the orchestrator still holds an `em_config` command for the radio, in any
    /// stage: pending, active or just finished.
    fn has_em_config_command(&self, radio: MacAddr) -> bool {
        (0..64).any(|id| {
            matches!(
                self.core.orch.command(ChainId(id)),
                Some(Command::EmConfig(config)) if config.radio == key(radio)
            )
        })
    }

    fn owner(&self, radio: MacAddr) -> Option<MacAddr> {
        self.core
            .nodes
            .links
            .get(&key(radio))
            .map(|link| link.agent_al)
    }

    fn mismatch(&self, radio: MacAddr) -> bool {
        self.core.nodes.links[&key(radio)].ssid_mismatch
    }

    fn set_mismatch(&mut self, radio: MacAddr) {
        self.core
            .nodes
            .links
            .get_mut(&key(radio))
            .unwrap()
            .ssid_mismatch = true;
    }

    fn proto_deadline(&self, radio: MacAddr) -> Option<Instant> {
        self.core
            .proto_deadlines
            .iter()
            .find(|deadline| deadline.key == key(radio))
            .map(|deadline| deadline.due)
    }

    /// Asserts that nothing is left of the radio: node, link and proto deadline.
    fn assert_radio_absent(&self, radio: MacAddr) {
        assert!(self.core.nodes().get(key(radio)).is_none());
        assert!(!self.core.nodes.links.contains_key(&key(radio)));
        assert!(self.proto_deadline(radio).is_none());
    }

    fn al_state(&self) -> CtrlState {
        self.core.nodes.table.iter().next().unwrap().state
    }

    /// A Topology Response from the agent for the given BSSs, one per radio.
    fn response(&mut self, agent: MacAddr, ssid: &[u8], radios: &[MacAddr]) {
        let bsses: Vec<[Bss<'_>; 1]> = radios
            .iter()
            .enumerate()
            .map(|(index, radio)| {
                let mut bssid = radio.octets();
                bssid[0] = 0x12 + u8::try_from(index).unwrap();
                [Bss {
                    bssid: MacAddr::new(bssid),
                    ssid,
                    enabled: true,
                    interface_mac: MacAddr::new(bssid),
                    band: FreqBand::BAND_5_GHZ,
                    haul_type: 0,
                    vap_mode: 0,
                }]
            })
            .collect();
        let view: Vec<Radio<'_>> = radios
            .iter()
            .zip(&bsses)
            .map(|(ruid, bss)| Radio {
                ruid: *ruid,
                bsses: bss,
            })
            .collect();
        let cmdu = topology::build_response(agent, &view, 5, false).unwrap();
        self.send(agent, &cmdu);
    }
}

fn m1_params(agent: MacAddr, radio: MacAddr, band: FreqBand) -> M1Params<'static> {
    M1Params {
        al_mac: agent,
        controller: CONTROLLER,
        band,
        identity: Identity {
            uuid: [5; 16],
            manufacturer: b"agent",
            model: b"model",
            serial: b"1",
            primary_device_type: [0, 6, 0, 0x50, 0xf2, 4, 0, 1],
        },
        basic: ApRadioBasicCapabilities {
            ruid: radio,
            max_bss: 4,
            operating_classes: vec![BasicOperatingClass {
                operating_class: 115,
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

fn wsc_count(cmdu: &[u8]) -> usize {
    let (_, area) = CmduHeader::parse(cmdu).unwrap();
    TlvIter::new(area)
        .map(Result::unwrap)
        .filter(|tlv| tlv.tlv_type == TlvType::WSC)
        .count()
}

/// A harness with both radios of one agent through M1 and M2.
fn two_radios() -> (Harness, AgentRadio, AgentRadio) {
    let mut harness = Harness::new();
    harness.search(AGENT);
    let first = harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    let second = harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    (harness, first, second)
}

#[test]
fn an_m1_creates_the_radio_node_and_answers_with_a_verifiable_m2() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.frames.clear();
    let mut radio = harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);

    // The node records its agent and the band from RF Bands.
    let node = harness.core.nodes().get(key(RADIO_5)).unwrap();
    assert_eq!(node.band, Some(FreqBand::BAND_5_GHZ));
    assert_eq!(harness.core.nodes.links[&key(RADIO_5)].agent_al, AGENT);

    // One M2 frame, to the agent, with a new MID and one WSC TLV per haul.
    let m2s = harness.frames_of(MsgType::AP_AUTOCONFIGURATION_WSC);
    assert_eq!(m2s.len(), 1);
    let frame = m2s[0].1;
    assert_eq!(frame.remote, AGENT);
    assert_eq!(wsc_count(&frame.cmdu), 2);
    validate(&frame.cmdu, ProfileType::MULTI_AP_PROFILE_3).unwrap();
    // The agent side accepts it and gets the reset file's rows (0013 D6).
    let configs = wsc::accept_m2(
        &mut radio.session,
        AgentState::WscM2Pending,
        CONTROLLER,
        &frame.cmdu,
    )
    .unwrap();
    let ssids: Vec<&[u8]> = configs
        .iter()
        .map(|config| config.ssid.as_slice())
        .collect();
    assert_eq!(ssids, [&b"front"[..], &b"back"[..]]);
    assert_eq!(configs[0].authentication_type, wsc::AUTH_WPA2_PERSONAL);
    assert_eq!(configs[1].authentication_type, wsc::AUTH_SAE);
    assert!(harness.core.orch.is_type_in_progress(CommandType::EmConfig));
}

#[test]
fn an_m1_from_an_agent_without_a_peer_model_is_dropped() {
    let mut harness = Harness::new();
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert!(harness.core.nodes().get(key(RADIO_5)).is_none());
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_WSC).len(),
        0
    );
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
}

#[test]
fn an_m1_for_an_existing_radio_resets_it_unless_it_is_in_wsc_m2_sent() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);

    // A radio in wsc_m2_sent is not reset and no M2 is built.
    harness
        .core
        .nodes
        .table
        .get_mut(key(RADIO_5))
        .unwrap()
        .state = CtrlState::WscM2Sent;
    let before = harness.frames.len();
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(harness.frames.len(), before);
    assert_eq!(harness.state(RADIO_5), CtrlState::WscM2Sent);

    // In any other state the M1 is answered again.
    harness
        .core
        .nodes
        .table
        .get_mut(key(RADIO_5))
        .unwrap()
        .state = CtrlState::TopoSyncPending;
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(harness.frames.len(), before + 1);
}

/// The outputs of a handled M1 apply in upstream's order: remove the old `em_config`, then
/// `wsc_m2_sent`, then submit (`em_configuration.cpp:6262-6275`). With the wrong order the
/// state would end `unconfigured`.
#[test]
fn a_second_m1_leaves_the_radio_in_topo_sync_pending() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);

    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    // The old chain is gone and the new one waits for the radio to become idle.
    assert_eq!(harness.state(RADIO_5), CtrlState::WscM2Sent);
    harness.run_for(Duration::from_millis(600));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert!(harness.core.orch.is_type_in_progress(CommandType::EmConfig));
}

#[test]
fn a_failed_hand_off_applies_none_of_the_outputs() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.core.m2_handoff = M2Handoff::Fail;
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_WSC).len(),
        0
    );
    assert_eq!(harness.state(RADIO_5), CtrlState::WscM1Pending);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
}

// The test switch replaces the first M2's hand-off with a no-op that reports success
// (0013 D10): the radio goes through wsc_m2_sent as after a write the network lost.
#[test]
fn the_test_switch_drops_only_the_first_m2_and_applies_its_outputs() {
    let mut harness = Harness::with(true);
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_WSC).len(),
        0
    );
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert!(harness.core.orch.is_type_in_progress(CommandType::EmConfig));

    // The agent restarts discovery and sends M1 again: this M2 is handed over.
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_WSC).len(),
        1
    );
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_WSC).len(),
        2
    );
}

#[test]
fn a_query_is_sent_each_idle_second_once_every_radio_of_the_agent_waits() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    // The second radio's M1 cannot be answered, so it stays in wsc_m1_pending.
    harness.core.m2_handoff = M2Handoff::Fail;
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.core.m2_handoff = M2Handoff::Deliver;
    assert_eq!(harness.state(RADIO_5), CtrlState::WscM1Pending);

    harness.run_for(5 * SECOND);
    assert_eq!(harness.frames_of(MsgType::TOPOLOGY_QUERY).len(), 0);
    assert_eq!(harness.al_state(), CtrlState::WscM1Pending);

    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(5 * SECOND + Duration::from_millis(500));
    // One Query per second, from the first radio only, to the agent.
    let queries = harness.frames_of(MsgType::TOPOLOGY_QUERY);
    assert_eq!(queries.len(), 5);
    assert!(queries.iter().all(|(_, frame)| frame.remote == AGENT));
    assert_eq!(harness.al_state(), CtrlState::TopoSyncPending);
}

#[test]
fn a_topology_response_synchronizes_every_radio_of_the_agent() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(2 * SECOND);
    harness.response(AGENT, b"front", &[RADIO_24, RADIO_5]);
    assert_eq!(harness.state(RADIO_24), CtrlState::TopoSynchronized);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSynchronized);

    // em_config finishes, and no Query follows.
    harness.run_for(SECOND);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
    let queries = harness.frames_of(MsgType::TOPOLOGY_QUERY).len();
    harness.run_for(10 * SECOND);
    assert_eq!(harness.frames_of(MsgType::TOPOLOGY_QUERY).len(), queries);
}

#[test]
fn a_response_is_ignored_when_no_radio_of_the_agent_waits() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.response(OTHER_AGENT, b"front", &[OTHER_RADIO]);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
}

#[test]
fn an_unknown_ssid_flags_a_mismatch_and_keeps_the_radios_waiting() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(2 * SECOND);
    harness.response(AGENT, b"stranger", &[RADIO_24, RADIO_5]);
    assert_eq!(harness.state(RADIO_24), CtrlState::TopoSyncPending);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert!(harness.core.nodes.links[&key(RADIO_24)].ssid_mismatch);

    // The 120 s path is S3: the 40 s cancel does not fire while a mismatch is flagged.
    harness.run_for(60 * SECOND);
    assert_eq!(harness.state(RADIO_24), CtrlState::TopoSyncPending);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW).len(),
        0
    );
}

// Before 0014 D18 R1 the cancel sent exactly one Renew per radio and no retry. Now it starts
// a retry episode: the first Renew at the cancel, then one per idle proto second, five in all
// (upstream: em_configuration.cpp:850-938, EM_MAX_RENEW_TX_THRESH 5, em_base.h:115).
#[test]
fn the_forty_second_cancel_marks_the_agents_radios_and_starts_a_renew_episode_each() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(39 * SECOND);
    assert_eq!(harness.state(RADIO_24), CtrlState::TopoSyncPending);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW).len(),
        0
    );

    harness.run_for(3 * SECOND);
    assert_eq!(harness.state(RADIO_24), CtrlState::Misconfigured);
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    let renews = harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW);
    // The first two Renews, one per radio in `radios_of` order (the order the radios were
    // created), go out at the cancel.
    let bands: Vec<FreqBand> = renews
        .iter()
        .take(2)
        .map(|(_, frame)| {
            let (_, area) = CmduHeader::parse(&frame.cmdu).unwrap();
            assert_eq!(readers::al_mac_address(area).unwrap(), Some(CONTROLLER));
            readers::freq_band(area).unwrap().unwrap()
        })
        .collect();
    assert_eq!(bands, [FreqBand::BAND_2_4_GHZ, FreqBand::BAND_5_GHZ]);
    assert!(renews.iter().all(|(_, frame)| frame.remote == AGENT));

    // No command is left: no further Query; the retry ends at five Renews per radio.
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
    let queries = harness.frames_of(MsgType::TOPOLOGY_QUERY).len();
    harness.run_for(120 * SECOND);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 5);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
    assert_eq!(harness.frames_of(MsgType::TOPOLOGY_QUERY).len(), queries);
}

// Before 0014 D18 R1 each cancel sent one Renew per radio; now five (see above).
#[test]
fn the_cancel_is_scoped_to_the_radios_of_the_agent() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(20 * SECOND);
    harness.search(OTHER_AGENT);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);
    harness.run_for(22 * SECOND);
    // The first agent timed out; the second has run for 22 s and is not touched.
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    assert_eq!(harness.state(OTHER_RADIO), CtrlState::TopoSyncPending);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 0);

    harness.run_for(22 * SECOND);
    assert_eq!(harness.state(OTHER_RADIO), CtrlState::Misconfigured);
    harness.run_for(20 * SECOND);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 5);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 5);
}

#[test]
fn an_m1_after_the_cancel_starts_a_new_em_config() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(42 * SECOND);
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(Duration::from_millis(600));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert!(harness.core.orch.is_type_in_progress(CommandType::EmConfig));
}

// Replay tests (0016 AC3): output determinism only. With the clock and both random sources
// fixed, a scripted run gives the same outputs every time.

/// Everything the core produced for one scripted step.
#[derive(Debug, PartialEq, Eq)]
struct StepRecord {
    step: &'static str,
    /// Frames out: offset from the start, destination and CMDU bytes.
    frames: Vec<(Duration, MacAddr, Vec<u8>)>,
    next_deadline: Option<Instant>,
}

/// Records the frames the harness gained since `frames_before`, and the core's deadline.
fn record_step(
    records: &mut Vec<StepRecord>,
    step: &'static str,
    harness: &Harness,
    frames_before: usize,
) {
    let frames = harness.frames[frames_before..]
        .iter()
        .map(|(offset, frame)| (*offset, frame.remote, Vec::from(frame.cmdu.as_slice())))
        .collect();
    records.push(StepRecord {
        step,
        frames,
        next_deadline: harness.core.next_deadline(),
    });
}

/// Registered, Search answered, M1 from a seeded test agent, M2 out, ticks, Topology Response.
fn replay_script(core_seed: u64, peer_seed: u64) -> Vec<StepRecord> {
    let mut harness = Harness::seeded(false, core_seed, peer_seed);
    let mut records = Vec::new();
    record_step(&mut records, "registered", &harness, 0);

    let before = harness.frames.len();
    harness.search(AGENT);
    record_step(&mut records, "search", &harness, before);

    let before = harness.frames.len();
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    record_step(&mut records, "m1_2_4", &harness, before);

    let before = harness.frames.len();
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    record_step(&mut records, "m1_5", &harness, before);

    let before = harness.frames.len();
    harness.run_for(2 * SECOND);
    record_step(&mut records, "ticks", &harness, before);

    let before = harness.frames.len();
    harness.response(AGENT, b"front", &[RADIO_24, RADIO_5]);
    record_step(&mut records, "topology_response", &harness, before);

    let before = harness.frames.len();
    harness.run_for(SECOND);
    record_step(&mut records, "final", &harness, before);
    records
}

/// The first M2 the controller sent in a run.
fn first_m2(records: &[StepRecord]) -> &[u8] {
    records
        .iter()
        .flat_map(|record| &record.frames)
        .find(|(_, _, cmdu)| header_of(cmdu).msg_type == MsgType::AP_AUTOCONFIGURATION_WSC)
        .map(|(_, _, cmdu)| cmdu.as_slice())
        .unwrap()
}

/// 0016 AC3, output determinism only: two runs with the same seeds and clock give the same
/// frames and deadlines at every step.
#[test]
fn replay_same_seed_gives_identical_output() {
    let first = replay_script(CORE_SEED, PEER_SEED);
    let second = replay_script(CORE_SEED, PEER_SEED);
    assert_eq!(first, second);
    // The run is not trivially empty: the Search was answered and both M1s got an M2.
    let search_step = first.iter().find(|record| record.step == "search").unwrap();
    assert_eq!(search_step.frames.len(), 1);
    let m2_count = first
        .iter()
        .flat_map(|record| &record.frames)
        .filter(|(_, _, cmdu)| header_of(cmdu).msg_type == MsgType::AP_AUTOCONFIGURATION_WSC)
        .count();
    assert_eq!(m2_count, 2);
}

/// 0016 AC3: only the core's seed changes, and the controller's M2 changes with it, so the
/// core draws its M2 randomness from the injected source.
#[test]
fn replay_core_seed_reaches_m2() {
    let first = replay_script(CORE_SEED, PEER_SEED);
    let other = replay_script(OTHER_CORE_SEED, PEER_SEED);
    assert_ne!(first_m2(&first), first_m2(&other));
}

// M1 path (0014 D18 S2-03, S2-06, S2-08) and peer-model eviction (Q1, R3).

/// Radio `n` of an agent in the tests below, distinct from the named radios.
fn radio_mac(agent_number: u8, n: u8) -> MacAddr {
    MacAddr::new([2, 0, 0, agent_number, 3, n])
}

/// The AL MAC of the filler agent `n`, which only sends a Search.
fn filler_agent(n: u16) -> MacAddr {
    let [high, low] = n.to_be_bytes();
    MacAddr::new([4, 0, 0, 0, high, low])
}

/// The M1 with a TLV of `tlv_type` put before all others. A raw TLV is written, so the
/// value can name a radio other than the one the M1 describes.
fn with_tlv_first(cmdu: &[u8], tlv_type: TlvType, value: &[u8]) -> Vec<u8> {
    let (_, area) = CmduHeader::parse(cmdu).unwrap();
    let header_len = cmdu.len() - area.len();
    let mut out = Vec::from(&cmdu[..header_len]);
    out.push(u8::from(tlv_type));
    out.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
    out.extend_from_slice(value);
    out.extend_from_slice(area);
    out
}

/// A valid M1 for `radio` that carries an AP HT Capabilities TLV for `other` before AP Radio
/// Basic Capabilities, so `readers::radio_id` routes it by `other`.
fn m1_routed_by_other_radio(
    harness: &mut Harness,
    agent: MacAddr,
    radio: MacAddr,
    other: MacAddr,
) -> Vec<u8> {
    let (cmdu, _) = harness.build_m1(agent, radio, FreqBand::BAND_5_GHZ);
    let mut value = Vec::from(other.octets());
    value.push(0);
    let crafted = with_tlv_first(&cmdu, TlvType::AP_HT_CAPABILITIES, &value);
    // The test must reach the RUID check: the M1 parses, and routes by the other radio.
    let (_, area) = CmduHeader::parse(&crafted).unwrap();
    assert_eq!(readers::radio_id(area).unwrap(), Some(other));
    assert_eq!(wsc::parse_m1(&crafted).unwrap().radio(), radio);
    crafted
}

/// An M1 whose AP Radio Basic Capabilities TLV is gone. It routes by the AP Radio Advanced
/// Capabilities TLV and cannot be parsed.
fn m1_without_basic_capabilities(harness: &mut Harness, agent: MacAddr, radio: MacAddr) -> Vec<u8> {
    let (cmdu, _) = harness.build_m1(agent, radio, FreqBand::BAND_5_GHZ);
    let (_, area) = CmduHeader::parse(&cmdu).unwrap();
    let header_len = cmdu.len() - area.len();
    let first = TlvIter::new(area).next().unwrap().unwrap();
    assert_eq!(first.tlv_type, TlvType::AP_RADIO_BASIC_CAPABILITIES);
    let first_len = 3 + first.value.len();
    let mut out = Vec::from(&cmdu[..header_len]);
    out.extend_from_slice(&area[first_len..]);
    let (_, area) = CmduHeader::parse(&out).unwrap();
    assert_eq!(readers::radio_id(area).unwrap(), Some(radio));
    assert!(wsc::parse_m1(&out).is_err());
    out
}

/// An M1 that parses but whose M2 cannot be built: AP Radio Basic Capabilities says the
/// radio supports no BSS (`build_m2` returns `NoBssCapacity`). `build_m1` refuses to write
/// such a value, so the byte is patched in the frame.
fn m1_without_bss_capacity(harness: &mut Harness, agent: MacAddr, radio: MacAddr) -> Vec<u8> {
    let (mut cmdu, _) = harness.build_m1(agent, radio, FreqBand::BAND_5_GHZ);
    let (_, area) = CmduHeader::parse(&cmdu).unwrap();
    let header_len = cmdu.len() - area.len();
    let first = TlvIter::new(area).next().unwrap().unwrap();
    assert_eq!(first.tlv_type, TlvType::AP_RADIO_BASIC_CAPABILITIES);
    // The TLV header is 3 bytes; the value is the RUID (6 bytes), then the BSS maximum.
    cmdu[header_len + 3 + 6] = 0;
    assert_eq!(wsc::parse_m1(&cmdu).unwrap().radio(), radio);
    cmdu
}

// Upstream creates the node from the routing RUID before it parses the M1
// (em_ctrl.cpp:1259-1265), so a bad M1 left a node with an unknown band. Here nothing is made.
#[test]
fn a_bad_m1_for_a_new_radio_creates_nothing() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.frames.clear();

    let unparsable = m1_without_basic_capabilities(&mut harness, AGENT, RADIO_5);
    harness.send(AGENT, &unparsable);
    let mismatched = m1_routed_by_other_radio(&mut harness, AGENT, RADIO_5, OTHER_RADIO);
    harness.send(AGENT, &mismatched);

    harness.assert_radio_absent(RADIO_5);
    harness.assert_radio_absent(OTHER_RADIO);
    assert_eq!(harness.frames.len(), 0);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
    assert!(harness.core.next_deadline().is_none());
}

// Upstream resets a known radio to ctrl_wsc_m1_pending before it parses the M1
// (em_ctrl.cpp:1245-1250).
#[test]
fn a_bad_m1_for_a_known_radio_changes_nothing() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(Duration::from_millis(1500));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    let deadline = harness.proto_deadline(RADIO_5);
    let frames = harness.frames.len();

    let unparsable = m1_without_basic_capabilities(&mut harness, AGENT, RADIO_5);
    harness.send(AGENT, &unparsable);
    let mismatched = m1_routed_by_other_radio(&mut harness, AGENT, RADIO_5, OTHER_RADIO);
    harness.send(AGENT, &mismatched);

    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(harness.chain_agent(RADIO_5), Some(AGENT));
    assert!(harness.core.em_config_in_progress(key(RADIO_5)));
    assert_eq!(harness.proto_deadline(RADIO_5), deadline);
    assert_eq!(harness.frames.len(), frames);
}

// The M2 is built and validated before any state change. An M1 with no BSS capacity
// parses and passes every check, but its M2 cannot be built.
#[test]
fn an_m1_whose_m2_cannot_be_built_for_a_new_radio_creates_nothing() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.frames.clear();
    let model_order: Vec<MacAddr> = harness.core.model_order.iter().copied().collect();

    let cmdu = m1_without_bss_capacity(&mut harness, AGENT, RADIO_5);
    harness.send(AGENT, &cmdu);

    harness.assert_radio_absent(RADIO_5);
    assert_eq!(harness.frames.len(), 0);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
    assert!(harness.core.next_deadline().is_none());
    // AGENT's model was not refreshed: it is still the oldest.
    let after: Vec<MacAddr> = harness.core.model_order.iter().copied().collect();
    assert_eq!(after, model_order);
}

#[test]
fn an_m1_whose_m2_cannot_be_built_leaves_a_waiting_radio_alone() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(Duration::from_millis(1500));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    harness.set_mismatch(RADIO_5);
    let deadline = harness.proto_deadline(RADIO_5);
    let frames = harness.frames.len();

    let cmdu = m1_without_bss_capacity(&mut harness, AGENT, RADIO_5);
    harness.send(AGENT, &cmdu);

    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(harness.chain_agent(RADIO_5), Some(AGENT));
    assert!(harness.core.em_config_in_progress(key(RADIO_5)));
    assert!(harness.mismatch(RADIO_5));
    assert_eq!(harness.proto_deadline(RADIO_5), deadline);
    assert_eq!(harness.frames.len(), frames);
}

// The failed M1 does not restart the 40 s age either: the cancel comes on schedule.
#[test]
fn an_m1_whose_m2_cannot_be_built_does_not_restart_the_age() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(30 * SECOND);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);

    let cmdu = m1_without_bss_capacity(&mut harness, AGENT, RADIO_5);
    harness.send(AGENT, &cmdu);
    harness.run_for(11 * SECOND);

    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
}

#[test]
fn an_m1_whose_m2_cannot_be_built_does_not_end_a_renew_episode() {
    let mut harness = cancelled_single_radio();
    harness.run_for(SECOND);
    let sent = harness.renews_for(AGENT, FreqBand::BAND_5_GHZ);
    assert!((1..5).contains(&sent));

    let cmdu = m1_without_bss_capacity(&mut harness, AGENT, RADIO_5);
    harness.send(AGENT, &cmdu);
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    assert!(harness.core.renew_counts.contains_key(&key(RADIO_5)));
    harness.run_for(20 * SECOND);

    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
}

#[test]
fn a_move_whose_m2_cannot_be_built_leaves_the_radio_with_its_old_owner() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(Duration::from_millis(1500));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    let deadline = harness.proto_deadline(RADIO_5);

    let cmdu = m1_without_bss_capacity(&mut harness, OTHER_AGENT, RADIO_5);
    harness.send(OTHER_AGENT, &cmdu);

    assert_eq!(harness.owner(RADIO_5), Some(AGENT));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(harness.chain_agent(RADIO_5), Some(AGENT));
    assert!(harness.core.em_config_in_progress(key(RADIO_5)));
    assert_eq!(harness.proto_deadline(RADIO_5), deadline);
}

// The routing RUID is the first TLV that carries one. An M1 from agent B that routes by
// agent A's radio must not touch it (upstream would reset and answer it, em_ctrl.cpp:1245-1270).
#[test]
fn an_m1_routed_by_another_agents_radio_leaves_that_radio_alone() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(Duration::from_millis(1500));
    let frames = harness.frames.len();

    let crafted = m1_routed_by_other_radio(&mut harness, OTHER_AGENT, OTHER_RADIO, RADIO_5);
    harness.send(OTHER_AGENT, &crafted);

    assert_eq!(harness.owner(RADIO_5), Some(AGENT));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(harness.chain_agent(RADIO_5), Some(AGENT));
    harness.assert_radio_absent(OTHER_RADIO);
    assert_eq!(harness.frames.len(), frames);
}

// Upstream drops an M1 from an agent without a data model only when it would create the
// node (em_ctrl.cpp:1253-1257); a known radio was reset and answered (S2-06).
#[test]
fn an_m1_without_a_peer_model_does_not_touch_a_known_radio() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(Duration::from_millis(1500));
    let frames = harness.frames.len();

    harness.m1(OTHER_AGENT, RADIO_5, FreqBand::BAND_5_GHZ);

    assert_eq!(harness.owner(RADIO_5), Some(AGENT));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(harness.chain_agent(RADIO_5), Some(AGENT));
    assert_eq!(harness.frames.len(), frames);
}

// Upstream defines EM_MAX_RADIO_PER_AGENT (em_base.h:90) and does not enforce it where it
// creates a radio node (em_ctrl.cpp:1259-1269).
#[test]
fn a_fifth_radio_of_one_agent_is_refused() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    for n in 0..4 {
        harness.m1(AGENT, radio_mac(1, n), FreqBand::BAND_5_GHZ);
    }
    assert_eq!(harness.core.nodes.radios_of(AGENT).len(), 4);
    let frames = harness.frames.len();

    harness.m1(AGENT, radio_mac(1, 4), FreqBand::BAND_5_GHZ);

    harness.assert_radio_absent(radio_mac(1, 4));
    assert_eq!(harness.core.nodes.radios_of(AGENT).len(), 4);
    assert_eq!(harness.frames.len(), frames);
    // A known radio of the full agent still gets its M1 answered.
    harness.m1(AGENT, radio_mac(1, 0), FreqBand::BAND_5_GHZ);
    assert_eq!(harness.frames.len(), frames + 1);
}

// Upstream sends M2 to the M1's sender, but Topology Query and Renew to the first owner's
// data model (em_configuration.cpp:483,959). Here the radio moves as a whole.
#[test]
fn a_radio_moves_to_the_agent_that_sent_the_m1() {
    let (mut harness, _, _) = two_radios();
    harness.search(OTHER_AGENT);
    harness.set_mismatch(RADIO_5);
    harness.frames.clear();
    harness.run_for(Duration::from_millis(1500));
    harness.frames.clear();

    harness.m1(OTHER_AGENT, RADIO_5, FreqBand::BAND_5_GHZ);

    // M2 goes to the new agent; the radio and its chain follow the M1.
    let m2s = harness.frames_of(MsgType::AP_AUTOCONFIGURATION_WSC);
    assert_eq!(m2s.len(), 1);
    assert_eq!(m2s[0].1.remote, OTHER_AGENT);
    assert_eq!(harness.owner(RADIO_5), Some(OTHER_AGENT));
    assert!(!harness.mismatch(RADIO_5));
    assert_eq!(harness.core.nodes.radios_of(AGENT), [key(RADIO_24)]);
    assert_eq!(harness.core.nodes.radios_of(OTHER_AGENT), [key(RADIO_5)]);
    harness.run_for(Duration::from_millis(600));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(harness.chain_agent(RADIO_5), Some(OTHER_AGENT));
    assert_eq!(harness.chain_agent(RADIO_24), Some(AGENT));

    // The Query for each radio goes to its own agent; no Query goes to A for RADIO_5.
    harness.run_for(2 * SECOND);
    let query_targets: Vec<MacAddr> = harness
        .frames_of(MsgType::TOPOLOGY_QUERY)
        .iter()
        .map(|(_, frame)| frame.remote)
        .collect();
    assert!(query_targets.contains(&OTHER_AGENT));
    assert!(query_targets.contains(&AGENT));

    // After 40 s without an answer, each Renew goes to the agent that owns the radio.
    harness.run_for(45 * SECOND);
    let renews: Vec<(FreqBand, MacAddr)> = harness
        .frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW)
        .iter()
        .map(|(_, frame)| {
            let (_, area) = CmduHeader::parse(&frame.cmdu).unwrap();
            (readers::freq_band(area).unwrap().unwrap(), frame.remote)
        })
        .collect();
    assert!(renews.contains(&(FreqBand::BAND_5_GHZ, OTHER_AGENT)));
    assert!(renews.contains(&(FreqBand::BAND_2_4_GHZ, AGENT)));
    assert!(!renews.contains(&(FreqBand::BAND_5_GHZ, AGENT)));
}

// Upstream does not enforce the radio cap (em_base.h:90); a move counts as a new radio of
// the receiving agent, and a refused move leaves the old owner's radio as it was.
#[test]
fn a_move_is_refused_when_the_new_agent_has_the_most_radios() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    for n in 0..4 {
        harness.m1(OTHER_AGENT, radio_mac(2, n), FreqBand::BAND_5_GHZ);
    }
    harness.run_for(Duration::from_millis(1500));
    let deadline = harness.proto_deadline(RADIO_5);
    let frames = harness.frames.len();

    harness.m1(OTHER_AGENT, RADIO_5, FreqBand::BAND_5_GHZ);

    assert_eq!(harness.owner(RADIO_5), Some(AGENT));
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(harness.chain_agent(RADIO_5), Some(AGENT));
    assert_eq!(harness.proto_deadline(RADIO_5), deadline);
    assert_eq!(harness.core.nodes.radios_of(OTHER_AGENT).len(), 4);
    assert_eq!(harness.frames.len(), frames);
}

// Upstream clears a mismatch only in a good Topology Response and flags the first radio
// only (em_configuration.cpp:6597-6632). A fresh M1 on either radio clears the agent's flags.
#[test]
fn a_fresh_m1_clears_the_mismatch_of_every_radio_of_the_agent() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(2 * SECOND);
    harness.response(AGENT, b"stranger", &[RADIO_24, RADIO_5]);
    assert!(harness.mismatch(RADIO_24));
    harness.set_mismatch(RADIO_5);
    harness.run_for(5 * SECOND);
    let queries = harness.frames_of(MsgType::TOPOLOGY_QUERY).len();
    harness.run_for(5 * SECOND);
    assert_eq!(harness.frames_of(MsgType::TOPOLOGY_QUERY).len(), queries);

    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);

    assert!(!harness.mismatch(RADIO_24));
    assert!(!harness.mismatch(RADIO_5));
    harness.run_for(3 * SECOND);
    assert!(harness.frames_of(MsgType::TOPOLOGY_QUERY).len() > queries);
}

// An M1 the owner's `wsc_m2_sent` rule ignores is never built and may be unanswerable, so
// it must not refresh the model: the owner stays the oldest and is the one evicted.
#[test]
fn an_ignored_m1_from_the_owner_does_not_refresh_its_model() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness
        .core
        .nodes
        .table
        .get_mut(key(RADIO_5))
        .unwrap()
        .state = CtrlState::WscM2Sent;
    fill_models(&mut harness, 255);
    assert_eq!(harness.core.models.len(), 256);
    let frames = harness.frames.len();

    let unanswerable = m1_without_bss_capacity(&mut harness, AGENT, RADIO_5);
    harness.send(AGENT, &unanswerable);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);

    assert_eq!(harness.frames.len(), frames);
    assert_eq!(harness.core.model_order.front(), Some(&AGENT));
    harness.search(filler_agent(1000));
    assert!(harness.core.peer(AGENT).is_none());
    assert!(harness.core.peer(filler_agent(0)).is_some());
}

/// Searches from `count` filler agents, so the peer-model table fills up.
fn fill_models(harness: &mut Harness, count: u16) {
    for n in 0..count {
        harness.search(filler_agent(n));
    }
}

// Upstream keeps peer models without limit; here the model heard from least recently goes
// (R3), so a Search, an M1 or a Topology Response keeps an agent in the table.
#[test]
fn a_search_keeps_a_model_in_a_full_table() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    fill_models(&mut harness, 255);
    assert_eq!(harness.core.models.len(), 256);
    harness.search(AGENT);

    harness.search(filler_agent(1000));

    assert!(harness.core.peer(AGENT).is_some());
    assert!(harness.core.peer(filler_agent(0)).is_none());
    assert!(harness.core.peer(filler_agent(1)).is_some());
}

#[test]
fn an_m1_keeps_a_model_in_a_full_table() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    fill_models(&mut harness, 255);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);

    harness.search(filler_agent(1000));

    assert!(harness.core.peer(AGENT).is_some());
    assert!(harness.core.peer(filler_agent(0)).is_none());
    assert!(harness.core.nodes().get(key(RADIO_5)).is_some());
}

// A refused M1 is no sign of life worth a place in the table: an agent at its radio cap
// could otherwise keep its model by sending M1s.
#[test]
fn an_m1_refused_at_the_radio_cap_does_not_keep_a_model_in_a_full_table() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    for n in 0..4 {
        harness.m1(AGENT, radio_mac(1, n), FreqBand::BAND_5_GHZ);
    }
    fill_models(&mut harness, 255);
    assert_eq!(harness.core.models.len(), 256);

    harness.m1(AGENT, radio_mac(1, 4), FreqBand::BAND_5_GHZ);
    harness.assert_radio_absent(radio_mac(1, 4));
    harness.search(filler_agent(1000));

    assert!(harness.core.peer(AGENT).is_none());
    assert!(harness.core.peer(filler_agent(0)).is_some());
}

#[test]
fn a_topology_response_keeps_a_model_in_a_full_table() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    fill_models(&mut harness, 255);
    harness.run_for(2 * SECOND);
    harness.response(AGENT, b"front", &[RADIO_5]);

    harness.search(filler_agent(1000));

    assert!(harness.core.peer(AGENT).is_some());
    assert!(harness.core.peer(filler_agent(0)).is_none());
}

// Upstream never evicts, so its models never leave radio nodes behind. Here an evicted agent
// leaves nothing: no chain to time out, no node to tick, no Renew to an agent it forgot.
#[test]
fn an_evicted_agent_leaves_nothing_behind() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);
    harness.run_for(2 * SECOND);
    harness.response(OTHER_AGENT, b"front", &[OTHER_RADIO]);
    harness.run_for(SECOND);
    assert_eq!(harness.state(OTHER_RADIO), CtrlState::TopoSynchronized);
    assert!(harness.core.orch.is_type_in_progress(CommandType::EmConfig));
    fill_models(&mut harness, 254);
    assert_eq!(harness.core.models.len(), 256);

    // The oldest model is AGENT's; the new Search evicts it.
    harness.search(filler_agent(1000));

    assert!(harness.core.peer(AGENT).is_none());
    assert!(harness.core.peer(OTHER_AGENT).is_some());
    harness.assert_radio_absent(RADIO_24);
    harness.assert_radio_absent(RADIO_5);
    harness.run_for(SECOND);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
    let frames = harness.frames.len();

    harness.run_for(41 * SECOND);

    assert_eq!(harness.frames.len(), frames);
    assert_eq!(harness.state(OTHER_RADIO), CtrlState::TopoSynchronized);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW).len(),
        0
    );

    // The agent has no peer model any more: its M1 is dropped (S2-06 path).
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.assert_radio_absent(RADIO_5);
    assert_eq!(harness.frames.len(), frames);
}

// A move does not wait for the M2: when the hand-off fails, the old owner's em_config for the
// radio is still gone and the radio waits for the new owner's M1 (the M2 path removes the
// chain only after a successful hand-off, em_configuration.cpp:6262-6275).
#[test]
fn a_move_removes_the_old_owners_chain_even_when_the_m2_cannot_be_handed_over() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(Duration::from_millis(1500));
    assert_eq!(harness.chain_agent(RADIO_5), Some(AGENT));

    harness.core.m2_handoff = M2Handoff::Fail;
    harness.m1(OTHER_AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(SECOND);

    assert_eq!(harness.owner(RADIO_5), Some(OTHER_AGENT));
    assert_eq!(harness.state(RADIO_5), CtrlState::WscM1Pending);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
}

// Renew retry (0014 D18 R1, R2). Upstream retries up to EM_MAX_RENEW_TX_THRESH = 5 while a
// cfg_renew command is in progress (em_configuration.cpp:850-938, em.cpp:546-559); the
// controller has no such command, so the idle proto tick of a Misconfigured radio sends them.

/// One agent, one radio, M1 answered, run to the 40 s cancel and its first Renew.
fn cancelled_single_radio() -> Harness {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    while harness.state(RADIO_5) != CtrlState::Misconfigured {
        harness.run_for(Duration::from_millis(250));
    }
    harness
}

#[test]
fn a_cancelled_radio_gets_exactly_five_renews_and_none_after() {
    let mut harness = cancelled_single_radio();
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 1);

    harness.run_for(20 * SECOND);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
    let renews = harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW);
    for pair in renews.windows(2).skip(1) {
        assert_eq!(pair[1].0.abs_diff(pair[0].0), SECOND);
    }

    harness.run_for(200 * SECOND);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
}

#[test]
fn an_m1_during_the_retry_stops_it() {
    let mut harness = cancelled_single_radio();
    harness.run_for(2 * SECOND);
    let sent = harness.renews_for(AGENT, FreqBand::BAND_5_GHZ);
    assert!((2..5).contains(&sent));

    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    harness.run_for(20 * SECOND);

    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), sent);
}

// Upstream carries the count into the next episode (em_configuration.cpp:935-938,
// em_orch_ctrl.cpp:171-175,418-420); here each cancel starts a fresh five (R1).
#[test]
fn a_second_cancel_starts_a_fresh_episode_of_five() {
    let mut harness = cancelled_single_radio();
    harness.run_for(SECOND);
    let first_episode = harness.renews_for(AGENT, FreqBand::BAND_5_GHZ);
    assert!((2..5).contains(&first_episode));

    // The M1 is answered, the agent never follows up: em_config is cancelled again.
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.run_for(100 * SECOND);

    assert_eq!(
        harness.renews_for(AGENT, FreqBand::BAND_5_GHZ),
        first_episode + 5
    );
}

#[test]
fn each_radio_has_its_own_retry_count() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(20 * SECOND);
    harness.search(OTHER_AGENT);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);
    // The first agent's episode is over (cancel at 41 s); the second agent's cancel is at 61 s.
    harness.run_for(28 * SECOND);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 5);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 0);

    harness.run_for(15 * SECOND);
    let second_agent = harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ);
    assert!((1..5).contains(&second_agent));
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);

    harness.run_for(20 * SECOND);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 5);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 5);
}

// The orchestrator activates one pending stage per pass, newest first. Reached here:
// RADIO_24 runs em_config from the start. RADIO_5 and OTHER_RADIO have their chains removed
// and are submitted again at once, with no pass in between, so both stages are pending.
// The pass at the 40 s cancel activates the newest (OTHER_RADIO) before the hook runs;
// RADIO_5 is still pending, so only the effects of `transient` can mark it.
#[test]
fn a_radio_with_a_pending_em_config_stage_is_marked_and_renewed_by_the_cancel() {
    let (mut harness, _, _) = two_radios();
    harness.search(OTHER_AGENT);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);
    for radio in [RADIO_5, OTHER_RADIO] {
        harness.core.remove_em_config_chain(key(radio), harness.now);
    }
    // The 40 s cancel comes on the idle tick at 41 s; submit after the tick at 40.75 s.
    harness.run_for(Duration::from_millis(40_750));
    assert_eq!(harness.state(RADIO_24), CtrlState::TopoSyncPending);
    for radio in [RADIO_5, OTHER_RADIO] {
        let config = EmConfig {
            radio: key(radio),
            agent_al: harness.owner(radio).unwrap(),
        };
        harness.core.submit_em_config(config, harness.now);
    }
    assert_eq!(harness.state(RADIO_5), CtrlState::Unconfigured);
    assert!(harness.core.orch.active(key(RADIO_5)).is_none());

    harness.run_for(2 * SECOND);
    assert_eq!(harness.state(RADIO_24), CtrlState::Misconfigured);
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    harness.run_for(20 * SECOND);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 5);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 0);
}

// A move or an eviction is not a cancel (upstream comments out pre_process_cancel in
// remove_chain_for_node, em_orch.cpp:252): it ends the retry and sends nothing.
#[test]
fn a_move_during_the_retry_stops_it() {
    let mut harness = cancelled_single_radio();
    harness.search(OTHER_AGENT);
    harness.run_for(SECOND);
    let sent = harness.renews_for(AGENT, FreqBand::BAND_5_GHZ);
    assert!((1..5).contains(&sent));
    assert!(harness.core.renew_counts.contains_key(&key(RADIO_5)));

    harness.m1(OTHER_AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(harness.owner(RADIO_5), Some(OTHER_AGENT));
    assert!(!harness.core.renew_counts.contains_key(&key(RADIO_5)));
    harness.run_for(10 * SECOND);

    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), sent);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 0);
}

#[test]
fn an_eviction_during_the_retry_stops_it() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    fill_models(&mut harness, 255);
    harness.run_for(42 * SECOND + Duration::from_millis(500));
    let sent = harness.renews_for(AGENT, FreqBand::BAND_5_GHZ);
    assert!((1..5).contains(&sent));

    // AGENT's model is the oldest; the new Search evicts it.
    harness.search(filler_agent(1000));
    assert!(harness.core.peer(AGENT).is_none());
    assert!(harness.core.renew_counts.is_empty());
    harness.run_for(20 * SECOND);

    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), sent);
}

// Per-agent 40 s age (0014 D18 S2-04). Upstream keeps one elapsed time per command type and
// resets it for the whole type (em_orch.cpp:72-88,129-138,222), so an agent held in
// mismatch past 40 s cancelled every agent that onboarded after it.
#[test]
fn an_agent_held_in_mismatch_does_not_cancel_a_new_agent() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(2 * SECOND);
    harness.response(AGENT, b"stranger", &[RADIO_24, RADIO_5]);
    harness.run_for(50 * SECOND);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);

    harness.search(OTHER_AGENT);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);
    harness.run_for(5 * SECOND);

    assert_eq!(harness.state(OTHER_RADIO), CtrlState::TopoSyncPending);
    let queries_to_new_agent = harness
        .frames_of(MsgType::TOPOLOGY_QUERY)
        .iter()
        .filter(|(_, frame)| frame.remote == OTHER_AGENT)
        .count();
    assert!(queries_to_new_agent >= 1);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW).len(),
        0
    );
}

// A fresh M1 on one radio of an agent restarts the 40 s of its other radios (upstream resets
// the one time of the whole type, em_orch.cpp:129-138,222).
#[test]
fn a_fresh_m1_extends_the_other_radios_of_the_agent() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(30 * SECOND);
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);

    // 50 s after the first M1s: past their old 40 s, 20 s after the fresh M1.
    harness.run_for(20 * SECOND);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW).len(),
        0
    );

    harness.run_for(30 * SECOND);
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
}

// The M1 alone restarts the age; it does not wait for the M2 to be handed over.
#[test]
fn a_fresh_m1_extends_the_other_radios_even_when_the_m2_cannot_be_handed_over() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(30 * SECOND);
    harness.core.m2_handoff = M2Handoff::Fail;
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);

    harness.run_for(20 * SECOND);
    assert_eq!(harness.state(RADIO_5), CtrlState::TopoSyncPending);
}

// Upstream cancels the whole command type when a radio in another state is stuck
// (em_orch_ctrl.cpp:142-150); here only the radios of its agent are cancelled.
#[test]
fn a_stuck_radio_cancels_only_the_radios_of_its_agent() {
    let (mut harness, _, _) = two_radios();
    for radio in [RADIO_24, RADIO_5] {
        harness.core.nodes.table.get_mut(key(radio)).unwrap().state = CtrlState::Unconfigured;
    }
    harness.run_for(20 * SECOND);
    harness.search(OTHER_AGENT);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);

    harness.run_for(22 * SECOND);

    assert_eq!(harness.state(RADIO_24), CtrlState::Misconfigured);
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    assert!((1..5).contains(&harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ)));
    assert_eq!(harness.state(OTHER_RADIO), CtrlState::TopoSyncPending);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 0);
}

// Upstream's type cancel marks only the candidates of cancelled em_config commands
// (em_orch.cpp:270-301). A sibling radio that finished its chain is not one of them.
#[test]
fn a_stuck_radio_leaves_a_finished_sibling_radio_of_its_agent_alone() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    harness.run_for(2 * SECOND);
    harness.response(AGENT, b"front", &[RADIO_24]);
    harness.run_for(SECOND);
    assert_eq!(harness.state(RADIO_24), CtrlState::TopoSynchronized);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));

    // RADIO_5 gets a good M1, then a second one whose M2 cannot be handed over. The M2 is
    // built before any state change, so a build failure would leave the radio as it was;
    // only the hand-off, which comes after the state change, can leave it in wsc_m1_pending
    // with its stage active.
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.core.m2_handoff = M2Handoff::Fail;
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    assert_eq!(harness.state(RADIO_5), CtrlState::WscM1Pending);
    assert!(harness.core.orch.active(key(RADIO_5)).is_some());

    harness.run_for(60 * SECOND);

    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 5);
    assert_eq!(harness.state(RADIO_24), CtrlState::TopoSynchronized);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 0);
}

// Rule (b) of the 40 s cancel: a radio stuck outside topo_sync_pending cancels
// em_config for its agent, and only the cancel hook marks and renews, for active stages.
// A stage still pending is dropped silently, as upstream (em_orch.cpp:278-302).
#[test]
fn a_pending_stage_of_the_agent_is_dropped_silently_by_a_non_topology_cancel() {
    let mut harness = Harness::new();
    harness.search(AGENT);
    harness.search(OTHER_AGENT);
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    harness.run_for(2 * SECOND);
    // RADIO_24 (X) is put in wsc_m1_pending by an M1 whose M2 cannot be handed over; its
    // stage stays active.
    harness.core.m2_handoff = M2Handoff::Fail;
    harness.m1(AGENT, RADIO_24, FreqBand::BAND_2_4_GHZ);
    harness.core.m2_handoff = M2Handoff::Deliver;
    assert_eq!(harness.state(RADIO_24), CtrlState::WscM1Pending);
    assert!(harness.core.orch.active(key(RADIO_24)).is_some());
    harness.m1(AGENT, RADIO_5, FreqBand::BAND_5_GHZ);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);
    for radio in [RADIO_5, OTHER_RADIO] {
        harness.core.remove_em_config_chain(key(radio), harness.now);
    }

    // The cancel comes on the idle tick after 40 s of age; submit just before it. The pass
    // activates the newest pending stage (OTHER_RADIO) and leaves RADIO_5 (Y) pending.
    harness.run_for(Duration::from_millis(40_750));
    for radio in [RADIO_5, OTHER_RADIO] {
        let config = EmConfig {
            radio: key(radio),
            agent_al: harness.owner(radio).unwrap(),
        };
        harness.core.submit_em_config(config, harness.now);
    }
    assert!(harness.core.orch.active(key(RADIO_5)).is_none());
    assert!(harness.has_em_config_command(RADIO_5));
    assert_eq!(harness.state(RADIO_24), CtrlState::WscM1Pending);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 0);

    harness.run_for(Duration::from_millis(250));
    assert_eq!(harness.state(RADIO_24), CtrlState::Misconfigured);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 1);
    assert_eq!(harness.state(RADIO_5), CtrlState::Unconfigured);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 0);
    assert!(!harness.has_em_config_command(RADIO_5));
    assert_ne!(harness.state(OTHER_RADIO), CtrlState::Misconfigured);

    harness.run_for(20 * SECOND);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_2_4_GHZ), 5);
    assert_eq!(harness.state(RADIO_5), CtrlState::Unconfigured);
    assert_eq!(harness.renews_for(AGENT, FreqBand::BAND_5_GHZ), 0);
    assert_eq!(harness.renews_for(OTHER_AGENT, FreqBand::BAND_5_GHZ), 0);
    assert_ne!(harness.state(OTHER_RADIO), CtrlState::Misconfigured);
}
