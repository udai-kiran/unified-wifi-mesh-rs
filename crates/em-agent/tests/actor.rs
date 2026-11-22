// SPDX-License-Identifier: Apache-2.0

//! Tests of the agent role actor against a fake AL-SAP service (0012 D9).

mod support;

use std::time::Duration;

use em_agent::platform::FakePlatform;
use em_agent::{ActorError, AgentConfig, run};
use em_cmd::RadioSpec;
use em_core::configuration::build_response;
use em_msg::{CmduHeader, readers};
use em_transport::TransportError;
use em_types::{FreqBand, MacAddr, MsgType};
use support::{FakeService, NOT_SUPPORTED, SUCCESS, ServiceSession, WireSdu, bounded};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
const OTHER_AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 9]);
const CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
const RADIO_24: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 1]);
const RADIO_5: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 2]);

type ActorResult = Result<(), ActorError>;

/// A running agent actor and the way to stop it.
struct Running {
    handle: JoinHandle<ActorResult>,
    stop: oneshot::Sender<()>,
}

fn start(service: &FakeService) -> Running {
    let config = AgentConfig {
        control_path: service.control_path(),
        data_path: service.data_path(),
        profile: 3,
    };
    let platform = FakePlatform::new(vec![
        RadioSpec {
            ruid: RADIO_24,
            band: FreqBand::BAND_2_4_GHZ,
        },
        RadioSpec {
            ruid: RADIO_5,
            band: FreqBand::BAND_5_GHZ,
        },
    ]);
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(run(config, platform, async {
        let _ = stopped.await;
    }));
    Running { handle, stop }
}

async fn finished(handle: JoinHandle<ActorResult>) -> ActorResult {
    bounded("the actor to end", handle)
        .await
        .expect("the actor task panicked")
}

/// Reads the next SDU and checks it is a Search from this agent; returns its band.
async fn expect_search(session: &mut ServiceSession) -> FreqBand {
    let WireSdu {
        local,
        remote,
        cmdu,
    } = session.read_sdu().await;
    assert_eq!(local, AL_MAC, "local is the recorded AL MAC");
    assert_eq!(remote, MacAddr::IEEE1905_MULTICAST);
    let (header, area) = CmduHeader::parse(&cmdu).unwrap();
    assert_eq!(header.msg_type, MsgType::AP_AUTOCONFIGURATION_SEARCH);
    assert_eq!(readers::al_mac_address(area).unwrap(), Some(AL_MAC));
    readers::freq_band(area).unwrap().unwrap()
}

/// The first Search of each radio, in whichever order they come.
async fn expect_both_bands(session: &mut ServiceSession) {
    let first = expect_search(session).await;
    let second = expect_search(session).await;
    let mut bands = [u8::from(first), u8::from(second)];
    bands.sort_unstable();
    assert_eq!(
        bands,
        [
            u8::from(FreqBand::BAND_2_4_GHZ),
            u8::from(FreqBand::BAND_5_GHZ)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn registers_and_sends_searches_with_the_recorded_al_mac() {
    let service = FakeService::new();
    let running = start(&service);
    let mut session = service.accept_and_respond(AL_MAC, SUCCESS).await;

    expect_both_bands(&mut session).await;
    // Searches are resent until a Response arrives (0012 D2).
    expect_both_bands(&mut session).await;

    running.stop.send(()).unwrap();
    assert!(finished(running.handle).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn a_response_per_radio_stops_the_searches() {
    let service = FakeService::new();
    let running = start(&service);
    let mut session = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut session).await;

    for band in [FreqBand::BAND_2_4_GHZ, FreqBand::BAND_5_GHZ] {
        let response = build_response(band, 3, 1).unwrap();
        session.write_sdu(AL_MAC, CONTROLLER, &response).await;
    }

    // Both radios moved on; five seconds of virtual time bring no further Search.
    let next = tokio::time::timeout(Duration::from_secs(5), session.read_sdu()).await;
    assert!(next.is_err(), "a Search was sent after the Responses");

    running.stop.send(()).unwrap();
    assert!(finished(running.handle).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn searches_continue_on_a_new_session_with_the_same_al_mac() {
    let service = FakeService::new();
    let running = start(&service);
    let mut first = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut first).await;
    drop(first);

    let mut second = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut second).await;

    running.stop.send(()).unwrap();
    assert!(finished(running.handle).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn a_different_al_mac_on_a_later_session_is_fatal() {
    let service = FakeService::new();
    let running = start(&service);
    let mut first = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut first).await;
    drop(first);

    let _second = service.accept_and_respond(OTHER_AL_MAC, SUCCESS).await;
    match finished(running.handle).await {
        Err(ActorError::AlMacChanged(changed)) => {
            assert_eq!(changed.recorded, AL_MAC);
            assert_eq!(changed.new, OTHER_AL_MAC);
        }
        other => panic!("expected AlMacChanged, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_rejected_registration_ends_the_actor_with_an_error() {
    let service = FakeService::new();
    let running = start(&service);
    let _session = service.accept_and_respond(AL_MAC, NOT_SUPPORTED).await;

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
    let _session = service.accept_and_respond(AL_MAC, SUCCESS).await;

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
