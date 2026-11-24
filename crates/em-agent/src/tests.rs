// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use em_cmd::{ChainId, CommandType, CommandView, NodeKey, NodeKind, OrchState, RadioSpec};
use em_core::configuration::{build_renew, build_response, build_search};
use em_core::wsc::{self, AUTH_SAE, AUTH_WPA2_PERSONAL, Identity, NetworkSsid};
use em_core::{AgentState, MsgIdCounter, topology};
use em_crypto::rand::OsRandom;
use em_msg::{AL_SAP_BUDGET, CmduHeader, MsgBuilder, RAW_BUDGET, TlvIter, readers};
use em_types::{FreqBand, MacAddr, MsgType, TlvType};
use tokio::sync::mpsc::Receiver;

use crate::platform::{Confirmation, FakePlatform, Platform};
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
    platform: FakePlatform,
    confirmations: Receiver<Confirmation>,
    controller: ScriptedController,
    start: Instant,
    now: Instant,
    frames: Vec<(Duration, Frame)>,
}

impl Harness {
    fn new(profile: u8, radios: Vec<RadioSpec>) -> Self {
        let start = Instant::now();
        let mut platform = FakePlatform::new(Vec::from(radios.as_slice()));
        let confirmations = platform.confirmations();
        Self {
            core: AgentCore::new(profile, radios),
            platform,
            confirmations,
            controller: ScriptedController::new(),
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
        self.deliver_from(CONTROLLER, cmdu);
    }

    /// Delivers a CMDU and, like the actor, hands the platform requests to the fake
    /// platform and its confirmations back to the core.
    fn deliver_from(&mut self, remote: MacAddr, cmdu: &[u8]) {
        let offset = self.now - self.start;
        for frame in self.core.received(remote, cmdu, self.now) {
            self.frames.push((offset, frame));
        }
        for request in self.core.take_platform_requests() {
            self.platform
                .apply_radio_config(request.radio, &request.bss);
        }
        while let Ok(confirmation) = self.confirmations.try_recv() {
            self.core.platform_confirmed(confirmation.bss, self.now);
        }
    }

    fn searches(&self) -> Vec<(Duration, SentSearch)> {
        self.frames
            .iter()
            .filter(|(_, frame)| msg_type_of(&frame.cmdu) == MsgType::AP_AUTOCONFIGURATION_SEARCH)
            .map(|(offset, frame)| (*offset, read_search(frame)))
            .collect()
    }

    /// Every M1 the agent sent, with its offset, in order.
    fn m1_frames(&self) -> Vec<(Duration, &Frame)> {
        self.frames
            .iter()
            .filter(|(_, frame)| msg_type_of(&frame.cmdu) == MsgType::AP_AUTOCONFIGURATION_WSC)
            .map(|(offset, frame)| (*offset, frame))
            .collect()
    }

    /// Lets two orchestrator passes run. A finished stage is destroyed one pass after it
    /// finished and the next stage starts one pass later (`em_orch.cpp:521-572`), so a
    /// follow-up command starts about 500 ms after its trigger.
    fn settle(&mut self) {
        self.run_for(Duration::from_millis(600));
    }

    /// Runs for `span` of virtual time from now.
    fn run_for(&mut self, span: Duration) {
        self.run_to(self.now - self.start + span);
    }

    /// Delivers a Response for a radio's band.
    fn respond(&mut self, band: FreqBand) {
        self.deliver(&response_for(band, 3, 0x7000));
    }

    /// Answers the M1 at `index` with the scripted controller's M2 and delivers it.
    fn answer_m1(&mut self, index: usize) {
        let m1 = Vec::from(self.m1_frames()[index].1.cmdu.as_slice());
        let m2 = self.controller.m2_for(&m1);
        self.deliver(&m2);
    }

    /// Delivers a Topology Query and returns the answer, if any.
    fn query(&mut self) -> Option<Vec<u8>> {
        let query = topology::build_query(&mut self.controller.ids).unwrap();
        let before = self.frames.len();
        self.deliver(&query);
        self.frames
            .get(before)
            .map(|(_, frame)| Vec::from(frame.cmdu.as_slice()))
    }

    fn radio_states(&self) -> Vec<AgentState> {
        [RADIO_24, RADIO_5]
            .iter()
            .map(|ruid| self.state(*ruid))
            .collect()
    }

    /// Both radios to `topo_synchronized` through the whole flow, with every M2 delivered
    /// and one Query. Starts from a registered harness.
    fn onboard_both(&mut self) {
        self.run_for(SECOND);
        self.respond(FreqBand::BAND_2_4_GHZ);
        self.respond(FreqBand::BAND_5_GHZ);
        let first = self.m1_frames().len() - 2;
        self.answer_m1(first);
        self.answer_m1(first + 1);
        self.settle();
        assert!(self.query().is_some());
        self.settle();
        assert_eq!(
            self.radio_states(),
            [AgentState::TopoSynchronized, AgentState::TopoSynchronized]
        );
    }

    fn state(&self, ruid: MacAddr) -> AgentState {
        self.core.nodes().get(radio_key(ruid)).unwrap().state
    }

    fn orch_state(&self, ruid: MacAddr) -> OrchState {
        self.core.orch.orch_state(radio_key(ruid))
    }
}

fn msg_type_of(cmdu: &[u8]) -> MsgType {
    CmduHeader::parse(cmdu).unwrap().0.msg_type
}

fn mid_of(cmdu: &[u8]) -> u16 {
    CmduHeader::parse(cmdu).unwrap().0.msg_id
}

const FRONTHAUL_SSID: &[u8] = b"fronthaul-net";
const BACKHAUL_SSID: &[u8] = b"backhaul-net";
const ALL_BANDS: [FreqBand; 3] = [
    FreqBand::BAND_2_4_GHZ,
    FreqBand::BAND_5_GHZ,
    FreqBand::BAND_6_GHZ,
];

/// Builds the controller's M2 the way the real controller does, from a reset-file-like
/// pair of rows: WPA2 fronthaul and SAE backhaul.
struct ScriptedController {
    ids: MsgIdCounter,
    random: OsRandom,
}

impl ScriptedController {
    fn new() -> Self {
        Self {
            ids: MsgIdCounter::new(),
            random: OsRandom,
        }
    }

