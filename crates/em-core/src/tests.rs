// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use em_cmd::{
    ActiveOp, ChainId, Command, CommandType, CommandView, DevInit, NoCommands, NodeKey, NodeKind,
    OrchState,
};
use em_msg::{CmduHeader, MsgBuilder, RAW_BUDGET, ViolationKind, readers, validate};
use em_types::{FreqBand, MacAddr, MsgType, ProfileType, ServiceType, TlvType};

use crate::configuration::{self, Error};
use crate::{
    AgentState, ControllerPeer, CtrlState, EngineCtx, MsgIdCounter, Node, NodeTable, Output,
    Outputs,
};

const AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
const CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
const RADIO: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 1]);
const BAND: FreqBand = FreqBand::BAND_5_GHZ;

fn key(mac: MacAddr, kind: NodeKind) -> NodeKey {
    NodeKey { mac, kind }
}

struct View {
    state: OrchState,
    active: Option<ActiveOp>,
    command: Option<Command>,
}
impl View {
    fn active() -> Self {
        Self {
            state: OrchState::Progress,
            active: Some(ActiveOp {
                chain: ChainId(7),
                stage: 1,
                kind: CommandType::DevInit,
            }),
            command: Some(Command::DevInit(DevInit {
                al_mac: AGENT,
                radios: Vec::new(),
            })),
        }
    }
}
impl CommandView for View {
    fn command(&self, chain: ChainId) -> Option<&Command> {
        if chain == ChainId(7) {
            self.command.as_ref()
        } else {
            None
        }
    }
    fn orch_state(&self, _: NodeKey) -> OrchState {
        self.state
    }
    fn active(&self, _: NodeKey) -> Option<ActiveOp> {
        self.active
    }
}

struct Agent {
    nodes: NodeTable<AgentState>,
    key: NodeKey,
    ids: MsgIdCounter,
    out: Outputs,
    view: View,
}
impl Agent {
    fn new(state: AgentState, profile: u8) -> Self {
        let key = key(RADIO, NodeKind::Radio);
        let mut nodes = NodeTable::new();
        nodes.insert(Node::new(key, state, profile, Some(BAND)));
        Self {
            nodes,
            key,
            ids: MsgIdCounter::new(),
            out: Outputs::new(),
            view: View::active(),
        }
    }
    fn ctx(&mut self) -> EngineCtx<'_, AgentState> {
        EngineCtx {
            nodes: &mut self.nodes,
            key: self.key,
            commands: &self.view,
            msg_ids: &mut self.ids,
            now: Instant::now(),
            out: &mut self.out,
        }
    }
    fn tick(&mut self) -> Result<(), Error> {
        configuration::agent_proto_tick(&mut self.ctx())
    }
    fn response(&mut self, cmdu: &[u8]) -> Result<(), Error> {
        configuration::handle_response(&mut self.ctx(), CONTROLLER, cmdu)
    }
    fn state(&self) -> AgentState {
        self.nodes.get(self.key).unwrap().state
    }
}

struct Controller {
    nodes: NodeTable<CtrlState>,
    key: NodeKey,
    ids: MsgIdCounter,
    out: Outputs,
}
impl Controller {
    fn new() -> Self {
        let key = key(CONTROLLER, NodeKind::Al);
        let mut nodes = NodeTable::new();
        nodes.insert(Node::new(key, CtrlState::Unconfigured, 3, None));
        Self {
            nodes,
            key,
            ids: MsgIdCounter::new(),
            out: Outputs::new(),
        }
    }
    fn search(&mut self, cmdu: &[u8]) -> Result<(), Error> {
        let mut ctx = EngineCtx {
            nodes: &mut self.nodes,
            key: self.key,
            commands: &NoCommands,
            msg_ids: &mut self.ids,
            now: Instant::now(),
            out: &mut self.out,
        };
        configuration::handle_search(&mut ctx, cmdu)
    }
}

