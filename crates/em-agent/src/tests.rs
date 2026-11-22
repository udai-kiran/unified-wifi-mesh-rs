// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use em_cmd::{ChainId, CommandType, CommandView, NodeKey, NodeKind, OrchState, RadioSpec};
use em_core::AgentState;
use em_core::configuration::{build_response, build_search};
use em_msg::{CmduHeader, MsgBuilder, RAW_BUDGET, readers};
use em_types::{FreqBand, MacAddr, MsgType, TlvType};

use crate::{AgentCore, AlMacChanged, Frame};

const AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
const OTHER_AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 9]);
const CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
const RADIO_24: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 1]);
const RADIO_5: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 2]);
const SECOND: Duration = Duration::from_secs(1);

fn radio_key(mac: MacAddr) -> NodeKey {
    NodeKey {
        mac,
        kind: NodeKind::Radio,
    }
}

fn al_key() -> NodeKey {
    NodeKey {
        mac: AGENT,
        kind: NodeKind::Al,
    }
}

fn two_radios() -> Vec<RadioSpec> {
    vec![
        RadioSpec {
            ruid: RADIO_24,
            band: FreqBand::BAND_2_4_GHZ,
        },
        RadioSpec {
            ruid: RADIO_5,
            band: FreqBand::BAND_5_GHZ,
        },
    ]
}

/// A Search as the agent sent it: AL MAC, band and message ID.
struct SentSearch {
    al_mac: MacAddr,
    band: FreqBand,
    mid: u16,
}

fn read_search(frame: &Frame) -> SentSearch {
    let (header, area) = CmduHeader::parse(&frame.cmdu).unwrap();
    assert_eq!(header.msg_type, MsgType::AP_AUTOCONFIGURATION_SEARCH);
    assert!(header.relay);
    assert_eq!(frame.remote, MacAddr::IEEE1905_MULTICAST);
    SentSearch {
        al_mac: readers::al_mac_address(area).unwrap().unwrap(),
        band: readers::freq_band(area).unwrap().unwrap(),
        mid: header.msg_id,
    }
}

/// Drives a core with virtual time and records every frame with its offset
/// from the start.
struct Harness {
    core: AgentCore,
    start: Instant,
    now: Instant,
    frames: Vec<(Duration, Frame)>,
}

impl Harness {
    fn new(profile: u8, radios: Vec<RadioSpec>) -> Self {
        let start = Instant::now();
        Self {
            core: AgentCore::new(profile, radios),
            start,
            now: start,
            frames: Vec::new(),
        }
    }

    fn register(&mut self) {
        self.core.registered(AGENT, self.now).unwrap();
    }

    /// Fires every deadline up to `offset` from the start, in order.
    fn run_to(&mut self, offset: Duration) {
        let end = self.start + offset;
        while let Some(due) = self.core.next_deadline() {
            if due > end {
                break;
            }
            self.now = self.now.max(due);
            self.tick_now();
        }
        self.now = self.now.max(end);
    }

    fn tick_now(&mut self) {
        let offset = self.now - self.start;
        for frame in self.core.tick(self.now) {
            self.frames.push((offset, frame));
        }
    }

    fn deliver(&mut self, cmdu: &[u8]) {
        let offset = self.now - self.start;
        for frame in self.core.received(CONTROLLER, cmdu, self.now) {
            self.frames.push((offset, frame));
        }
    }

    fn searches(&self) -> Vec<(Duration, SentSearch)> {
        self.frames
            .iter()
            .map(|(offset, frame)| (*offset, read_search(frame)))
            .collect()
    }

    fn state(&self, ruid: MacAddr) -> AgentState {
        self.core.nodes().get(radio_key(ruid)).unwrap().state
    }

    fn orch_state(&self, ruid: MacAddr) -> OrchState {
        self.core.orch.orch_state(radio_key(ruid))
    }
}

fn response_for(band: FreqBand, profile: u8, mid: u16) -> Vec<u8> {
    build_response(band, profile, mid).unwrap()
}

fn response_without_profile(band: FreqBand) -> Vec<u8> {
    let header = CmduHeader {
        msg_type: MsgType::AP_AUTOCONFIGURATION_RESPONSE,
        msg_id: 5,
        frag_id: 0,
        last_fragment: true,
        relay: false,
    };
    let mut builder = MsgBuilder::new(&header, RAW_BUDGET);
    builder
        .tlv(TlvType::SUPPORTED_ROLE, &[0])
        .tlv(TlvType::SUPPORTED_FREQ_BAND, &[u8::from(band)])
        .tlv(TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY, &[0, 0, 0])
        .tlv(TlvType::CONTROLLER_CAPABILITY, &[0xc0]);
    builder.finish().unwrap()
}