    fn m2_for(&mut self, m1_cmdu: &[u8]) -> Vec<u8> {
        let m1 = wsc::parse_m1(m1_cmdu).unwrap();
        let rows = [
            NetworkSsid {
                enabled: true,
                ssid: FRONTHAUL_SSID,
                network_key: b"front-pass",
                bands: &ALL_BANDS,
                haul_types: &[0],
                authentication_type: AUTH_WPA2_PERSONAL,
            },
            NetworkSsid {
                enabled: true,
                ssid: BACKHAUL_SSID,
                network_key: b"back-pass",
                bands: &ALL_BANDS,
                haul_types: &[1],
                authentication_type: AUTH_SAE,
            },
        ];
        let identity = Identity {
            uuid: [7; 16],
            manufacturer: b"controller",
            model: b"model",
            serial: b"1",
            primary_device_type: [0, 6, 0, 0x50, 0xf2, 4, 0, 1],
        };
        wsc::build_m2(&m1, &identity, &rows, &mut self.ids, &mut self.random)
            .unwrap()
            .cmdu
    }
}

/// The M2 with one bit of its first Authenticator flipped.
fn with_bad_authenticator(m2: &[u8]) -> Vec<u8> {
    let (header, area) = CmduHeader::parse(m2).unwrap();
    let mut builder = MsgBuilder::new(&header, AL_SAP_BUDGET);
    let mut flipped = false;
    for item in TlvIter::new(area) {
        let tlv = item.unwrap();
        let mut value = Vec::from(tlv.value);
        if tlv.tlv_type == TlvType::WSC && !flipped {
            *value.last_mut().unwrap() ^= 1;
            flipped = true;
        }
        builder.tlv(tlv.tlv_type, &value);
    }
    builder.finish().unwrap()
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

// intentional-difference:search-until-found
// S1 asserted that the 10 s cancel resets only the last candidate. In S2 the "until found"
// reset also covers `wsc_m2_pending`, so a lost M2 never lets `dev_init` reach its cancel
// (0013 D5): the radios restart discovery under the same `dev_init` instead. The
// single-node cancel stays covered by `em-orch`'s own tests.
#[test]
fn after_both_responses_the_cancel_resets_only_the_last_candidate() {
    let mut harness = Harness::new(3, two_radios());
    harness.register();
    harness.run_to(3 * SECOND);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    assert_eq!(harness.radio_states(), [AgentState::WscM2Pending; 2]);

    // Neither M2 arrives. For 60 s the same `dev_init` stays active, is never cancelled and
    // no second one is submitted.
    let responses_at = harness.now - harness.start;
    let mut searching_again = false;
    for second in 1..=60 {
        harness.run_to(responses_at + second * SECOND);
        assert!(
            harness.core.orch.command(ChainId(0)).is_some(),
            "at {second} s"
        );
        assert!(
            harness.core.orch.command(ChainId(1)).is_none(),
            "at {second} s"
        );
        assert_eq!(harness.orch_state(RADIO_24), OrchState::Progress);
        assert_eq!(harness.orch_state(RADIO_5), OrchState::Progress);
        searching_again |= harness.state(RADIO_5) == AgentState::AutoconfigRspPending;
    }
    assert!(searching_again, "the radios restarted discovery");
    assert!(harness.core.orch.is_type_in_progress(CommandType::DevInit));
    assert!(harness.searches().len() > 4);
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

fn renew_for(band: FreqBand, controller: MacAddr) -> Vec<u8> {
    build_renew(controller, band, 0x60).unwrap()
}

fn registered(radios: Vec<RadioSpec>) -> Harness {
    let mut harness = Harness::new(3, radios);
    harness.register();
    harness
}

#[test]
fn m1_follows_the_response_with_a_new_mid_and_the_al_mac() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_5_GHZ);
    let m1s = harness.m1_frames();
    assert_eq!(m1s.len(), 1);
    let (_, frame) = m1s[0];
    assert_eq!(frame.remote, CONTROLLER);
    let parsed = wsc::parse_m1(&frame.cmdu).unwrap();
    assert_eq!(parsed.radio(), RADIO_5);
    assert_eq!(parsed.band(), FreqBand::BAND_5_GHZ);
    assert_eq!(parsed.enrollee_mac(), AGENT);
    let searches = harness.searches();
    assert!(
        searches
            .iter()
            .all(|(_, search)| search.mid != mid_of(&frame.cmdu))
    );
    assert_ne!(mid_of(&frame.cmdu), 0x7000);
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert!(harness.core.rules.wsc.is_active(radio_key(RADIO_5)));
}

// intentional-difference:onewifi-cb-per-device
// Upstream creates one `onewifi_cb` per radio (`em_agent.cpp:276-277`).
#[test]
fn two_radios_reach_topo_synchronized_with_one_confirmation() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    harness.answer_m1(0);
    assert_eq!(harness.state(RADIO_24), AgentState::OwconfigPending);
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert!(!harness.core.rules.wsc.is_active(radio_key(RADIO_24)));
    // No confirmation yet: a Query is not answered.
    assert!(harness.query().is_none());