fn test_header(msg_type: MsgType, id: u16) -> CmduHeader {
    CmduHeader {
        msg_type,
        msg_id: id,
        frag_id: 0,
        last_fragment: true,
        relay: msg_type == MsgType::AP_AUTOCONFIGURATION_SEARCH,
    }
}
fn search(
    profile: Option<&[u8]>,
    role: Option<u8>,
    supported: Option<&[u8]>,
    searched: Option<&[u8]>,
) -> Vec<u8> {
    let mut b = MsgBuilder::new(
        &test_header(MsgType::AP_AUTOCONFIGURATION_SEARCH, 0x1234),
        RAW_BUDGET,
    );
    b.tlv(TlvType::AL_MAC_ADDRESS, &AGENT.octets());
    if let Some(role) = role {
        b.tlv(TlvType::SEARCHED_ROLE, &[role]);
    }
    b.tlv(TlvType::AUTOCONFIG_FREQ_BAND, &[u8::from(BAND)]);
    if let Some(value) = supported {
        b.tlv(TlvType::SUPPORTED_SERVICE, value);
    }
    if let Some(value) = searched {
        b.tlv(TlvType::SEARCHED_SERVICE, value);
    }
    if let Some(value) = profile {
        b.tlv(TlvType::MULTI_AP_PROFILE, value);
    }
    b.finish().unwrap()
}
fn response(profile: Option<&[u8]>, security: bool, id: u16) -> Vec<u8> {
    let mut b = MsgBuilder::new(
        &test_header(MsgType::AP_AUTOCONFIGURATION_RESPONSE, id),
        RAW_BUDGET,
    );
    b.tlv(TlvType::SUPPORTED_ROLE, &[0])
        .tlv(TlvType::SUPPORTED_FREQ_BAND, &[u8::from(BAND)]);
    if security {
        b.tlv(TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY, &[0, 0, 0]);
    }
    if let Some(value) = profile {
        b.tlv(TlvType::MULTI_AP_PROFILE, value);
    }
    b.tlv(TlvType::CONTROLLER_CAPABILITY, &[0xc0]);
    b.finish().unwrap()
}
fn frame(events: &[Output], index: usize) -> (MacAddr, &[u8]) {
    match &events[index] {
        Output::Frame { remote, cmdu } => (*remote, cmdu),
        Output::Progress(_) => panic!("expected frame"),
    }
}
fn profile_value(cmdu: &[u8]) -> Option<&[u8]> {
    let (_, area) = CmduHeader::parse(cmdu).unwrap();
    readers::find_tlv(area, TlvType::MULTI_AP_PROFILE).unwrap()
}

#[test]
fn search_golden_bytes_follow_upstream_order() {
    // em_configuration.cpp:4546-4587: header, AL MAC, searched role,
    // band, supported service, searched service, profile, then EOM.
    let bytes = configuration::build_search(AGENT, BAND, 2, 0x1234).unwrap();
    assert_eq!(
        bytes,
        [
            0, 0, 0, 7, 0x12, 0x34, 0, 0xc0, 0x01, 0, 6, 2, 0, 0, 0, 0, 1, 0x0d, 0, 1, 0, 0x0e, 0,
            1, 1, 0x80, 0, 2, 1, 1, 0x81, 0, 2, 1, 0, 0xb3, 0, 1, 2, 0, 0, 0,
        ]
    );
    assert!(validate(&bytes, ProfileType::MULTI_AP_PROFILE_3).is_ok());
}

#[test]
fn response_golden_bytes_follow_upstream_order_and_s1_capabilities() {
    // em_configuration.cpp:4399-4462,4492-4495: role, band, service,
    // security, profile, controller capability, then EOM.
    let bytes = configuration::build_response(BAND, 2, 0x1234).unwrap();
    assert_eq!(
        bytes,
        [
            0, 0, 0, 8, 0x12, 0x34, 0, 0x80, 0x0f, 0, 1, 0, 0x10, 0, 1, 1, 0x80, 0, 2, 1, 0, 0xa9,
            0, 3, 0, 0, 0, 0xb3, 0, 1, 2, 0xdd, 0, 1, 0xc0, 0, 0, 0,
        ]
    );
    assert!(validate(&bytes, ProfileType::MULTI_AP_PROFILE_2).is_ok());
}

#[test]
fn node_table_preserves_order_on_replace_and_remove_and_disambiguates_kinds() {
    let al = key(AGENT, NodeKind::Al);
    let same_radio = key(AGENT, NodeKind::Radio);
    let other = key(RADIO, NodeKind::Radio);
    let mut nodes = NodeTable::new();
    for k in [al, same_radio, other] {
        assert!(
            nodes
                .insert(Node::new(k, AgentState::Unconfigured, 3, None))
                .is_none()
        );
    }
    assert!(
        nodes
            .insert(Node::new(
                same_radio,
                AgentState::WscM2Pending,
                2,
                Some(BAND)
            ))
            .is_some()
    );
    assert_eq!(
        nodes.iter().map(|n| n.key).collect::<Vec<_>>(),
        [al, same_radio, other]
    );
    assert_eq!(nodes.remove(al).unwrap().key, al);
    assert_eq!(
        nodes.iter().map(|n| n.key).collect::<Vec<_>>(),
        [same_radio, other]
    );
}

