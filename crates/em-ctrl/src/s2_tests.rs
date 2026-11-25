// SPDX-License-Identifier: Apache-2.0

//! Tests of the controller's S2 flow in virtual time (note 0013 D3, D7, D10).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use em_cmd::{CommandType, NodeKey, NodeKind};
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
        let mut session = Session::new();
        let params = m1_params(agent, radio, band);
        let cmdu =
            wsc::build_m1(&params, &mut self.ids, &mut self.peer_random, &mut session).unwrap();
        self.send(agent, &cmdu);
        AgentRadio { session }
    }

    fn frames_of(&self, msg_type: MsgType) -> Vec<(Duration, &Frame)> {
        self.frames
            .iter()
            .filter(|(_, frame)| header_of(&frame.cmdu).msg_type == msg_type)
            .map(|(offset, frame)| (*offset, frame))
            .collect()
    }

    fn state(&self, radio: MacAddr) -> CtrlState {
        self.core.nodes.state(key(radio)).unwrap()
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

#[test]
fn the_forty_second_cancel_marks_the_agents_radios_and_sends_one_renew_each() {
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
    assert_eq!(renews.len(), 2);
    let bands: Vec<FreqBand> = renews
        .iter()
        .map(|(_, frame)| {
            let (_, area) = CmduHeader::parse(&frame.cmdu).unwrap();
            assert_eq!(readers::al_mac_address(area).unwrap(), Some(CONTROLLER));
            readers::freq_band(area).unwrap().unwrap()
        })
        .collect();
    assert_eq!(bands, [FreqBand::BAND_2_4_GHZ, FreqBand::BAND_5_GHZ]);
    assert!(renews.iter().all(|(_, frame)| frame.remote == AGENT));

    // No command is left: no further Query and no further renew.
    assert!(!harness.core.orch.is_type_in_progress(CommandType::EmConfig));
    let queries = harness.frames_of(MsgType::TOPOLOGY_QUERY).len();
    harness.run_for(120 * SECOND);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW).len(),
        2
    );
    assert_eq!(harness.frames_of(MsgType::TOPOLOGY_QUERY).len(), queries);
}

#[test]
fn the_cancel_is_scoped_to_the_radios_of_the_agent() {
    let (mut harness, _, _) = two_radios();
    harness.run_for(20 * SECOND);
    harness.search(OTHER_AGENT);
    harness.m1(OTHER_AGENT, OTHER_RADIO, FreqBand::BAND_5_GHZ);
    harness.run_for(22 * SECOND);
    // The first agent timed out; the second has run for 22 s.
    assert_eq!(harness.state(RADIO_5), CtrlState::Misconfigured);
    assert_eq!(harness.state(OTHER_RADIO), CtrlState::TopoSyncPending);
    assert_eq!(
        harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW).len(),
        2
    );

    harness.run_for(22 * SECOND);
    assert_eq!(harness.state(OTHER_RADIO), CtrlState::Misconfigured);
    let renews = harness.frames_of(MsgType::AP_AUTOCONFIGURATION_RENEW);
    assert_eq!(renews.len(), 3);
    assert_eq!(renews[2].1.remote, OTHER_AGENT);
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