    harness.answer_m1(1);
    harness.settle();
    // One confirmation for the device; one onewifi_cb moved both radios.
    assert_eq!(harness.radio_states(), [AgentState::OneWifiBssconfigInd; 2]);
    assert!(
        harness
            .core
            .orch
            .is_type_in_progress(CommandType::OneWifiCb)
    );
    assert!(!harness.core.orch.is_type_in_progress(CommandType::DevInit));
    assert!(
        harness.core.orch.command(ChainId(2)).is_none(),
        "one onewifi_cb only"
    );

    let response = harness.query().expect("every radio is ready");
    let parsed = topology::parse_response(&response, &[FRONTHAUL_SSID, BACKHAUL_SSID], 3).unwrap();
    assert_eq!(parsed.device.al_mac, AGENT);
    assert_eq!(parsed.operational_bss.radios.len(), 2);
    for radio in &parsed.operational_bss.radios {
        let ssids: Vec<&[u8]> = radio.bsses.iter().map(|bss| bss.ssid).collect();
        assert_eq!(ssids, [&b"fronthaul-net\0"[..], &b"backhaul-net\0"[..]]);
    }
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);
    harness.run_for(SECOND);
    assert!(
        !harness
            .core
            .orch
            .is_type_in_progress(CommandType::OneWifiCb)
    );
    assert_eq!(harness.orch_state(RADIO_24), OrchState::Idle);
}

#[test]
fn a_query_is_answered_with_the_query_mid_and_only_once_the_radios_are_ready() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    assert!(harness.query().is_none(), "radios wait for their M2");
    harness.answer_m1(0);
    harness.answer_m1(1);
    harness.settle();
    let query = topology::build_query(&mut MsgIdCounter::new()).unwrap();
    let before = harness.frames.len();
    harness.deliver_from(CONTROLLER, &query);
    let answer = &harness.frames[before].1;
    assert_eq!(answer.remote, CONTROLLER);
    assert_eq!(mid_of(&answer.cmdu), mid_of(&query));
}

