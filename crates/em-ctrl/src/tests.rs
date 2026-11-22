// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use em_core::CtrlState;
use em_core::configuration::build_search;
use em_msg::{CmduHeader, MsgBuilder, RAW_BUDGET, readers, validate};
use em_types::{FreqBand, MacAddr, MsgType, ProfileType, ServiceType, TlvType};

use crate::{AlMacChanged, CtrlCore, Frame, PeerModel};

const CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
const AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
const BAND: FreqBand = FreqBand::BAND_5_GHZ;

fn registered_core() -> CtrlCore {
    let mut core = CtrlCore::new(Vec::new(), false);
    core.registered(CONTROLLER).unwrap();
    core
}

fn al_state(core: &CtrlCore) -> CtrlState {
    core.nodes().iter().next().unwrap().state
}

fn send(core: &mut CtrlCore, cmdu: &[u8]) -> Vec<Frame> {
    core.received(AGENT, cmdu, Instant::now())
}

fn search_header(mid: u16) -> CmduHeader {
    CmduHeader {
        msg_type: MsgType::AP_AUTOCONFIGURATION_SEARCH,
        msg_id: mid,
        frag_id: 0,
        last_fragment: true,
        relay: true,
    }
}

/// A Search with a readable AL MAC and band but no Searched Role, Supported
/// Service or Searched Service: the validation table rejects it.
fn invalid_search(profile: Option<u8>) -> Vec<u8> {
    let mut builder = MsgBuilder::new(&search_header(0x44), RAW_BUDGET);
    builder
        .tlv(TlvType::AL_MAC_ADDRESS, &AGENT.octets())
        .tlv(TlvType::AUTOCONFIG_FREQ_BAND, &[u8::from(BAND)]);
    if let Some(profile) = profile {
        builder.tlv(TlvType::MULTI_AP_PROFILE, &[profile]);
    }
    builder.finish().unwrap()
}

fn emplus_search(profile: u8, mid: u16) -> Vec<u8> {
    let mut builder = MsgBuilder::new(&search_header(mid), RAW_BUDGET);
    builder
        .tlv(TlvType::AL_MAC_ADDRESS, &AGENT.octets())
        .tlv(TlvType::SEARCHED_ROLE, &[0])
        .tlv(TlvType::AUTOCONFIG_FREQ_BAND, &[u8::from(BAND)])
        .tlv(
            TlvType::SUPPORTED_SERVICE,
            &[1, u8::from(ServiceType::EMPLUS_AGENT)],
        )
        .tlv(
            TlvType::SEARCHED_SERVICE,
            &[1, u8::from(ServiceType::MULTI_AP_CONTROLLER)],
        )
        .tlv(TlvType::MULTI_AP_PROFILE, &[profile]);
    builder.finish().unwrap()
}

#[test]
fn the_al_node_is_created_at_the_first_registration() {
    let mut core = CtrlCore::new(Vec::new(), false);
    assert!(core.nodes().iter().next().is_none());
    // A Search before registration finds no AL node: no frame, no model.
    let search = build_search(AGENT, BAND, 3, 1).unwrap();
    assert_eq!(send(&mut core, &search).len(), 0);
    assert_eq!(core.peer(AGENT), None);

    core.registered(CONTROLLER).unwrap();
    assert_eq!(core.nodes().iter().count(), 1);
    assert_eq!(al_state(&core), CtrlState::Unconfigured);
}

#[test]
fn a_search_creates_the_model_once_and_is_answered_each_time() {
    let mut core = registered_core();
    let search = build_search(AGENT, BAND, 3, 0x1234).unwrap();
    let frames = send(&mut core, &search);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].remote, AGENT);
    let (header, area) = CmduHeader::parse(&frames[0].cmdu).unwrap();
    assert_eq!(header.msg_type, MsgType::AP_AUTOCONFIGURATION_RESPONSE);
    assert_eq!(header.msg_id, 0x1234);
    assert_eq!(readers::freq_band(area).unwrap(), Some(BAND));
    validate(&frames[0].cmdu, ProfileType::MULTI_AP_PROFILE_3).unwrap();
    assert_eq!(
        core.peer(AGENT),
        Some(&PeerModel {
            profile: 3,
            is_emplus_agent: false
        })
    );
    assert_eq!(al_state(&core), CtrlState::WscM1Pending);

    // A later Search takes the existing-model path: the profile stays, the
    // EM+ flag is updated, and a Response is sent again.
    let frames = send(&mut core, &emplus_search(2, 0x1235));
    assert_eq!(frames.len(), 1);
    let (header, _) = CmduHeader::parse(&frames[0].cmdu).unwrap();
    assert_eq!(header.msg_id, 0x1235);
    assert_eq!(
        core.peer(AGENT),
        Some(&PeerModel {
            profile: 3,
            is_emplus_agent: true
        })
    );
}

