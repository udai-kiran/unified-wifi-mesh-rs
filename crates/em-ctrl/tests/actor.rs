// SPDX-License-Identifier: Apache-2.0

//! Tests of the controller role actor against a fake AL-SAP service (0012 D9).

mod support;

use std::time::Duration;

use em_core::configuration::build_search;
use em_ctrl::{ActorError, CtrlConfig, run};
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