#[test]
fn a_rejected_m2_keeps_the_session_and_a_valid_one_is_then_accepted() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_5_GHZ);
    let m1 = Vec::from(harness.m1_frames()[0].1.cmdu.as_slice());
    let valid = harness.controller.m2_for(&m1);

    harness.deliver(&with_bad_authenticator(&valid));
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert!(harness.core.rules.wsc.is_active(radio_key(RADIO_5)));
    assert!(harness.core.take_platform_requests().is_empty());

    // An M2 from another sender is rejected as well.
    harness.deliver_from(OTHER_AGENT, &valid);
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);

    harness.deliver(&valid);
    assert_eq!(harness.state(RADIO_5), AgentState::OwconfigPending);
    assert!(!harness.core.rules.wsc.is_active(radio_key(RADIO_5)));
}

#[test]
fn an_m2_is_accepted_only_in_wsc_m2_pending() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_5_GHZ);
    let m1 = Vec::from(harness.m1_frames()[0].1.cmdu.as_slice());
    let m2 = harness.controller.m2_for(&m1);
    harness.deliver(&m2);
    assert_eq!(harness.state(RADIO_5), AgentState::OwconfigPending);
    // The same M2 again: the radio is no longer waiting for one.
    harness.deliver(&m2);
    assert_eq!(harness.state(RADIO_5), AgentState::OwconfigPending);
    assert!(harness.core.take_platform_requests().is_empty());
}

// intentional-difference:restart-discovery-on-m2-timeout
// Upstream has no M2 timer (`em_configuration.cpp:6781-6784`).
#[test]
fn a_lost_m2_is_recovered_by_the_restart_with_exactly_one_dev_init() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    // The 2.4 GHz M2 arrives, the 5 GHz one is lost.
    harness.answer_m1(0);
    assert_eq!(harness.state(RADIO_24), AgentState::OwconfigPending);

    harness.run_for(12 * SECOND);
    assert_ne!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert!(!harness.core.rules.wsc.is_active(radio_key(RADIO_5)));
    assert_eq!(
        harness.state(RADIO_24),
        AgentState::OwconfigPending,
        "it waits"
    );
    harness.run_for(SECOND);
    assert_eq!(harness.state(RADIO_5), AgentState::AutoconfigRspPending);

    // The search under the same dev_init finds the controller again.
    harness.respond(FreqBand::BAND_5_GHZ);
    assert_eq!(harness.m1_frames().len(), 3);
    harness.answer_m1(2);
    harness.settle();
    assert_eq!(harness.radio_states(), [AgentState::OneWifiBssconfigInd; 2]);
    assert!(harness.query().is_some());
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);
    // chain 0 is the only dev_init; chain 1 is the onewifi_cb.
    assert!(matches!(
        harness.core.orch.command(ChainId(1)),
        Some(em_cmd::Command::OneWifiCb(_))
    ));
    assert!(harness.core.orch.command(ChainId(2)).is_none());
}

#[test]
fn the_restart_boundary_is_more_than_ten_seconds_after_the_hand_off() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_5_GHZ);
    let handed_off = harness.now - harness.start;
    let frames_before = harness.frames.len();

    harness.run_to(handed_off + Duration::from_millis(10_900));
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert!(harness.core.rules.wsc.is_active(radio_key(RADIO_5)));
    assert!(
        harness.frames[frames_before..]
            .iter()
            .all(|(_, frame)| read_search(frame).band == FreqBand::BAND_2_4_GHZ)
    );

    harness.run_to(handed_off + 12 * SECOND);
    assert_ne!(harness.state(RADIO_5), AgentState::WscM2Pending);
    assert!(!harness.core.rules.wsc.is_active(radio_key(RADIO_5)));
}

