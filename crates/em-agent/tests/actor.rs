// SPDX-License-Identifier: Apache-2.0

//! Tests of the agent on the event loop against a fake AL-SAP service (0012 D9).

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "0016 U27: integration test measures real time and uses OS randomness"
)]

mod support;

use std::cell::RefCell;
use std::io;
use std::sync::Once;
use std::time::Duration;

use em_agent::platform::FakePlatform;
use em_agent::{ActorError, AgentConfig, run};
use em_cmd::RadioSpec;
use em_core::MsgIdCounter;
use em_core::configuration::build_response;
use em_core::topology::{build_query, parse_response};
use em_core::wsc::{AUTH_WPA2_PERSONAL, Identity, NetworkSsid, build_m2, parse_m1};
use em_crypto::rand::OsRandom;
use em_msg::{CmduHeader, readers};
use em_transport::TransportError;
use em_types::{FreqBand, MacAddr, MsgType};
use support::{FakeService, NOT_SUPPORTED, SUCCESS, ServiceSession, WireSdu, bounded};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

thread_local! {
    /// What the current test thread logged. The tests run on a current-thread runtime, so the
    /// agent logs on the thread of the test.
    static LOG: RefCell<String> = const { RefCell::new(String::new()) };
}

struct ThreadLog;

impl io::Write for ThreadLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        LOG.with(|log| log.borrow_mut().push_str(&String::from_utf8_lossy(bytes)));
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Starts capturing the logs of this test thread, from INFO up, for [`captured_logs`].
///
/// The subscriber is global and installed once: a scoped one would miss the call sites that a
/// test running in parallel had already seen without a subscriber. Each test thread has its own
/// buffer, so the tests do not see each other's lines.
fn capture_logs() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        tracing_subscriber::fmt()
            .with_writer(|| ThreadLog)
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .init();
    });
    LOG.with(|log| log.borrow_mut().clear());
}

fn captured_logs() -> String {
    LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

const AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 1]);
const OTHER_AL_MAC: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 9]);
const CONTROLLER: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 2]);
const RADIO_24: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 1]);
const RADIO_5: MacAddr = MacAddr::new([2, 0, 0, 0, 1, 2]);

type ActorResult = Result<(), ActorError>;

/// A running agent and the way to stop it.
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

    // Both radios moved on and sent their M1; five seconds of virtual time (less than the
    // 10 s wait for an M2) bring no further Search.
    read_m1(&mut session).await;
    read_m1(&mut session).await;
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

/// The role is told the link was lost before it is told about the new session (0016 D5). The
/// log lines are the ones `docker/run-s1.sh` greps.
#[tokio::test(start_paused = true)]
async fn a_reconnect_logs_the_lost_link_before_the_new_session() {
    capture_logs();
    let service = FakeService::new();
    let running = start(&service);
    let mut first = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut first).await;
    drop(first);
    let mut second = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut second).await;
    running.stop.send(()).unwrap();
    assert!(finished(running.handle).await.is_ok());

    let logs = captured_logs();
    let registered = logs
        .find("em_agent::actor: registered session=1")
        .unwrap_or_else(|| panic!("no registered line in:\n{logs}"));
    let lost = logs
        .find("em_agent::actor: link lost, waiting to register")
        .expect("no 'link lost' line");
    let new_session = logs
        .find("em_agent::actor: new session, same AL MAC")
        .expect("no 'new session' line");
    assert!(
        registered < lost && lost < new_session,
        "wrong order:\n{logs}"
    );
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

/// The transport ends on its own, after the role was running for a session: `run` returns
/// the transport's error.
#[tokio::test(start_paused = true)]
async fn a_rejected_registration_on_a_reconnect_ends_the_actor_with_an_error() {
    let service = FakeService::new();
    let running = start(&service);
    let mut first = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut first).await;
    drop(first);

    let _second = service.accept_and_respond(AL_MAC, NOT_SUPPORTED).await;
    match finished(running.handle).await {
        Err(ActorError::Transport(TransportError::RegistrationRejected { result })) => {
            assert_eq!(result, NOT_SUPPORTED);
        }
        other => panic!("expected a rejected registration, got {other:?}"),
    }
}