#[test]
fn message_counter_starts_at_one_and_wraps_without_zero() {
    let mut counter = MsgIdCounter::new();
    assert_eq!(counter.next_id(), 1);
    counter.0 = u16::MAX - 1;
    assert_eq!(counter.next_id(), u16::MAX);
    assert_eq!(counter.next_id(), 1);
    assert_eq!(counter.next_id(), 2);
}

#[test]
fn search_gate_requires_progress_active_command_radio_and_state_range() {
    let mut agent = Agent::new(AgentState::Unconfigured, 3);
    assert!(configuration::search_gate(&agent.ctx()));
    for state in [
        OrchState::None,
        OrchState::Idle,
        OrchState::Pending,
        OrchState::Fini,
    ] {
        agent.view.state = state;
        assert!(!configuration::search_gate(&agent.ctx()));
        agent.tick().unwrap();
    }
    agent.view.state = OrchState::Progress;
    agent.view.active = None;
    assert!(!configuration::search_gate(&agent.ctx()));
    agent.view.active = View::active().active;
    agent.view.command = None;
    assert!(!configuration::search_gate(&agent.ctx()));
    agent.view.command = View::active().command;
    agent.nodes.get_mut(agent.key).unwrap().state = AgentState::Configured;
    assert!(!configuration::search_gate(&agent.ctx()));
    agent.nodes.get_mut(agent.key).unwrap().state = AgentState::TopologyNotify;
    assert!(!configuration::search_gate(&agent.ctx()));
    agent.key = key(AGENT, NodeKind::Al);
    agent
        .nodes
        .insert(Node::new(agent.key, AgentState::Unconfigured, 3, None));
    assert!(!configuration::search_gate(&agent.ctx()));
    assert_eq!(agent.out.events, []);
}

#[test]
fn tick_resends_with_new_ids_and_stops_after_any_mid_response() {
    // intentional-difference:state-on-handoff
    // Upstream em_configuration.cpp:6727-6733 and :6378-6391 waits for send success.
    let mut agent = Agent::new(AgentState::Unconfigured, 2);
    agent.tick().unwrap();
    assert_eq!(agent.state(), AgentState::AutoconfigRspPending);
    assert_eq!(agent.out.events[1], Output::Progress(agent.key));
    agent.tick().unwrap();
    let (remote, first) = frame(&agent.out.events, 0);
    assert_eq!(remote, MacAddr::IEEE1905_MULTICAST);
    assert_eq!(CmduHeader::parse(first).unwrap().0.msg_id, 1);
    assert_eq!(profile_value(first), Some(&[2][..]));
    let (_, second) = frame(&agent.out.events, 2);
    assert_eq!(CmduHeader::parse(second).unwrap().0.msg_id, 2);
    agent.response(&response(Some(&[2]), true, 0xbeef)).unwrap();
    assert_eq!(agent.state(), AgentState::WscM2Pending);
    assert_eq!(
        agent.nodes.get(agent.key).unwrap().controller,
        Some(ControllerPeer {
            al_mac: CONTROLLER,
            profile: 2
        })
    );
    assert_eq!(agent.out.events[3], Output::Progress(agent.key));
    agent.tick().unwrap();
    assert_eq!(agent.out.events.len(), 4);
}

#[test]
fn response_only_advances_pending_radio() {
    for state in [
        AgentState::Unconfigured,
        AgentState::WscM2Pending,
        AgentState::Configured,
    ] {
        let mut agent = Agent::new(state, 3);
        agent.response(&response(Some(&[3]), true, 1)).unwrap();
        assert_eq!(agent.state(), state);
        assert_eq!(agent.out.events, []);
        assert!(agent.nodes.get(agent.key).unwrap().controller.is_none());
    }
}