// intentional-difference:onewifi-cb-timeout-restart
// Upstream resets one radio and has no recovery (`em_orch_agent.cpp:64-71`).
#[test]
fn the_onewifi_cb_timeout_restarts_every_radio_under_a_new_dev_init() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    harness.answer_m1(0);
    harness.answer_m1(1);
    harness.settle();
    assert_eq!(harness.radio_states(), [AgentState::OneWifiBssconfigInd; 2]);

    // No Query arrives. After more than 10 s the onewifi_cb is cancelled.
    harness.run_for(12 * SECOND);
    assert!(
        !harness
            .core
            .orch
            .is_type_in_progress(CommandType::OneWifiCb)
    );
    assert!(harness.core.orch.is_type_in_progress(CommandType::DevInit));
    // chain 0 dev_init, chain 1 onewifi_cb, chain 2 the new dev_init; no chain 3.
    assert!(matches!(
        harness.core.orch.command(ChainId(2)),
        Some(em_cmd::Command::DevInit(_))
    ));
    assert!(harness.core.orch.command(ChainId(3)).is_none());
    for ruid in [RADIO_24, RADIO_5] {
        assert!(!harness.core.rules.wsc.is_active(radio_key(ruid)));
        assert!(matches!(
            harness.state(ruid),
            AgentState::Unconfigured | AgentState::AutoconfigRspPending
        ));
    }
    harness.run_for(2 * SECOND);
    assert_eq!(
        harness.radio_states(),
        [AgentState::AutoconfigRspPending; 2]
    );

    // The flow completes the second time.
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    let first = harness.m1_frames().len() - 2;
    harness.answer_m1(first);
    harness.answer_m1(first + 1);
    harness.settle();
    assert!(harness.query().is_some());
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);
}

/// The states in which a renew starts `cfg_renew` and those in which it is refused
/// (`em_agent.cpp:2438-2447`).
#[test]
fn a_renew_is_refused_in_the_listed_states_and_accepted_otherwise() {
    let refused = [
        AgentState::AutoconfigRenewPending,
        AgentState::WscM2Pending,
        AgentState::WscM2Processed,
    ];
    let accepted = [
        AgentState::Unconfigured,
        AgentState::AutoconfigRspPending,
        AgentState::OwconfigPending,
        AgentState::OneWifiBssconfigInd,
        AgentState::TopoSynchronized,
    ];
    for state in refused.iter().chain(&accepted) {
        let mut harness = registered(two_radios());
        harness.onboard_both();
        harness
            .core
            .nodes
            .get_mut(radio_key(RADIO_5))
            .unwrap()
            .state = *state;
        harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, CONTROLLER));
        let started = harness.core.orch.is_type_in_progress(CommandType::CfgRenew);
        assert_eq!(started, accepted.contains(state), "state {state:?}");
    }
}

#[test]
fn a_renew_is_ignored_for_another_band_controller_or_a_running_cfg_renew() {
    let mut harness = registered(two_radios());
    harness.onboard_both();
    // No 6 GHz radio.
    harness.deliver(&renew_for(FreqBand::BAND_6_GHZ, CONTROLLER));
    assert!(!harness.core.orch.is_type_in_progress(CommandType::CfgRenew));
    // A renew from another controller than the one the radio found.
    harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, OTHER_AGENT));
    assert!(!harness.core.orch.is_type_in_progress(CommandType::CfgRenew));

    harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, CONTROLLER));
    assert!(harness.core.orch.is_type_in_progress(CommandType::CfgRenew));
    // A second renew while the first runs is refused, whatever its band.
    harness.deliver(&renew_for(FreqBand::BAND_2_4_GHZ, CONTROLLER));
    harness.run_for(Duration::from_millis(300));
    assert_eq!(harness.state(RADIO_24), AgentState::TopoSynchronized);
}

#[test]
fn a_renew_reruns_wsc_for_one_radio_and_confirms_again() {
    let mut harness = registered(two_radios());
    harness.onboard_both();
    let m1_count = harness.m1_frames().len();
    harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, CONTROLLER));
    harness.run_for(Duration::from_millis(300));
    assert_eq!(harness.state(RADIO_5), AgentState::AutoconfigRenewPending);
    assert!(!harness.core.rules.wsc.is_active(radio_key(RADIO_5)));
    assert_eq!(harness.state(RADIO_24), AgentState::TopoSynchronized);
    // No Response is needed: M1 leaves from the proto tick.
    assert_eq!(harness.m1_frames().len(), m1_count);
    harness.run_for(SECOND);
    assert_eq!(harness.m1_frames().len(), m1_count + 1);
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);

    harness.answer_m1(m1_count);
    harness.settle();
    // cfg_renew finished at owconfig_pending and the platform confirmed again.
    assert_eq!(harness.state(RADIO_5), AgentState::OneWifiBssconfigInd);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::CfgRenew));
    assert!(harness.query().is_some());
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);
}

