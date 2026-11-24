// SPDX-License-Identifier: Apache-2.0

//! Tests of the controller role actor against a fake AL-SAP service (0012 D9).

mod support;

use std::time::Duration;

use em_core::MsgIdCounter;
use em_core::configuration::build_search;
use em_core::wsc::{self, Identity, M1Params, Session};
use em_crypto::rand::OsRandom;
use em_ctrl::{ActorError, CtrlConfig, run};
use em_db::load_reset_str;
use em_msg::bodies::{
    ApRadioAdvancedCapabilities, ApRadioBasicCapabilities, BasicOperatingClass,
    Profile2ApCapability,
};
use em_msg::{CmduHeader, readers};
use em_transport::TransportError;
use em_types::{FreqBand, MacAddr, MsgType};
use support::{FakeService, NOT_SUPPORTED, SUCCESS, ServiceSession, bounded};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
const OTHER_CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 9]);
const AGENT: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
const BAND: FreqBand = FreqBand::BAND_5_GHZ;
const RADIO: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 2]);

const RESET: &str = r#"{"wfa-dataelements:Reset": {"NetworkSSIDList": [
  {"SSID": "front", "PassPhrase": "front-pass", "Band": ["5"], "Enable": true,
   "HaulType": ["Fronthaul"], "AuthType": "WPA2 Personal"}]}}"#;

type ActorResult = Result<(), ActorError>;

/// A running controller actor and the way to stop it.
struct Running {
    handle: JoinHandle<ActorResult>,
    stop: oneshot::Sender<()>,
}

fn start(service: &FakeService) -> Running {
    let config = CtrlConfig {
        control_path: service.control_path(),
        data_path: service.data_path(),
        networks: load_reset_str(RESET).unwrap(),
        drop_first_m2: false,
    };
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(run(config, async {
        let _ = stopped.await;
    }));
    Running { handle, stop }
}

async fn finished(handle: JoinHandle<ActorResult>) -> ActorResult {
    bounded("the actor to end", handle)
        .await
        .expect("the actor task panicked")
}

/// Sends a Search from the agent and checks the Response that comes back on the
/// same session: the transport writes a frame only into the session it was built
/// for, so a Response stamped with an old session would never arrive.
async fn expect_answered(session: &mut ServiceSession, mid: u16) {
    let search = build_search(AGENT, BAND, 3, mid).unwrap();
    session.write_sdu(CONTROLLER, AGENT, &search).await;

    let response = session.read_sdu().await;
    assert_eq!(response.local, CONTROLLER, "local is the recorded AL MAC");
    assert_eq!(response.remote, AGENT);
    let (header, area) = CmduHeader::parse(&response.cmdu).unwrap();
    assert_eq!(header.msg_type, MsgType::AP_AUTOCONFIGURATION_RESPONSE);
    assert_eq!(header.msg_id, mid);
    assert_eq!(readers::freq_band(area).unwrap(), Some(BAND));
}

#[tokio::test(start_paused = true)]
async fn a_search_is_answered_with_a_response_for_the_current_session() {
    let service = FakeService::new();
    let running = start(&service);
    let mut first = service.accept_and_respond(CONTROLLER, SUCCESS).await;
    expect_answered(&mut first, 1).await;
    drop(first);

    // After a reconnect with the same AL MAC the Response is stamped with the
    // new session.
    let mut second = service.accept_and_respond(CONTROLLER, SUCCESS).await;
    expect_answered(&mut second, 2).await;

    running.stop.send(()).unwrap();
    assert!(finished(running.handle).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn a_different_al_mac_on_a_later_session_is_fatal() {
    let service = FakeService::new();
    let running = start(&service);
    let mut first = service.accept_and_respond(CONTROLLER, SUCCESS).await;
    expect_answered(&mut first, 1).await;
    drop(first);

    let _second = service.accept_and_respond(OTHER_CONTROLLER, SUCCESS).await;
    match finished(running.handle).await {
        Err(ActorError::AlMacChanged(changed)) => {
            assert_eq!(changed.recorded, CONTROLLER);
            assert_eq!(changed.new, OTHER_CONTROLLER);
        }
        other => panic!("expected AlMacChanged, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_rejected_registration_ends_the_actor_with_an_error() {
    let service = FakeService::new();
    let running = start(&service);
    let _session = service.accept_and_respond(CONTROLLER, NOT_SUPPORTED).await;

    match finished(running.handle).await {
        Err(ActorError::Transport(TransportError::RegistrationRejected { result })) => {
            assert_eq!(result, NOT_SUPPORTED);
        }
        other => panic!("expected a rejected registration, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn the_stop_path_ends_within_a_second() {
    let service = FakeService::new();
    let running = start(&service);
    let _session = service.accept_and_respond(CONTROLLER, SUCCESS).await;

    let stopped_at = tokio::time::Instant::now();
    running.stop.send(()).unwrap();
    let result = finished(running.handle).await;
    assert!(result.is_ok());
    assert!(
        stopped_at.elapsed() < Duration::from_secs(1),
        "stopping took {:?}",
        stopped_at.elapsed()
    );
}

fn m1_cmdu() -> Vec<u8> {
    let params = M1Params {
        al_mac: AGENT,
        controller: CONTROLLER,
        band: BAND,
        identity: Identity {
            uuid: [5; 16],
            manufacturer: b"agent",
            model: b"model",
            serial: b"1",
            primary_device_type: [0, 6, 0, 0x50, 0xf2, 4, 0, 1],
        },
        basic: ApRadioBasicCapabilities {
            ruid: RADIO,
            max_bss: 2,
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
            ruid: RADIO,
            combined_front_back: false,
            combined_profile_1_2: false,
            mscs: false,
            scs: false,
            qos_map: false,
            dscp_policy: false,
            qm_scs_traffic_description: false,
        },
    };
    wsc::build_m1(
        &params,
        &mut MsgIdCounter::new(),
        &mut OsRandom,
        &mut Session::new(),
    )
    .unwrap()
}

/// An M1 is answered with an M2, and the controller then sends a Topology Query on its
/// own deadline (0013 D3, D7).
#[tokio::test(start_paused = true)]
async fn an_m1_is_answered_and_a_topology_query_follows_on_the_deadline() {
    let service = FakeService::new();
    let running = start(&service);
    let mut session = service.accept_and_respond(CONTROLLER, SUCCESS).await;
    expect_answered(&mut session, 1).await;

    session.write_sdu(CONTROLLER, AGENT, &m1_cmdu()).await;
    let m2 = session.read_sdu().await;
    assert_eq!(m2.remote, AGENT);
    assert_eq!(
        CmduHeader::parse(&m2.cmdu).unwrap().0.msg_type,
        MsgType::AP_AUTOCONFIGURATION_WSC
    );

    let query = session.read_sdu().await;
    assert_eq!(query.remote, AGENT);
    assert_eq!(query.local, CONTROLLER);
    assert_eq!(
        CmduHeader::parse(&query.cmdu).unwrap().0.msg_type,
        MsgType::TOPOLOGY_QUERY
    );

    running.stop.send(()).unwrap();
    assert!(finished(running.handle).await.is_ok());
}