#[test]
fn response_routes_to_first_matching_radio_excluding_upstream_states() {
    let mut nodes = NodeTable::new();
    nodes.insert(Node::new(
        key(AGENT, NodeKind::Al),
        AgentState::Unconfigured,
        3,
        Some(BAND),
    ));
    for (index, state) in [
        AgentState::AutoconfigRenewPending,
        AgentState::WscM2Pending,
        AgentState::OwconfigPending,
        AgentState::Ieee1905Securing,
        AgentState::WscM2Processed,
    ]
    .into_iter()
    .enumerate()
    {
        let mut mac = [2, 0, 0, 0, 2, 0];
        mac[5] = u8::try_from(index).unwrap();
        nodes.insert(Node::new(
            key(MacAddr::new(mac), NodeKind::Radio),
            state,
            3,
            Some(BAND),
        ));
    }
    let wrong = key(MacAddr::new([2, 0, 0, 0, 3, 0]), NodeKind::Radio);
    nodes.insert(Node::new(
        wrong,
        AgentState::Unconfigured,
        3,
        Some(FreqBand::BAND_6_GHZ),
    ));
    let first = key(RADIO, NodeKind::Radio);
    let next = key(MacAddr::new([2, 0, 0, 0, 3, 1]), NodeKind::Radio);
    nodes.insert(Node::new(first, AgentState::Unconfigured, 3, Some(BAND)));
    nodes.insert(Node::new(
        next,
        AgentState::AutoconfigRspPending,
        3,
        Some(BAND),
    ));
    let bytes = response(Some(&[3]), true, 1);
    assert_eq!(
        configuration::route_response(&nodes, &bytes).unwrap(),
        Some(first)
    );
    nodes.get_mut(first).unwrap().state = AgentState::WscM2Pending;
    assert_eq!(
        configuration::route_response(&nodes, &bytes).unwrap(),
        Some(next)
    );
    nodes.get_mut(next).unwrap().state = AgentState::OwconfigPending;
    assert_eq!(configuration::route_response(&nodes, &bytes).unwrap(), None);
}

#[test]
fn invalid_search_is_still_routable_before_handler_validation() {
    let bytes = search(
        Some(&[9]),
        None,
        Some(&[2, 1, u8::from(ServiceType::EMPLUS_AGENT)]),
        None,
    );
    let route = configuration::route_search(&bytes).unwrap();
    assert_eq!(route.al_mac, AGENT);
    assert_eq!(route.band, BAND);
    // em_msg.cpp:145-148 and em_ctrl.cpp:1219-1222 default out-of-range profiles.
    assert_eq!(route.profile, 1);
    assert!(route.is_emplus_agent);
    let mut ctrl = Controller::new();
    assert!(matches!(ctrl.search(&bytes), Err(Error::Validation(_))));
    assert_eq!(ctrl.out.events, []);
    assert_eq!(
        ctrl.nodes.get(ctrl.key).unwrap().state,
        CtrlState::Unconfigured
    );
}

#[test]
fn response_profile_comes_from_each_current_search() {
    // intentional-difference:response-profile-from-search
    // em_configuration.cpp:4444-4448 always uses profile 3.
    // intentional-difference:state-on-handoff
    // em_configuration.cpp:6471-6478 changes state only after send_frame succeeds.
    let mut ctrl = Controller::new();
    for profile in [Some(&[2][..]), Some(&[3][..]), None, Some(&[1][..])] {
        let bytes = search(profile, Some(0), Some(&[1, 1]), Some(&[1, 0]));
        let before = ctrl.out.events.len();
        ctrl.search(&bytes).unwrap();
        let (remote, reply) = frame(&ctrl.out.events, before);
        assert_eq!(remote, AGENT);
        assert_eq!(profile_value(reply), Some(profile.unwrap_or(&[1])));
        assert_eq!(CmduHeader::parse(reply).unwrap().0.msg_id, 0x1234);
        assert!(validate(reply, ProfileType::from(profile.map_or(1, |p| p[0]))).is_ok());
        assert_eq!(
            ctrl.nodes.get(ctrl.key).unwrap().state,
            CtrlState::WscM1Pending
        );
    }
    assert_eq!(ctrl.out.events[1], Output::Progress(ctrl.key));
}

#[test]
fn reserved_search_profile_assumes_controller_profile_but_routing_uses_upstream_fallback() {
    // intentional-difference:reserved-profile-assume-own
    // em_configuration.cpp:6409-6418 falls back to profile 1.
    // em_msg.cpp:145-148 and em_ctrl.cpp:1219-1222 store 0 but default 4+ to 1.
    for (raw, routed) in [(0, 0), (4, 1), (255, 1)] {
        for value in [&[raw][..], &[raw, 9, 8, 7][..]] {
            let bytes = search(Some(value), Some(0), None, None);
            assert_eq!(configuration::route_search(&bytes).unwrap().profile, routed);
            let mut ctrl = Controller::new();
            ctrl.search(&bytes).unwrap();
            assert_eq!(profile_value(frame(&ctrl.out.events, 0).1), Some(&[3][..]));
        }
    }
}