#[test]
fn dev_init_is_submitted_only_after_the_first_registration() {
    let mut harness = Harness::new(3, two_radios());
    assert_eq!(harness.core.next_deadline(), None);
    harness.run_to(5 * SECOND);
    assert!(harness.core.nodes().iter().next().is_none());
    assert_eq!(harness.frames.len(), 0);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::DevInit));

    harness.register();
    let keys: Vec<NodeKey> = harness.core.nodes().iter().map(|node| node.key).collect();
    assert_eq!(keys, [al_key(), radio_key(RADIO_24), radio_key(RADIO_5)]);
    assert!(harness.core.orch.is_type_in_progress(CommandType::DevInit));
    assert_eq!(harness.core.al_mac(), Some(AGENT));
    for ruid in [RADIO_24, RADIO_5] {
        let node = harness.core.nodes().get(radio_key(ruid)).unwrap();
        assert_eq!(node.state, AgentState::Unconfigured);
        assert_eq!(node.profile, 3);
        assert_eq!(harness.orch_state(ruid), OrchState::Progress);
    }
    assert_eq!(harness.core.nodes().get(al_key()).unwrap().band, None);
    assert_eq!(harness.orch_state(RADIO_5), OrchState::Progress);
    assert!(harness.core.orch.command(ChainId(0)).is_some());
}

#[test]
fn nodes_carry_the_configured_profile() {
    let mut harness = Harness::new(2, two_radios());
    harness.register();
    for node in harness.core.nodes().iter() {
        assert_eq!(node.profile, 2);
    }
}

#[test]
fn a_search_per_radio_on_each_idle_proto_tick_with_increasing_mids() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(Duration::from_millis(999));
    assert_eq!(harness.frames.len(), 0);
    harness.run_to(3 * SECOND);
    let searches = harness.searches();
    assert_eq!(searches.len(), 6);
    let mut last_mid = 0;
    for (index, (offset, search)) in searches.iter().enumerate() {
        assert_eq!(*offset, SECOND * u32::try_from(index / 2 + 1).unwrap());
        assert_eq!(search.al_mac, AGENT);
        let expected_band = if index % 2 == 0 {
            FreqBand::BAND_2_4_GHZ
        } else {
            FreqBand::BAND_5_GHZ
        };
        assert_eq!(search.band, expected_band);
        assert!(search.mid > last_mid);
        last_mid = search.mid;
    }
    assert_eq!(searches[0].1.mid, 1);
    assert_eq!(harness.state(RADIO_5), AgentState::AutoconfigRspPending);
}

#[test]
fn no_search_while_unregistered_and_reregistration_continues() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(Duration::from_millis(1500));
    assert_eq!(harness.frames.len(), 2);

    harness.core.unregistered();
    harness.run_to(6 * SECOND);
    assert_eq!(harness.frames.len(), 2);
    // Only the orchestrator deadline remains while the link is down.
    let due = harness.core.next_deadline().unwrap();
    assert!(due - harness.now <= Duration::from_millis(250));

    harness.core.registered(AGENT, harness.now).unwrap();
    harness.run_to(7 * SECOND);
    let searches = harness.searches();
    assert!(searches.len() > 2);
    // The same session continues: MIDs go on, nodes keep their state, and
    // dev_init was not submitted again.
    assert_eq!(searches[2].1.mid, 3);
    assert_eq!(harness.core.nodes().iter().count(), 3);
    assert!(harness.core.orch.command(ChainId(1)).is_none());
}

#[test]
fn registration_with_a_different_al_mac_is_fatal() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(Duration::from_millis(1500));
    harness.core.unregistered();
    let result = harness.core.registered(OTHER_AGENT, harness.now);
    assert_eq!(
        result,
        Err(AlMacChanged {
            recorded: AGENT,
            new: OTHER_AGENT
        })
    );
    assert_eq!(harness.core.al_mac(), Some(AGENT));
    assert_eq!(harness.core.nodes().iter().count(), 3);
    // The link stays down after a rejected registration.
    let before = harness.frames.len();
    harness.run_to(5 * SECOND);
    assert_eq!(harness.frames.len(), before);
}

// intentional-difference:search-until-found
// Upstream cancels dev_init once it is older than 10 s (`em_orch_agent.cpp:42-74`);
// the port keeps searching while any radio is unconfigured or waiting.
#[test]
fn searching_continues_past_ten_seconds() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(15 * SECOND);
    let searches = harness.searches();
    assert_eq!(searches.len(), 30);
    let (last_offset, last) = searches.last().unwrap();
    assert_eq!(*last_offset, 15 * SECOND);
    assert_eq!(last.mid, 30);
    assert!(harness.core.orch.command(ChainId(0)).is_some());
    assert_eq!(harness.orch_state(RADIO_24), OrchState::Progress);
    assert_eq!(harness.orch_state(RADIO_5), OrchState::Progress);
    assert_eq!(harness.state(RADIO_5), AgentState::AutoconfigRspPending);
}