// intentional-difference:renew-timeout-restart
// Upstream's radio stays `unconfigured` (`em_orch_agent.cpp:64-71`).
#[test]
fn a_renew_followed_by_a_lost_m2_restarts_discovery_with_one_new_dev_init() {
    let mut harness = registered(two_radios());
    harness.onboard_both();
    let m1_count = harness.m1_frames().len();
    harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, CONTROLLER));
    harness.run_for(2 * SECOND);
    assert_eq!(harness.m1_frames().len(), m1_count + 1);
    assert_eq!(harness.state(RADIO_5), AgentState::WscM2Pending);

    // The M2 is lost. The radio gives up and a new dev_init opens the search gate.
    harness.run_for(20 * SECOND);
    assert!(harness.core.orch.is_type_in_progress(CommandType::DevInit));
    assert!(!harness.core.orch.is_type_in_progress(CommandType::CfgRenew));
    // chain 0 dev_init, 1 onewifi_cb, 2 cfg_renew, 3 the recovery dev_init; no chain 4.
    assert!(matches!(
        harness.core.orch.command(ChainId(3)),
        Some(em_cmd::Command::DevInit(_))
    ));
    assert!(harness.core.orch.command(ChainId(4)).is_none());
    assert_eq!(harness.state(RADIO_5), AgentState::AutoconfigRspPending);

    // The search finds the controller and the radio completes.
    harness.respond(FreqBand::BAND_5_GHZ);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    let first = harness.m1_frames().len() - 2;
    harness.answer_m1(first);
    harness.answer_m1(first + 1);
    harness.settle();
    assert!(harness.query().is_some());
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);
}

/// 0013 D4: the controller sends one renew per radio back to back; the agent refuses the
/// second while the first `cfg_renew` runs, and the `onewifi_cb` restart completes it.
#[test]
fn two_renews_from_the_controller_timeout_complete_through_the_onewifi_cb_restart() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    harness.answer_m1(0);
    harness.answer_m1(1);
    harness.settle();
    assert_eq!(harness.radio_states(), [AgentState::OneWifiBssconfigInd; 2]);

    // The controller sends no Query (its second radio node is misconfigured) and renews
    // both radios at its 40 s timeout.
    harness.run_for(Duration::from_millis(500));
    harness.deliver(&renew_for(FreqBand::BAND_2_4_GHZ, CONTROLLER));
    harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, CONTROLLER));
    assert!(harness.core.orch.is_type_in_progress(CommandType::CfgRenew));

    // The onewifi_cb times out and the restart brings both radios back through discovery.
    harness.run_for(14 * SECOND);
    assert!(harness.core.orch.is_type_in_progress(CommandType::DevInit));
    // chain 0 dev_init, 1 onewifi_cb, 2 the one accepted cfg_renew, 3 the restart dev_init.
    // The restart cancelled the cfg_renew (S2-ADV-01), so its chain is gone.
    assert!(harness.core.orch.command(ChainId(2)).is_none());
    assert!(matches!(
        harness.core.orch.command(ChainId(3)),
        Some(em_cmd::Command::DevInit(_))
    ));

    // The scripted controller answers whatever the agent does until it is synchronized.
    let mut answered = harness.m1_frames().len();
    for _ in 0..120 {
        for band in [FreqBand::BAND_2_4_GHZ, FreqBand::BAND_5_GHZ] {
            let ruid = if band == FreqBand::BAND_2_4_GHZ {
                RADIO_24
            } else {
                RADIO_5
            };
            if harness.state(ruid) == AgentState::AutoconfigRspPending {
                harness.respond(band);
            }
        }
        while answered < harness.m1_frames().len() {
            harness.answer_m1(answered);
            answered += 1;
        }
        if harness
            .radio_states()
            .iter()
            .all(|state| *state >= AgentState::OneWifiBssconfigInd)
            && harness.query().is_some()
        {
            break;
        }
        harness.run_for(SECOND);
    }
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);
}