#[test]
fn search_routing_defaults_absent_malformed_and_out_of_range_profiles() {
    // em_msg.cpp:131-152,154-175 and em_ctrl.cpp:1219-1222.
    for raw in 0..=u8::MAX {
        for value in [&[raw][..], &[raw, 9, 8, 7][..]] {
            let bytes = search(Some(value), Some(0), None, None);
            let expected = if raw >= 4 { 1 } else { raw };
            assert_eq!(
                configuration::route_search(&bytes).unwrap().profile,
                expected
            );
        }
    }
    for value in [
        None,
        Some(&[][..]),
        Some(&[2, 3][..]),
        Some(&[2, 3, 4][..]),
        Some(&[2, 3, 4, 5, 6][..]),
    ] {
        let bytes = search(value, Some(0), None, None);
        assert_eq!(configuration::route_search(&bytes).unwrap().profile, 1);
    }
}

#[test]
fn response_sets_kibmib_and_early_capability_and_zero_security() {
    // intentional-difference:kibmib-set
    // em_configuration.cpp:4455-4458 sets only Early_AP_Capability.
    let mut ctrl = Controller::new();
    ctrl.search(&search(Some(&[2]), Some(0), None, None))
        .unwrap();
    let (_, area) = CmduHeader::parse(frame(&ctrl.out.events, 0).1).unwrap();
    assert_eq!(
        readers::find_tlv(area, TlvType::CONTROLLER_CAPABILITY).unwrap(),
        Some(&[0xc0][..])
    );
    assert_eq!(
        readers::find_tlv(area, TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY).unwrap(),
        Some(&[0, 0, 0][..])
    );
}

#[test]
fn every_valid_search_gets_controller_service_even_when_roles_differ() {
    // Upstream em_configuration.cpp:4416-4432 includes SupportedService on every Response;
    // 0012 preserves upstream despite the spec's conditional requirement.
    for (role, searched) in [(7, Some(&[1, 9][..])), (0, None), (0, Some(&[1, 0][..]))] {
        let bytes = search(Some(&[2]), Some(role), None, searched);
        let mut ctrl = Controller::new();
        ctrl.search(&bytes).unwrap();
        let (_, area) = CmduHeader::parse(frame(&ctrl.out.events, 0).1).unwrap();
        assert_eq!(
            readers::find_tlv(area, TlvType::SUPPORTED_SERVICE).unwrap(),
            Some(&[1, 0][..])
        );
    }
}

#[test]
fn response_without_profile_is_rejected_for_every_local_profile() {
    // intentional-difference:response-profile-required
    // em_configuration.cpp:6297-6304 and em_msg.cpp:719-729 used profile 1
    // when absent, where upstream's table does not require the TLV.
    for local in [1, 2, 3] {
        let mut agent = Agent::new(AgentState::AutoconfigRspPending, local);
        let err = agent.response(&response(None, true, 7)).unwrap_err();
        let Error::Validation(em_msg::ValidateError::Table(rows)) = err else {
            panic!("expected table error");
        };
        assert!(
            rows.iter()
                .any(|row| row.tlv_type == TlvType::MULTI_AP_PROFILE
                    && row.kind == ViolationKind::Missing)
        );
        assert_eq!(agent.state(), AgentState::AutoconfigRspPending);
        assert_eq!(agent.out.events, []);
    }
}

#[test]
fn malformed_response_profile_is_rejected_without_changing_pending_state() {
    // em_msg.cpp:131-152 and em_configuration.cpp:6304-6317 reject malformed
    // profiles; D6 still requires the Profile TLV when it is absent.
    for local in [1, 2, 3] {
        for value in [&[2, 3][..], &[][..], &[2, 3, 4][..], &[2, 3, 4, 5, 6][..]] {
            let mut agent = Agent::new(AgentState::AutoconfigRspPending, local);
            let error = agent.response(&response(Some(value), true, 7)).unwrap_err();
            assert_eq!(
                error,
                Error::Reader(em_msg::ReaderError::InvalidLength {
                    tlv_type: TlvType::MULTI_AP_PROFILE,
                    len: value.len(),
                })
            );
            assert_eq!(agent.state(), AgentState::AutoconfigRspPending);
            assert!(agent.nodes.get(agent.key).unwrap().controller.is_none());
            assert_eq!(agent.out.events, []);
        }
    }
}