// intentional-difference:elapsed-whole-seconds
// The cancel needs more than 10 whole elapsed seconds, counted from the last
// orchestrator pass that saw a searching node; upstream can cancel up to 1 s earlier.
#[test]
fn after_both_responses_the_cancel_resets_only_the_last_candidate() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(3 * SECOND);
    // The orchestrator pass at 3.0 s is the last one that sees a searching node.
    let last_searching_tick = 3 * SECOND;
    harness.deliver(&response_for(FreqBand::BAND_2_4_GHZ, 3, 0x7001));
    harness.deliver(&response_for(FreqBand::BAND_5_GHZ, 3, 0x7002));
    assert_eq!(harness.state(RADIO_24), AgentState::WscM2Pending);
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);

    harness.run_to(last_searching_tick + Duration::from_millis(10_900));
    assert!(harness.core.orch.command(ChainId(0)).is_some());
    assert_eq!(harness.orch_state(RADIO_24), OrchState::Progress);
    assert_eq!(harness.orch_state(RADIO_5), OrchState::Progress);
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);

    harness.run_to(last_searching_tick + 12 * SECOND);
    // Candidates are visited last first: only the last radio goes back to
    // unconfigured; the first keeps its state although it was cancelled too.
    assert_eq!(harness.state(RADIO_5), AgentState::Unconfigured);
    assert_eq!(harness.state(RADIO_24), AgentState::WscM2Pending);
    assert!(harness.core.orch.command(ChainId(0)).is_none());
    assert_eq!(harness.orch_state(RADIO_24), OrchState::Idle);
    assert_eq!(harness.orch_state(RADIO_5), OrchState::Idle);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::DevInit));

    // The gate needs an active dev_init, so the reset radio does not search again.
    let before = harness.frames.len();
    harness.run_to(last_searching_tick + 20 * SECOND);
    assert_eq!(harness.frames.len(), before);
}

#[test]
fn a_radio_in_wsc_m2_pending_stops_resending() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(2 * SECOND);
    harness.deliver(&response_for(FreqBand::BAND_5_GHZ, 3, 1));
    let before = harness.frames.len();
    harness.run_to(5 * SECOND);
    let later: Vec<SentSearch> = harness.frames[before..]
        .iter()
        .map(|(_, frame)| read_search(frame))
        .collect();
    assert_eq!(later.len(), 3);
    assert!(
        later
            .iter()
            .all(|search| search.band == FreqBand::BAND_2_4_GHZ)
    );
}

#[test]
fn a_response_is_accepted_whatever_its_message_id() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(SECOND);
    assert_eq!(harness.searches()[1].1.mid, 2);
    harness.deliver(&response_for(FreqBand::BAND_5_GHZ, 3, 0xFFF0));
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);
    let controller = harness
        .core
        .nodes()
        .get(radio_key(RADIO_5))
        .unwrap()
        .controller
        .unwrap();
    assert_eq!(controller.al_mac, CONTROLLER);
    assert_eq!(controller.profile, 3);
}

#[test]
fn a_response_routes_by_band_and_state() {
    let radios = vec![
        RadioSpec {
            ruid: RADIO_24,
            band: FreqBand::BAND_2_4_GHZ,
        },
        RadioSpec {
            ruid: RADIO_5,
            band: FreqBand::BAND_5_GHZ,
        },
        RadioSpec {
            ruid: MacAddr::new([2, 0, 0, 0, 1, 3]),
            band: FreqBand::BAND_5_GHZ,
        },
    ];
    let second_5ghz = radios[2].ruid;
    let mut harness = Harness::new(3, radios);
    harness.register();
    harness.run_to(SECOND);

    // A band no radio has is dropped.
    harness.deliver(&response_for(FreqBand::BAND_6_GHZ, 3, 1));
    for node in harness.core.nodes().iter() {
        assert!(node.controller.is_none());
    }

    // The first 5 GHz radio takes the first Response, the second the next.
    harness.deliver(&response_for(FreqBand::BAND_5_GHZ, 3, 2));
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert_eq!(harness.state(second_5ghz), AgentState::AutoconfigRspPending);
    assert_eq!(harness.state(RADIO_24), AgentState::AutoconfigRspPending);
    harness.deliver(&response_for(FreqBand::BAND_5_GHZ, 3, 3));
    assert_eq!(harness.state(second_5ghz), AgentState::WscM2Pending);

    // Both are now excluded by state: a third 5 GHz Response changes nothing.
    harness.deliver(&response_for(FreqBand::BAND_5_GHZ, 3, 4));
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert_eq!(harness.state(second_5ghz), AgentState::WscM2Pending);
    assert_eq!(harness.state(RADIO_24), AgentState::AutoconfigRspPending);
}

// intentional-difference:response-profile-required
#[test]
fn a_rejected_response_keeps_the_radio_searching() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(SECOND);
    harness.deliver(&response_without_profile(FreqBand::BAND_5_GHZ));
    assert_eq!(harness.state(RADIO_5), AgentState::AutoconfigRspPending);
    harness.run_to(2 * SECOND);
    assert_eq!(harness.frames.len(), 4);
}

#[test]
fn unrelated_and_garbled_cmdus_are_ignored() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(SECOND);
    let search = build_search(CONTROLLER, FreqBand::BAND_5_GHZ, 3, 9).unwrap();
    harness.deliver(&search);
    harness.deliver(&[1, 2, 3]);
    assert_eq!(harness.state(RADIO_5), AgentState::AutoconfigRspPending);
}