/// The service reads nothing, so Searches pile up in the socket and the transport has frames
/// queued when the stop comes. The stop takes at most 1 s (900 ms of writing, 100 ms of linger).
#[tokio::test(start_paused = true)]
async fn a_stop_with_frames_queued_ends_within_a_second() {
    let service = FakeService::new();
    let running = start(&service);
    let _session = service.accept_and_respond(AL_MAC, SUCCESS).await;
    // Ten rounds of Searches that nobody reads.
    tokio::time::sleep(Duration::from_secs(10)).await;

    let stopped_at = tokio::time::Instant::now();
    running.stop.send(()).unwrap();
    let result = finished(running.handle).await;
    assert!(result.is_ok());
    assert!(
        stopped_at.elapsed() <= Duration::from_secs(1),
        "stopping took {:?}",
        stopped_at.elapsed()
    );
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

/// Reads SDUs until an M1 arrives and returns its CMDU.
async fn read_m1(session: &mut ServiceSession) -> Vec<u8> {
    loop {
        let sdu = session.read_sdu().await;
        if CmduHeader::parse(&sdu.cmdu).unwrap().0.msg_type == MsgType::AP_AUTOCONFIGURATION_WSC {
            assert_eq!(sdu.remote, CONTROLLER);
            return sdu.cmdu;
        }
    }
}

fn m2_for(m1_cmdu: &[u8]) -> Vec<u8> {
    let m1 = parse_m1(m1_cmdu).unwrap();
    let rows = [NetworkSsid {
        enabled: true,
        ssid: b"actor-net",
        network_key: b"actor-pass",
        bands: &[FreqBand::BAND_2_4_GHZ, FreqBand::BAND_5_GHZ],
        haul_types: &[0],
        authentication_type: AUTH_WPA2_PERSONAL,
    }];
    let identity = Identity {
        uuid: [3; 16],
        manufacturer: b"controller",
        model: b"model",
        serial: b"1",
        primary_device_type: [0, 6, 0, 0x50, 0xf2, 4, 0, 1],
    };
    build_m2(
        &m1,
        &identity,
        &rows,
        &mut MsgIdCounter::new(),
        &mut OsRandom::new(),
    )
    .unwrap()
    .cmdu
}

/// An M2 per radio reaches the platform, which confirms once; the agent then answers the
/// controller's Topology Query (0013 D4).
#[tokio::test(start_paused = true)]
async fn m2s_and_a_query_complete_onboarding_through_the_platform_channel() {
    let service = FakeService::new();
    let running = start(&service);
    let mut session = service.accept_and_respond(AL_MAC, SUCCESS).await;
    expect_both_bands(&mut session).await;

    for band in [FreqBand::BAND_2_4_GHZ, FreqBand::BAND_5_GHZ] {
        let response = build_response(band, 3, 1).unwrap();
        session.write_sdu(AL_MAC, CONTROLLER, &response).await;
    }
    for _ in 0..2 {
        let m1 = read_m1(&mut session).await;
        session.write_sdu(AL_MAC, CONTROLLER, &m2_for(&m1)).await;
    }

    // Queries before the confirmation are not answered; the controller repeats them.
    let mut ids = MsgIdCounter::new();
    let mut answer = None;
    for _ in 0..20 {
        let query = build_query(&mut ids).unwrap();
        session.write_sdu(AL_MAC, CONTROLLER, &query).await;
        if let Ok(sdu) = tokio::time::timeout(Duration::from_secs(1), session.read_sdu()).await {
            answer = Some((query, sdu));
            break;
        }
    }
    let (query, sdu) = answer.expect("the Topology Query was never answered");
    assert_eq!(sdu.remote, CONTROLLER);
    assert_eq!(sdu.local, AL_MAC);
    let (header, _) = CmduHeader::parse(&sdu.cmdu).unwrap();
    assert_eq!(header.msg_type, MsgType::TOPOLOGY_RESPONSE);
    assert_eq!(header.msg_id, CmduHeader::parse(&query).unwrap().0.msg_id);
    let parsed = parse_response(&sdu.cmdu, &[b"actor-net"], 3).unwrap();
    assert_eq!(parsed.operational_bss.radios.len(), 2);

    running.stop.send(()).unwrap();
    assert!(finished(running.handle).await.is_ok());
}