#[test]
fn response_with_broken_tlv_framing_keeps_the_validation_error() {
    let mut bytes = response(Some(&[2]), true, 7);
    bytes[9] = u8::MAX; // The first TLV claims a length past the end of the CMDU.
    let mut agent = Agent::new(AgentState::AutoconfigRspPending, 2);
    assert!(matches!(
        agent.response(&bytes),
        Err(Error::Validation(em_msg::ValidateError::Framing(_)))
    ));
    assert_eq!(agent.state(), AgentState::AutoconfigRspPending);
    assert!(agent.nodes.get(agent.key).unwrap().controller.is_none());
    assert_eq!(agent.out.events, []);
}

#[test]
fn validation_overrides_allow_legacy_search_and_require_response_security() {
    // intentional-difference:search-profile-optional
    // em_msg.cpp:719-720 requires Profile above 1 and forbids it at 1.
    // intentional-difference:response-security-cap-required
    // em_msg.cpp:726 forbids Security Capability below profile 3.
    // intentional-difference:response-profile-required
    // em_msg.cpp:728 forbids Profile at 1 rather than requiring it.
    for p in [1, 2, 3] {
        let profile = ProfileType::from(p);
        assert!(validate(&search(None, Some(0), None, None), profile).is_ok());
        assert!(validate(&search(Some(&[p]), Some(0), None, None), profile).is_ok());
        assert!(validate(&response(Some(&[p]), true, 1), profile).is_ok());
        assert!(validate(&response(Some(&[p]), false, 1), profile).is_err());
    }
}

#[test]
fn reserved_response_profile_assumes_receiving_agents_own_profile() {
    // intentional-difference:reserved-response-profile-assume-own
    // em_configuration.cpp:6297-6304 instead falls back to profile 1.
    for own in [2, 3] {
        for raw in [0, 4, 255] {
            for value in [&[raw][..], &[raw, 9, 8, 7][..]] {
                let mut agent = Agent::new(AgentState::AutoconfigRspPending, own);
                agent.response(&response(Some(value), true, 42)).unwrap();
                assert_eq!(
                    agent
                        .nodes
                        .get(agent.key)
                        .unwrap()
                        .controller
                        .unwrap()
                        .profile,
                    own
                );
                assert_eq!(agent.state(), AgentState::WscM2Pending);
            }
        }
    }
}

#[test]
fn response_routing_precedes_validation_and_rejection_keeps_state() {
    let bytes = response(Some(&[2]), false, 0x1010);
    let mut agent = Agent::new(AgentState::AutoconfigRspPending, 2);
    assert_eq!(
        configuration::route_response(&agent.nodes, &bytes).unwrap(),
        Some(agent.key)
    );
    let error = agent.response(&bytes).unwrap_err();
    let Error::Validation(em_msg::ValidateError::Table(rows)) = error else {
        panic!("expected table error");
    };
    assert!(rows.iter().any(
        |row| row.tlv_type == TlvType::IEEE1905_LAYER_SECURITY_CAPABILITY
            && row.kind == ViolationKind::Missing
    ));
    assert_eq!(agent.state(), AgentState::AutoconfigRspPending);
    assert!(agent.nodes.get(agent.key).unwrap().controller.is_none());
    assert_eq!(agent.out.events, []);
}

#[test]
fn four_octet_profile_and_unreadable_optional_service_follow_reader_rules() {
    let bytes = search(Some(&[2, 9, 8, 7]), Some(0), Some(&[2, 1]), None);
    let route = configuration::route_search(&bytes).unwrap();
    assert_eq!(route.profile, 2);
    assert!(!route.is_emplus_agent);
    let mut ctrl = Controller::new();
    ctrl.search(&bytes).unwrap();
    assert_eq!(profile_value(frame(&ctrl.out.events, 0).1), Some(&[2][..]));
    let mut agent = Agent::new(AgentState::AutoconfigRspPending, 3);
    agent
        .response(&response(Some(&[2, 0, 0, 0]), true, 1))
        .unwrap();
    assert_eq!(
        agent
            .nodes
            .get(agent.key)
            .unwrap()
            .controller
            .unwrap()
            .profile,
        2
    );
}