#[test]
fn the_model_stores_one_for_a_reserved_profile_and_the_response_assumes_three() {
    // Routing stores 1 for a profile octet of 4 or more, as upstream
    // (0012 rev 7 erratum to D3); the Response still assumes the controller's 3.
    let mut core = registered_core();
    let frames = send(&mut core, &emplus_search(7, 1));
    assert_eq!(core.peer(AGENT).unwrap().profile, 1);
    assert_eq!(frames.len(), 1);
    let (_, area) = CmduHeader::parse(&frames[0].cmdu).unwrap();
    assert_eq!(
        readers::profile(area).unwrap(),
        Some(ProfileType::MULTI_AP_PROFILE_3)
    );
}

#[test]
fn an_invalid_search_still_creates_the_model_and_sends_no_response() {
    let mut core = registered_core();
    let frames = send(&mut core, &invalid_search(None));
    assert_eq!(frames.len(), 0);
    assert_eq!(
        core.peer(AGENT),
        Some(&PeerModel {
            profile: 1,
            is_emplus_agent: false
        })
    );
    assert_eq!(al_state(&core), CtrlState::Unconfigured);
}

#[test]
fn a_search_that_cannot_be_routed_changes_nothing() {
    let mut core = registered_core();
    let mut builder = MsgBuilder::new(&search_header(1), RAW_BUDGET);
    builder.tlv(TlvType::AL_MAC_ADDRESS, &AGENT.octets());
    let cmdu = builder.finish().unwrap();
    assert_eq!(send(&mut core, &cmdu).len(), 0);
    assert_eq!(core.peer(AGENT), None);
    assert_eq!(send(&mut core, &[1, 2, 3]).len(), 0);
}

#[test]
fn re_registration_with_the_same_al_mac_keeps_state() {
    let mut core = registered_core();
    send(&mut core, &build_search(AGENT, BAND, 3, 1).unwrap());
    assert_eq!(al_state(&core), CtrlState::WscM1Pending);

    core.registered(CONTROLLER).unwrap();
    assert_eq!(core.nodes().iter().count(), 1);
    assert_eq!(al_state(&core), CtrlState::WscM1Pending);
    assert!(core.peer(AGENT).is_some());
}

#[test]
fn registration_with_a_different_al_mac_is_fatal() {
    let mut core = registered_core();
    let other = MacAddr::new([2, 0, 0, 0, 0, 9]);
    assert_eq!(
        core.registered(other),
        Err(AlMacChanged {
            recorded: CONTROLLER,
            new: other
        })
    );
    assert_eq!(core.nodes().iter().count(), 1);
    assert_eq!(core.nodes().iter().next().unwrap().key.mac, CONTROLLER);
}

#[test]
fn messages_other_than_a_search_are_ignored() {
    let mut core = registered_core();
    let response = em_core::configuration::build_response(BAND, 3, 1).unwrap();
    assert_eq!(send(&mut core, &response).len(), 0);
    assert_eq!(core.peer(AGENT), None);
}

fn agent_mac(index: u16) -> MacAddr {
    let [high, low] = index.to_be_bytes();
    MacAddr::new([2, 0, 0, 0, high, low])
}

fn search_from(index: u16) -> Vec<u8> {
    build_search(agent_mac(index), BAND, 3, index).unwrap()
}

#[test]
fn the_table_keeps_256_models_and_evicts_the_oldest_created() {
    let mut core = registered_core();
    for index in 0..257 {
        send(&mut core, &search_from(index));
    }
    assert_eq!(core.models.len(), 256);
    assert_eq!(core.peer(agent_mac(0)), None);
    assert!(core.peer(agent_mac(1)).is_some());
    assert!(core.peer(agent_mac(256)).is_some());

    // An evicted AL MAC creates a model again; that evicts the next oldest.
    send(&mut core, &search_from(0));
    assert!(core.peer(agent_mac(0)).is_some());
    assert_eq!(core.peer(agent_mac(1)), None);
    assert_eq!(core.models.len(), 256);
}

#[test]
fn an_update_does_not_change_the_eviction_order() {
    let mut core = registered_core();
    for index in 0..256 {
        send(&mut core, &search_from(index));
    }
    // Searching again from the oldest AL MAC updates it; it stays the oldest.
    send(&mut core, &search_from(0));
    send(&mut core, &search_from(256));
    assert_eq!(core.peer(agent_mac(0)), None);
    assert!(core.peer(agent_mac(1)).is_some());
}