/// One second of the scripted controller: it answers a Search with a Response and an M1
/// with an M2, and sends a Query only when every radio waits for one, as the real
/// controller does for a device it has not yet seen synchronized. Returns whether a Query
/// was answered.
fn controller_second(harness: &mut Harness, answered_m1s: &mut usize) -> bool {
    for (band, ruid) in [
        (FreqBand::BAND_2_4_GHZ, RADIO_24),
        (FreqBand::BAND_5_GHZ, RADIO_5),
    ] {
        if harness.state(ruid) == AgentState::AutoconfigRspPending {
            harness.respond(band);
        }
    }
    while *answered_m1s < harness.m1_frames().len() {
        harness.answer_m1(*answered_m1s);
        *answered_m1s += 1;
    }
    let all_waiting = harness
        .radio_states()
        .iter()
        .all(|state| *state == AgentState::OneWifiBssconfigInd);
    let queried = all_waiting && harness.query().is_some();
    harness.run_for(SECOND);
    queried
}

// intentional-difference:onewifi-cb-timeout-restart
// S2-ADV-01: the restart drops the cfg_renew that the first renew started. The second renew
// is refused while that cfg_renew is in progress. Upstream has no recovery
// (`em_orch_agent.cpp:64-71`).
#[test]
fn the_onewifi_cb_restart_leaves_no_stale_cfg_renew_after_synchronization() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    harness.respond(FreqBand::BAND_2_4_GHZ);
    harness.respond(FreqBand::BAND_5_GHZ);
    harness.answer_m1(0);
    harness.answer_m1(1);
    harness.settle();
    harness.run_for(Duration::from_millis(500));
    harness.deliver(&renew_for(FreqBand::BAND_2_4_GHZ, CONTROLLER));
    harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, CONTROLLER));
    harness.run_for(14 * SECOND);

    let mut answered = harness.m1_frames().len();
    let mut synchronized = false;
    for _ in 0..120 {
        if controller_second(&mut harness, &mut answered) {
            synchronized = true;
            break;
        }
    }
    assert!(synchronized, "the restart completes onboarding");
    harness.settle();
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);

    // The real controller never Queries again: its other radio node is synchronized.
    let m1_count = harness.m1_frames().len();
    let search_count = harness.searches().len();
    for _ in 0..60 {
        let queried = controller_second(&mut harness, &mut answered);
        assert!(!queried, "no radio waits for a Query");
        assert!(!harness.core.orch.is_type_in_progress(CommandType::CfgRenew));
        assert!(!harness.core.orch.is_type_in_progress(CommandType::DevInit));
        assert!(
            !harness
                .core
                .orch
                .is_type_in_progress(CommandType::OneWifiCb)
        );
    }
    assert_eq!(harness.m1_frames().len(), m1_count, "no stale M1");
    assert_eq!(harness.searches().len(), search_count, "no new dev_init");
    assert_eq!(harness.radio_states(), [AgentState::TopoSynchronized; 2]);
}

/// A Renew accepted while the radios still search is queued behind `dev_init`
/// (`em_agent.cpp:2400-2447`). Nothing has restarted discovery, so upstream's behaviour
/// stands: the `cfg_renew` stays queued and runs once `dev_init` is done with the radio.
#[test]
fn a_renew_queued_behind_dev_init_still_runs_when_no_recovery_is_involved() {
    let mut harness = registered(two_radios());
    harness.run_for(SECOND);
    assert_eq!(
        harness.radio_states(),
        [AgentState::AutoconfigRspPending; 2]
    );
    harness.deliver(&renew_for(FreqBand::BAND_5_GHZ, CONTROLLER));
    assert!(harness.core.orch.is_type_in_progress(CommandType::CfgRenew));
    assert!(harness.core.orch.is_type_in_progress(CommandType::DevInit));

    // The searching radios onboard under dev_init; the queued cfg_renew has not started.
    let mut answered = 0;
    controller_second(&mut harness, &mut answered);
    assert_eq!(harness.m1_frames().len(), 2);
    assert!(harness.core.orch.is_type_in_progress(CommandType::CfgRenew));
    assert_eq!(harness.state(RADIO_5), AgentState::OneWifiBssconfigInd);

    // dev_init is done with the radios, then the cfg_renew sends a third M1 for the
    // renewed radio alone, and no recovery dev_init is submitted.
    controller_second(&mut harness, &mut answered);
    let m1s = harness.m1_frames();
    assert_eq!(m1s.len(), 3);
    assert_eq!(wsc::parse_m1(&m1s[2].1.cmdu).unwrap().radio(), RADIO_5);
    assert!(!harness.core.orch.is_type_in_progress(CommandType::DevInit));
    assert_eq!(harness.state(RADIO_24), AgentState::TopoSynchronized);
}