#[test]
fn output_from_controller_can_be_routed_and_accepted_by_agent() {
    let mut agent = Agent::new(AgentState::Unconfigured, 2);
    let mut ctrl = Controller::new();
    agent.tick().unwrap();
    ctrl.search(frame(&agent.out.events, 0).1).unwrap();
    let (destination, reply) = frame(&ctrl.out.events, 0);
    assert_eq!(destination, AGENT);
    assert_eq!(
        configuration::route_response(&agent.nodes, reply).unwrap(),
        Some(agent.key)
    );
    agent.response(reply).unwrap();
    assert_eq!(agent.state(), AgentState::WscM2Pending);
    assert_eq!(
        agent.nodes.get(agent.key).unwrap().controller,
        Some(ControllerPeer {
            al_mac: CONTROLLER,
            profile: 2
        })
    );
}

#[test]
fn controller_routes_readable_band_from_invalid_search_before_validation() {
    // src/ctrl/em_ctrl.cpp:1197-1203 uses get_freq_band, which finds either
    // band TLV in frame order (src/em/em_msg.cpp:346-363).
    let mut builder = MsgBuilder::new(
        &test_header(MsgType::AP_AUTOCONFIGURATION_SEARCH, 1),
        RAW_BUDGET,
    );
    builder
        .tlv(TlvType::AL_MAC_ADDRESS, &AGENT.octets())
        .tlv(TlvType::SUPPORTED_FREQ_BAND, &[u8::from(BAND)])
        .tlv(TlvType::SEARCHED_ROLE, &[0]);
    let bytes = builder.finish().unwrap();
    assert_eq!(configuration::route_search(&bytes).unwrap().band, BAND);
    let mut ctrl = Controller::new();
    assert!(matches!(ctrl.search(&bytes), Err(Error::Validation(_))));
    assert_eq!(ctrl.out.events, []);
}

#[test]
fn upstream_parity_response_routing_uses_first_frequency_band() {
    // src/agent/em_agent.cpp:2367-2371 calls get_freq_band; src/em/em_msg.cpp:346-363
    // chooses the first Supported or Autoconfig Frequency Band TLV in frame order.
    let mut nodes = NodeTable::new();
    let other = key(MacAddr::new([2, 0, 0, 0, 9, 1]), NodeKind::Radio);
    let expected = key(RADIO, NodeKind::Radio);
    nodes.insert(Node::new(
        other,
        AgentState::Unconfigured,
        3,
        Some(FreqBand::BAND_6_GHZ),
    ));
    nodes.insert(Node::new(expected, AgentState::Unconfigured, 3, Some(BAND)));
    let supported = (TlvType::SUPPORTED_FREQ_BAND, BAND);
    let autoconfig = (TlvType::AUTOCONFIG_FREQ_BAND, FreqBand::BAND_6_GHZ);
    let cases: [(&[(TlvType, FreqBand)], _); 4] = [
        (&[autoconfig, supported], other),
        (&[supported, autoconfig], expected),
        (&[autoconfig], other),
        (&[supported], expected),
    ];
    for (bands, selected) in cases {
        let mut builder = MsgBuilder::new(
            &test_header(MsgType::AP_AUTOCONFIGURATION_RESPONSE, 1),
            RAW_BUDGET,
        );
        for &(tlv_type, band) in bands {
            builder.tlv(tlv_type, &[u8::from(band)]);
        }
        let bytes = builder.finish().unwrap();
        assert_eq!(
            configuration::route_response(&nodes, &bytes).unwrap(),
            Some(selected)
        );
    }

    // em-msg rejects an empty band value instead of reading beyond it.
    let mut builder = MsgBuilder::new(
        &test_header(MsgType::AP_AUTOCONFIGURATION_RESPONSE, 1),
        RAW_BUDGET,
    );
    builder
        .tlv(TlvType::AUTOCONFIG_FREQ_BAND, &[])
        .tlv(TlvType::SUPPORTED_FREQ_BAND, &[u8::from(BAND)]);
    assert_eq!(
        configuration::route_response(&nodes, &builder.finish().unwrap()),
        Err(Error::Reader(em_msg::ReaderError::ValueTooShort {
            tlv_type: TlvType::AUTOCONFIG_FREQ_BAND,
            needed: 1,
            found: 0,
        }))
    );

    let mut builder = MsgBuilder::new(
        &test_header(MsgType::AP_AUTOCONFIGURATION_RESPONSE, 1),
        RAW_BUDGET,
    );
    builder
        .tlv(
            TlvType::AUTOCONFIG_FREQ_BAND,
            &[u8::from(FreqBand::BAND_6_GHZ)],
        )
        .tlv(TlvType::SUPPORTED_FREQ_BAND, &[]);
    assert_eq!(
        configuration::route_response(&nodes, &builder.finish().unwrap()).unwrap(),
        Some(other)
    );

    let mut builder = MsgBuilder::new(
        &test_header(MsgType::AP_AUTOCONFIGURATION_RESPONSE, 1),
        RAW_BUDGET,
    );
    builder.tlv(TlvType::AL_MAC_ADDRESS, &[1]);
    let mut malformed = builder.finish().unwrap();
    // Remove the value and EOM while retaining the TLV's declared length.
    malformed.truncate(malformed.len() - 4);
    assert!(matches!(
        configuration::route_response(&nodes, &malformed),
        Err(Error::Framing(_))
    ));

    let missing = MsgBuilder::new(
        &test_header(MsgType::AP_AUTOCONFIGURATION_RESPONSE, 1),
        RAW_BUDGET,
    )
    .finish()
    .unwrap();
    assert_eq!(
        configuration::route_response(&nodes, &missing),
        Err(Error::MissingTlv(TlvType::SUPPORTED_FREQ_BAND))
    );
}

#[test]
fn upstream_parity_controller_response_services_follow_peer() {
    // src/em/config/em_configuration.cpp:4416-4432 adds the EM+ controller
    // service when the routed Search identifies an EM+ agent.
    let agent = [1, u8::from(ServiceType::MULTI_AP_AGENT)];
    let emplus_agent = [
        2,
        u8::from(ServiceType::MULTI_AP_AGENT),
        u8::from(ServiceType::EMPLUS_AGENT),
    ];
    let emplus_agent_only = [1, u8::from(ServiceType::EMPLUS_AGENT)];
    let malformed_agent = [2, u8::from(ServiceType::MULTI_AP_AGENT)];
    let controller = [1, u8::from(ServiceType::MULTI_AP_CONTROLLER)];
    let emplus_controller = [
        2,
        u8::from(ServiceType::MULTI_AP_CONTROLLER),
        u8::from(ServiceType::EMPLUS_CONTROLLER),
    ];
    let mut ctrl = Controller::new();
    for (supported, is_emplus, expected) in [
        (None, false, &controller[..]),
        (Some(&agent[..]), false, &controller[..]),
        (Some(&emplus_agent[..]), true, &emplus_controller[..]),
        (Some(&emplus_agent_only[..]), true, &emplus_controller[..]),
        (Some(&malformed_agent[..]), false, &controller[..]),
        (Some(&agent[..]), false, &controller[..]),
    ] {
        let bytes = search(Some(&[3]), Some(0), supported, Some(&[1, 0]));
        assert_eq!(
            configuration::route_search(&bytes).unwrap().is_emplus_agent,
            is_emplus
        );
        let before = ctrl.out.events.len();
        ctrl.search(&bytes).unwrap();
        let (_, reply) = frame(&ctrl.out.events, before);
        let (_, area) = CmduHeader::parse(reply).unwrap();
        assert_eq!(
            readers::find_tlv(area, TlvType::SUPPORTED_SERVICE).unwrap(),
            Some(expected)
        );
        assert!(validate(reply, ProfileType::MULTI_AP_PROFILE_3).is_ok());
        let mut receiving_agent = Agent::new(AgentState::AutoconfigRspPending, 3);
        assert_eq!(
            configuration::route_response(&receiving_agent.nodes, reply).unwrap(),
            Some(receiving_agent.key)
        );
        receiving_agent.response(reply).unwrap();
        assert_eq!(receiving_agent.state(), AgentState::WscM2Pending);
    }
}

#[test]
fn renew_round_trips_and_validates() {
    let renew = configuration::build_renew(CONTROLLER, BAND, 9).unwrap();
    validate(&renew, ProfileType::MULTI_AP_PROFILE_3).unwrap();
    let (header, _) = CmduHeader::parse(&renew).unwrap();
    assert_eq!(header.msg_type, MsgType::AP_AUTOCONFIGURATION_RENEW);
    assert_eq!(header.msg_id, 9);
    assert!(!header.relay);
    let route = configuration::route_renew(&renew).unwrap();
    assert_eq!(route.al_mac, CONTROLLER);
    assert_eq!(route.band, BAND);
    for len in 0..renew.len() {
        assert!(configuration::route_renew(&renew[..len]).is_err());
    }
    let search = configuration::build_search(AGENT, BAND, 3, 1).unwrap();
    assert!(configuration::route_renew(&search).is_err());
}
