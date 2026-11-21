// SPDX-License-Identifier: Apache-2.0

//! Tests of `em-transport` against a scripted fake service (note 0011 D6).
//!
//! Tests tagged `intentional-difference:<id>` pin a place where this client does
//! not do what upstream's does (0011 D3, steps 3, 4 and 6).

mod support;

use std::time::Duration;

use em_transport::{
    LinkState, MAX_FRAGMENT_PAYLOAD, MAX_SDU_PAYLOAD, OutboundSdu, Role, Sdu, Transport,
    TransportConfig, TransportCounters, TransportError, TransportTask, spawn,
};
use em_types::MacAddr;
use support::{
    FakeService, RECONNECT_DELAY, ServiceSession, TempDir, Watchdog, bounded, join, join_within,
    next_received, read_one, sdu_frame,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};

const SERVICE_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x01];
const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x02];
const SUCCESS: u8 = 0x01;
const UNFRAGMENTED: [u8; 3] = [0, 1, 0];

/// The client side of a test: the fields of [`Transport`].
struct Client {
    sdus: mpsc::Sender<OutboundSdu>,
    received: mpsc::Receiver<Sdu>,
    link: watch::Receiver<LinkState>,
    counters: watch::Receiver<TransportCounters>,
    handle: TransportTask,
}

fn client_from(transport: Transport) -> Client {
    let Transport {
        sdus,
        received,
        link,
        counters,
        task,
    } = transport;
    Client {
        sdus,
        received,
        link,
        counters,
        handle: task,
    }
}

fn start(service: &FakeService, role: Role) -> Client {
    client_from(spawn(service.config(role)))
}

/// Waits until `check` holds for the link state and returns that state.
async fn wait_for_link(
    link: &mut watch::Receiver<LinkState>,
    check: impl Fn(&LinkState) -> bool,
) -> LinkState {
    let state = bounded(
        "the link state to reach the expected value",
        link.wait_for(check),
    )
    .await
    .unwrap();
    *state
}

/// Waits until the link shows a registration.
async fn wait_until_registered(link: &mut watch::Receiver<LinkState>) -> LinkState {
    wait_for_link(link, |state| state.al_mac.is_some()).await
}

/// Waits until the link shows no registration.
async fn wait_until_disconnected(link: &mut watch::Receiver<LinkState>) {
    wait_for_link(link, |state| state.al_mac.is_none()).await;
}

fn registered_link(session: u64, al_mac: [u8; 6]) -> LinkState {
    LinkState {
        session,
        al_mac: Some(MacAddr::new(al_mac)),
    }
}

/// Starts a client and completes a successful registration.
async fn registered(role: Role) -> (FakeService, ServiceSession, Client) {
    let service = FakeService::new();
    let mut client = start(&service, role);
    let mut session = service.accept().await;
    session.respond(SERVICE_MAC, SUCCESS).await;
    assert_eq!(
        wait_until_registered(&mut client.link).await,
        registered_link(1, SERVICE_MAC)
    );
    (service, session, client)
}

fn sdu(local: [u8; 6], remote: [u8; 6], cmdu: &[u8]) -> Sdu {
    Sdu {
        local: MacAddr::new(local),
        remote: MacAddr::new(remote),
        cmdu: cmdu.to_vec(),
    }
}

/// An SDU built for `session`.
fn out(session: u64, local: [u8; 6], remote: [u8; 6], cmdu: &[u8]) -> OutboundSdu {
    OutboundSdu {
        session,
        sdu: sdu(local, remote, cmdu),
    }
}

async fn expect_received(client: &mut Client, expected: Sdu) {
    assert_eq!(next_received(&mut client.received).await, expected);
}

/// Waits until `check` holds for the counters.
async fn wait_for_counters(client: &mut Client, check: impl Fn(&TransportCounters) -> bool) {
    bounded(
        "the counters to reach the expected value",
        client.counters.wait_for(check),
    )
    .await
    .unwrap();
}

/// Lets the actor and the runtime's I/O driver run until nothing is ready.
async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

/// Counts the connections waiting in the listener's backlog, and takes them.
async fn take_pending_connections(listener: &UnixListener) -> usize {
    let mut taken = 0;
    while let Ok(Ok(_stream)) = tokio::time::timeout(Duration::ZERO, listener.accept()).await {
        taken += 1;
    }
    taken
}

/// Advances the paused clock, then lets the actor react.
async fn advance(by: Duration) {
    tokio::time::advance(by).await;
    settle().await;
}

#[tokio::test(start_paused = true)]
async fn registration_request_bytes_for_both_roles() {
    for (role, service_type) in [(Role::Agent, 0x01), (Role::Controller, 0x02)] {
        let service = FakeService::new();
        let _client = start(&service, role);
        let session = service.accept().await;
        // length 2, service_operation enable, service_type
        assert_eq!(session.request, [0, 0, 0, 2, 0x01, service_type]);
    }
}

#[tokio::test(start_paused = true)]
async fn registered_link_state_carries_the_al_mac_and_the_session() {
    let (_service, _session, client) = registered(Role::Agent).await;
    assert_eq!(*client.link.borrow(), registered_link(1, SERVICE_MAC));
    assert_eq!(client.counters.borrow().registrations, 1);
}

#[tokio::test(start_paused = true)]
async fn rejected_registration_ends_the_actor_with_the_result() {
    // 0x00 unknown, 0x02 not supported, and a value the protocol does not define.
    for result in [0x00, 0x02, 0x7f] {
        let service = FakeService::new();
        let client = start(&service, Role::Controller);
        let mut session = service.accept().await;
        session.respond(SERVICE_MAC, result).await;
        let error = join(client.handle).await.unwrap_err();
        assert!(
            matches!(error, TransportError::RegistrationRejected { result: seen } if seen == result),
            "unexpected error {error:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn rejected_registration_is_fatal_for_the_agent_role_too() {
    // intentional-difference:agent-rejection-fatal
    // Upstream's agent logs a rejected registration and carries on
    // (`em_agent.cpp:3006-3018`); only its controller exits (`em_ctrl.cpp:1800-1803`).
    // This client ends the actor with the result for both roles (0011 D3 step 3).
    let service = FakeService::new();
    let client = start(&service, Role::Agent);
    let mut session = service.accept().await;
    session.respond(SERVICE_MAC, 0x02).await;

    let error = join(client.handle).await.unwrap_err();
    assert!(
        matches!(error, TransportError::RegistrationRejected { result: 0x02 }),
        "unexpected error {error:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn response_split_across_two_reads_is_waited_for() {
    // intentional-difference:split-response
    // Upstream reads the registration response with one `recv` and takes whatever
    // arrived (`al_service_access_point.cpp:119-121`), so half a frame would be an
    // error there. This client waits for the whole frame.
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);
    let mut session = service.accept().await;

    let mut frame = vec![0, 0, 0, 7];
    frame.extend_from_slice(&SERVICE_MAC);
    frame.push(SUCCESS);
    session.write_control(&frame[..6]).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    session.write_control(&frame[6..]).await;

    assert_eq!(
        wait_until_registered(&mut client.link).await,
        registered_link(1, SERVICE_MAC)
    );
}

#[tokio::test(start_paused = true)]
async fn retries_while_the_socket_files_have_no_listener() {
    // intentional-difference:retry-connect
    // Upstream's controller exits when the connect fails (`em_ctrl.cpp:1718-1738`) and the
    // agent's constructor throws without catching it (`al_service_access_point.cpp:36-55`).
    // This client retries after the reconnect delay.
    // A stale socket file: the file exists and nothing listens (the service between sessions).
    let dir = TempDir::new();
    let control_path = dir.path().join("control");
    let data_path = dir.path().join("data");
    drop(UnixListener::bind(&control_path).unwrap());
    drop(UnixListener::bind(&data_path).unwrap());
    let mut config = TransportConfig::new(
        dir.path().join("control"),
        dir.path().join("data"),
        Role::Agent,
    );
    config.reconnect_delay = RECONNECT_DELAY;
    // A client that never waits between attempts spins without yielding, and nothing on
    // its thread can interrupt it.
    let _watchdog = Watchdog::start(
        "retry with a stale socket file".to_owned(),
        Duration::from_secs(10),
    );
    let mut transport = spawn(config);

    // Let several attempts fail, then bring a listener up on the same paths.
    for _ in 0..3 {
        advance(RECONNECT_DELAY).await;
    }
    std::fs::remove_file(&control_path).unwrap();
    std::fs::remove_file(&data_path).unwrap();
    let live = FakeService {
        control: UnixListener::bind(&control_path).unwrap(),
        data: UnixListener::bind(&data_path).unwrap(),
        dir,
    };
    let mut session = live.accept().await;
    session.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut transport.link).await;
}

#[tokio::test(start_paused = true)]
async fn retries_while_the_socket_files_are_missing() {
    // intentional-difference:retry-connect
    // Upstream's controller exits when the connect fails (`em_ctrl.cpp:1718-1738`) and the
    // agent's constructor throws without catching it (`al_service_access_point.cpp:36-55`).
    // This client retries after the reconnect delay.
    let dir = TempDir::new();
    let mut config = TransportConfig::new(
        dir.path().join("control"),
        dir.path().join("data"),
        Role::Agent,
    );
    config.reconnect_delay = RECONNECT_DELAY;
    let _watchdog = Watchdog::start(
        "retry with missing socket files".to_owned(),
        Duration::from_secs(10),
    );
    let mut transport = spawn(config);

    for _ in 0..3 {
        advance(RECONNECT_DELAY).await;
    }
    let live = FakeService {
        control: UnixListener::bind(dir.path().join("control")).unwrap(),
        data: UnixListener::bind(dir.path().join("data")).unwrap(),
        dir,
    };
    let mut session = live.accept().await;
    session.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut transport.link).await;
}

#[tokio::test(start_paused = true)]
async fn retries_after_end_of_file_before_the_response() {
    // intentional-difference:retry-connect
    // Upstream's controller exits when registration fails (`em_ctrl.cpp:1718-1738`) and the
    // agent's constructor throws without catching it (`al_service_access_point.cpp:36-55`).
    // This client registers again after the reconnect delay.
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);

    let first = service.accept().await;
    drop(first); // closes both sockets with no response

    // The next attempt waits for the reconnect delay.
    let early = tokio::time::timeout(RECONNECT_DELAY / 2, service.control.accept()).await;
    assert!(
        early.is_err(),
        "registered again before the reconnect delay"
    );

    let mut second = service.accept().await;
    second.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn all_zero_al_mac_is_retried_not_used() {
    // intentional-difference:zero-al-mac
    // Upstream would take the zero address as its AL MAC: it copies the six octets
    // and reads `result` without looking at them (`al_service_registration_response.cpp:31-57`).
    // This client closes the sockets and registers again after the reconnect delay.
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);

    let mut first = service.accept().await;
    first.respond([0; 6], SUCCESS).await;
    // The client closes the first session: end of file on its data socket.
    assert_eq!(read_one(&mut first.data).await, 0);

    let mut second = service.accept().await;
    second.respond(SERVICE_MAC, SUCCESS).await;
    // The first state with an address is the real one: the zero one was never published.
    assert_eq!(
        wait_until_registered(&mut client.link).await,
        registered_link(1, SERVICE_MAC)
    );
    assert_eq!(client.counters.borrow().registrations, 1);
}

#[tokio::test(start_paused = true)]
async fn sent_sdu_bytes_follow_the_sdu_codec() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    let local = [0x02, 0, 0, 0, 0, 0x0a];
    let remote = [0x02, 0, 0, 0, 0, 0x0b];
    client
        .sdus
        .send(out(1, local, remote, &[0x80, 0x01, 0xff]))
        .await
        .unwrap();

    let frame = session.read_data_frame().await;
    assert_eq!(
        frame,
        [
            0, 0, 0, 18, // 6 + 6 + 3 + 3
            0x02, 0, 0, 0, 0, 0x0a, // MAC A: local
            0x02, 0, 0, 0, 0, 0x0b, // MAC B: remote
            0, 1, 0, // is_fragment 0, is_last_fragment 1, fragment_id 0
            0x80, 0x01, 0xff,
        ]
    );

    wait_for_counters(&mut client, |c| c.sdus_sent >= 1).await;
    assert_eq!(client.counters.borrow().sdus_sent, 1);
}

#[tokio::test(start_paused = true)]
async fn address_mapping_in_both_directions() {
    let (_service, mut session, mut client) = registered(Role::Controller).await;

    // Send: local goes in MAC A, remote in MAC B.
    client
        .sdus
        .send(out(1, [1, 1, 1, 1, 1, 1], [2, 2, 2, 2, 2, 2], &[9]))
        .await
        .unwrap();
    let frame = session.read_data_frame().await;
    assert_eq!(frame[4..10], [1, 1, 1, 1, 1, 1]);
    assert_eq!(frame[10..16], [2, 2, 2, 2, 2, 2]);

    // Delivery: MAC A (the service's AL MAC) is local, MAC B (the sender) is remote.
    session
        .write_data(&sdu_frame([3; 6], [4; 6], UNFRAGMENTED, &[7]))
        .await;
    expect_received(&mut client, sdu([3; 6], [4; 6], &[7])).await;
}

#[tokio::test(start_paused = true)]
async fn payload_of_65517_is_one_frame_and_65518_is_two() {
    let (_service, mut session, client) = registered(Role::Agent).await;
    let local = [0x02, 0, 0, 0, 0, 0x0a];

    client
        .sdus
        .send(out(1, local, PEER_MAC, &vec![0xaa; MAX_FRAGMENT_PAYLOAD]))
        .await
        .unwrap();
    let whole = session.read_data_frame().await;
    assert_eq!(whole.len(), 65_536);
    assert_eq!(whole[16..19], [0, 1, 0]);

    client
        .sdus
        .send(out(
            1,
            local,
            PEER_MAC,
            &vec![0xbb; MAX_FRAGMENT_PAYLOAD + 1],
        ))
        .await
        .unwrap();
    let first = session.read_data_frame().await;
    let second = session.read_data_frame().await;
    assert_eq!(first.len(), 65_536);
    assert_eq!(first[16..19], [1, 0, 0]); // fragment, not last, id 0
    assert_eq!(second.len(), 4 + 15 + 1);
    assert_eq!(second[16..19], [1, 1, 1]); // fragment, last, id 1
    assert_eq!(second[19], 0xbb);
}

#[tokio::test(start_paused = true)]
async fn received_fragments_are_joined() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 0], b"ab"))
        .await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 1], b"cd"))
        .await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 1, 2], b"e"))
        .await;
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"abcde")).await;
}

#[tokio::test(start_paused = true)]
async fn fragment_out_of_order_is_dropped_and_id_zero_resyncs() {
    // intentional-difference:fragment-resync
    // Upstream throws "Fragment out of order" (`al_service_access_point.cpp:267-268`), and
    // the manager loop rethrows anything but an InvalidMessage out of the loop
    // (`em_mgr.cpp:427-433`). This client drops the set and goes on: id 0 or an unfragmented
    // frame starts a new SDU.
    let (_service, mut session, mut client) = registered(Role::Agent).await;

    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 0], b"ab"))
        .await;
    // id 2 where 1 is expected
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 2], b"xx"))
        .await;
    // the tail of the dropped set
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 1, 3], b"yy"))
        .await;
    // an unfragmented frame is delivered
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"whole"))
        .await;
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"whole")).await;

    // a set that starts at id 0 joins normally afterwards
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 0], b"12"))
        .await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 1, 1], b"3"))
        .await;
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"123")).await;

    // the session never ended
    assert_eq!(client.counters.borrow().registrations, 1);
    assert_eq!(client.counters.borrow().dropped_receive_fragments, 3);
}

#[tokio::test(start_paused = true)]
async fn oversize_frame_is_skipped_and_the_stream_stays_in_step() {
    // intentional-difference:oversize-skipped
    // Upstream throws on a length above `SOCKET_MTU - 4` before reading the body
    // (`al_service_access_point.cpp:235-237`). The manager drops that packet
    // (`em_mgr.cpp:427-433`), but the body is still in the stream, so the stream is out of
    // step. This client skips exactly that many octets and drops the frame.
    let (_service, mut session, mut client) = registered(Role::Agent).await;

    let oversize_length: u32 = 70_000;
    let mut bytes = oversize_length.to_be_bytes().to_vec();
    // The body looks like a valid frame start so that a wrong skip would be noticed.
    bytes.extend(std::iter::repeat_n(0x02, oversize_length as usize));
    bytes.extend_from_slice(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"next"));
    session.write_data(&bytes).await;

    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"next")).await;
    let counters = *client.counters.borrow();
    assert_eq!(counters.dropped_receive_oversize, 1);
    assert_eq!(counters.registrations, 1);
}

#[tokio::test(start_paused = true)]
async fn frame_shorter_than_an_sdu_header_is_dropped() {
    // intentional-difference:bad-header-dropped
    // Upstream throws "Invalid packet size" for a length below the SDU header size, before
    // reading the body (`al_service_access_point.cpp:235-237`). The manager drops the packet
    // (`em_mgr.cpp:427-433`), but the 5 octets of body stay unread and are taken as the next
    // length, so the stream is out of step. This client reads the body and drops the frame.
    let (_service, mut session, mut client) = registered(Role::Agent).await;

    session.write_data(&[0, 0, 0, 5, 1, 2, 3, 4, 5]).await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"ok"))
        .await;

    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"ok")).await;
    assert_eq!(client.counters.borrow().dropped_receive_malformed, 1);
}

#[tokio::test(start_paused = true)]
async fn reconnects_after_the_service_closes_data() {
    // intentional-difference:reconnect-after-disconnect
    // Upstream rethrows the closed-socket error out of the manager loop
    // (`em_mgr.cpp:427-433`), so upstream does not register again. This client reports
    // Disconnected and registers again after the reconnect delay (0010 D2).
    let (service, session, mut client) = registered(Role::Agent).await;
    drop(session);

    wait_until_disconnected(&mut client.link).await;
    let disconnected_at = tokio::time::Instant::now();

    // Before the delay has passed the client has not connected again.
    let early = tokio::time::timeout(RECONNECT_DELAY / 2, service.control.accept()).await;
    assert!(early.is_err(), "reconnected before the delay");

    let mut again = service.accept().await;
    let waited = disconnected_at.elapsed();
    assert!(
        waited < RECONNECT_DELAY + Duration::from_millis(100),
        "reconnected after {waited:?}, long after the delay of {RECONNECT_DELAY:?}"
    );
    again.respond(SERVICE_MAC, SUCCESS).await;
    assert_eq!(
        wait_until_registered(&mut client.link).await,
        registered_link(2, SERVICE_MAC)
    );
    assert_eq!(client.counters.borrow().registrations, 2);
}

#[tokio::test(start_paused = true)]
async fn sdus_sent_while_disconnected_are_dropped_not_delivered_late() {
    let (service, session, mut client) = registered(Role::Agent).await;
    drop(session);
    wait_until_disconnected(&mut client.link).await;

    let stale = out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"stale");
    client.sdus.send(stale).await.unwrap();
    wait_for_counters(&mut client, |c| c.dropped_send_disconnected == 1).await;

    let mut again = service.accept().await;
    again.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut client.link).await;

    // The first frame the service reads is the one sent after registering.
    let fresh = out(2, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"fresh");
    client.sdus.send(fresh).await.unwrap();
    let frame = again.read_data_frame().await;
    assert_eq!(&frame[19..], b"fresh");
}

#[tokio::test(start_paused = true)]
async fn sdus_left_over_from_the_old_session_are_not_written_into_the_new_one() {
    // A6 (S0-01): the registration does not drain the channel. Sent
    // together with the response, the SDUs are counted either when the actor reads them
    // while unregistered or, as stale, in the new session. They are never written.
    let (service, session, mut client) = registered(Role::Agent).await;
    drop(session);
    wait_until_disconnected(&mut client.link).await;

    let mut again = service.accept().await;
    // No await between the sends and the response, so the actor sees both together.
    send_burst(&client.sdus, 1, 5);
    let mut response = vec![0, 0, 0, 7];
    response.extend_from_slice(&SERVICE_MAC);
    response.push(SUCCESS);
    again.control.try_write(&response).unwrap();

    wait_until_registered(&mut client.link).await;
    wait_for_counters(&mut client, |c| c.dropped_send_disconnected == 5).await;

    client
        .sdus
        .send(out(2, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"fresh"))
        .await
        .unwrap();
    assert_eq!(&again.read_data_frame().await[19..], b"fresh");
    assert_eq!(client.counters.borrow().sdus_sent, 1);
}

#[tokio::test(start_paused = true)]
async fn link_already_shows_the_session_of_the_first_received_sdu() {
    // A5 test 5: the role actor reads the SDU first and only then looks at the link.
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);
    for session_number in 1..=2u8 {
        let al_mac = [0x02, 0, 0, 0, 0, 0x20 + session_number];
        let mut session = service.accept().await;
        session.respond(al_mac, SUCCESS).await;
        session
            .write_data(&sdu_frame(al_mac, PEER_MAC, UNFRAGMENTED, b"first"))
            .await;

        let first = next_received(&mut client.received).await;
        assert_eq!(first.local, MacAddr::new(al_mac));
        assert_eq!(
            *client.link.borrow(),
            registered_link(u64::from(session_number), al_mac)
        );
        drop(session);
        wait_until_disconnected(&mut client.link).await;
    }
}

/// SDUs of one frame each, whose payload is the sequence number from `first` on.
fn numbered_sdu_frames(first: usize, count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in first..first + count {
        let tag = u16::try_from(index).unwrap().to_be_bytes();
        bytes.extend_from_slice(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, &tag));
    }
    bytes
}

fn tag_of(sdu: &Sdu) -> u16 {
    u16::from_be_bytes([sdu.cmdu[0], sdu.cmdu[1]])
}

/// Reads everything the actor still holds for the role actor: the loop ends when nothing
/// more arrives.
async fn read_all_received(client: &mut Client) -> Vec<Sdu> {
    let mut all = Vec::new();
    while let Ok(Some(sdu)) =
        tokio::time::timeout(Duration::from_millis(50), client.received.recv()).await
    {
        all.push(sdu);
    }
    all
}

#[tokio::test(start_paused = true)]
async fn full_received_channel_drops_the_oldest_and_counts_them() {
    const SENT: usize = 600;
    let (_service, mut session, mut client) = registered(Role::Agent).await;

    // The role actor does not read while the service sends.
    session.write_data(&numbered_sdu_frames(0, SENT)).await;
    wait_for_counters(&mut client, |c| c.sdus_received == SENT as u64).await;

    let tags: Vec<u16> = read_all_received(&mut client)
        .await
        .iter()
        .map(tag_of)
        .collect();

    // The first 64 went into the channel. The actor kept the newest 256 of the other 536
    // and dropped the 280 oldest of them.
    assert_eq!(client.counters.borrow().dropped_receive_overflow, 280);
    assert_eq!(tags.len(), 64 + 256);
    assert_eq!(tags[63], 63, "the channel holds the first 64");
    assert_eq!(tags[64], 344, "the queue holds the newest 256");
    assert!(tags.windows(2).all(|pair| pair[0] < pair[1]), "order kept");
    assert_eq!(*tags.last().unwrap(), u16::try_from(SENT - 1).unwrap());
}

#[tokio::test(start_paused = true)]
async fn many_reconnects_with_an_unread_receiver_keep_the_newest_and_the_last_link() {
    // A5 test: more than 64 + 256 SDUs over several sessions, a different AL MAC each.
    const SESSIONS: usize = 4;
    const PER_SESSION: usize = 100;
    const TOTAL: usize = SESSIONS * PER_SESSION;
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);

    let mut last_mac = [0; 6];
    let mut last_session = None;
    for number in 1..=SESSIONS {
        let al_mac = [0x02, 0, 0, 0, 0, u8::try_from(0x10 + number).unwrap()];
        let mut session = service.accept().await;
        session.respond(al_mac, SUCCESS).await;
        session
            .write_data(&numbered_sdu_frames(
                (number - 1) * PER_SESSION,
                PER_SESSION,
            ))
            .await;
        let received_so_far = (number * PER_SESSION) as u64;
        wait_for_counters(&mut client, |c| c.sdus_received == received_so_far).await;
        last_mac = al_mac;
        if number < SESSIONS {
            drop(session);
            wait_until_disconnected(&mut client.link).await;
        } else {
            last_session = Some(session);
        }
    }

    // 64 sit in the channel, and the queue keeps the newest 256 of the other 336.
    let overflow = (TOTAL - 64 - 256) as u64;
    assert_eq!(client.counters.borrow().dropped_receive_overflow, overflow);
    assert_eq!(
        *client.link.borrow(),
        registered_link(SESSIONS as u64, last_mac)
    );

    let tags: Vec<u16> = read_all_received(&mut client)
        .await
        .iter()
        .map(tag_of)
        .collect();
    let first_64 = (0..64).map(|tag| u16::try_from(tag).unwrap());
    let newest_256 = (TOTAL - 256..TOTAL).map(|tag| u16::try_from(tag).unwrap());
    let expected: Vec<u16> = first_64.chain(newest_256).collect();
    assert_eq!(tags, expected);
    drop(last_session);
}

#[tokio::test(start_paused = true)]
async fn actor_ends_after_a_linger_when_the_sender_is_dropped() {
    let (_service, mut session, client) = registered(Role::Agent).await;
    let Client { sdus, handle, .. } = client;

    sdus.send(out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"last"))
        .await
        .unwrap();
    drop(sdus);

    // The last write still left before the actor ended.
    let frame = session.read_data_frame().await;
    assert_eq!(&frame[19..], b"last");
    assert!(join(handle).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn actor_ends_when_the_sender_is_dropped_while_not_registered() {
    let service = FakeService::new();
    let client = start(&service, Role::Agent);
    // Accepted but never answered: the client waits for the response.
    let _session = service.accept().await;
    drop(client.sdus);
    assert!(join(client.handle).await.is_ok());
}
// ---------------------------------------------------------------------------
// Stopping the actor
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn actor_stays_for_the_linger_after_the_sender_is_dropped_and_then_ends() {
    let (_service, _session, client) = registered(Role::Agent).await;
    let Client { sdus, handle, .. } = client;

    drop(sdus);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!handle.is_finished(), "ended before the linger of 100 ms");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(handle.is_finished(), "still running 150 ms after the drop");
    assert!(join(handle).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn service_sees_no_end_of_file_until_the_linger_has_passed() {
    let (_service, mut session, client) = registered(Role::Agent).await;
    drop(client.sdus);

    let data_eof = tokio::time::timeout(Duration::from_millis(50), read_one(&mut session.data));
    assert!(
        data_eof.await.is_err(),
        "the data socket closed during the linger"
    );
    let control_eof =
        tokio::time::timeout(Duration::from_millis(10), read_one(&mut session.control));
    assert!(
        control_eof.await.is_err(),
        "the control socket closed during the linger"
    );

    // After the linger both close.
    assert_eq!(read_one(&mut session.data).await, 0);
    assert_eq!(read_one(&mut session.control).await, 0);
}

const LINGER: Duration = Duration::from_millis(100);

/// Sends `count` one-octet SDUs without yielding, so they all sit in the channel.
fn send_burst(sdus: &mpsc::Sender<OutboundSdu>, session: u64, count: u8) {
    for index in 0..count {
        sdus.try_send(out(session, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, &[index]))
            .unwrap_or_else(|error| panic!("SDU {index} did not fit: {error}"));
    }
}

/// Reads `count` frames from the service and checks they carry the payloads 0, 1, 2, ... in order.
async fn expect_burst(session: &mut ServiceSession, count: u8) {
    for index in 0..count {
        let frame = session.read_data_frame().await;
        assert_eq!(&frame[19..], [index], "frame {index}");
    }
}

/// Checks that the service sees no end of file for most of the linger, then one, and that
/// the whole linger has passed since `dropped_at`.
async fn expect_linger_then_end_of_file(
    session: &mut ServiceSession,
    dropped_at: tokio::time::Instant,
) {
    let early_eof = tokio::time::timeout(LINGER / 2, read_one(&mut session.data));
    assert!(
        early_eof.await.is_err(),
        "the data socket closed during the linger"
    );
    assert_eq!(read_one(&mut session.data).await, 0);
    assert!(
        dropped_at.elapsed() >= LINGER,
        "end of file after only {:?}",
        dropped_at.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn dropping_both_ends_in_one_poll_still_writes_every_queued_sdu_and_lingers() {
    // A5 test 1: 64 SDUs, then the sender and the receiver dropped in the same poll.
    let (_service, mut session, client) = registered(Role::Agent).await;
    let Client {
        sdus,
        received,
        counters,
        handle,
        ..
    } = client;

    send_burst(&sdus, 1, 64);
    drop((sdus, received));
    let dropped_at = tokio::time::Instant::now();

    expect_burst(&mut session, 64).await;
    expect_linger_then_end_of_file(&mut session, dropped_at).await;
    assert!(join(handle).await.is_ok());
    assert_eq!(counters.borrow().sdus_sent, 64);
}

#[tokio::test(start_paused = true)]
async fn dropping_only_the_receiver_writes_the_queued_sdus_and_lingers() {
    // A5 test 2, first half: the sender is held while the receiver goes.
    let (_service, mut session, client) = registered(Role::Agent).await;
    let Client {
        sdus,
        received,
        handle,
        ..
    } = client;

    send_burst(&sdus, 1, 5);
    drop(received);
    let dropped_at = tokio::time::Instant::now();

    expect_burst(&mut session, 5).await;
    expect_linger_then_end_of_file(&mut session, dropped_at).await;
    assert!(join(handle).await.is_ok());
    drop(sdus);
}

#[tokio::test(start_paused = true)]
async fn sends_after_the_stop_began_fail_and_are_not_written() {
    // A6 (S0-03, S0-04): the actor closes the send channel when the stop begins, so a send
    // after that fails and is not written.
    let (_service, mut session, client) = registered(Role::Agent).await;
    let Client {
        sdus,
        received,
        counters,
        handle,
        ..
    } = client;

    drop(received);
    advance(LINGER / 2).await;
    let late = out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"late");
    assert!(matches!(sdus.try_send(late), Err(TrySendError::Closed(_))));
    let later = out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"later");
    assert!(sdus.send(later).await.is_err());

    // The service sees end of file, with no frame before it: `read_one` would return 1.
    assert_eq!(read_one(&mut session.data).await, 0);
    assert!(join(handle).await.is_ok());
    assert_eq!(counters.borrow().sdus_sent, 0);
}

#[tokio::test(start_paused = true)]
async fn permit_held_across_the_stop_and_used_in_the_linger_is_counted_not_written() {
    // A6 (S0-04): tokio keeps a closed channel open for a permit already reserved.
    let (_service, mut session, client) = registered(Role::Agent).await;
    let Client {
        sdus,
        received,
        counters,
        handle,
        ..
    } = client;

    let permit = sdus.reserve().await.unwrap();
    drop(received);
    advance(LINGER / 2).await;
    permit.send(out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"late"));

    assert_eq!(read_one(&mut session.data).await, 0);
    assert!(join(handle).await.is_ok());
    let counters = *counters.borrow();
    assert_eq!(counters.dropped_send_disconnected, 1);
    assert_eq!(counters.sdus_sent, 0);
}

#[tokio::test(start_paused = true)]
async fn permit_held_across_a_stop_while_unregistered_is_waited_for_for_at_most_the_linger() {
    // A6 (S0-04): while not registered the stop ends at once, unless a permit is outstanding.
    let service = FakeService::new();
    let Client {
        sdus,
        received,
        counters,
        handle,
        ..
    } = start(&service, Role::Agent);
    let _session = service.accept().await;

    let permit = sdus.reserve().await.unwrap();
    drop(received);
    advance(LINGER / 2).await;
    assert!(!handle.is_finished(), "ended while a permit was held");
    permit.send(out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"late"));
    assert!(join(handle).await.is_ok());
    assert_eq!(counters.borrow().dropped_send_disconnected, 1);
}

#[tokio::test(start_paused = true)]
async fn permit_that_is_never_used_cannot_hold_a_stop_longer_than_the_linger() {
    let service = FakeService::new();
    let Client {
        sdus,
        received,
        handle,
        ..
    } = start(&service, Role::Agent);
    let _session = service.accept().await;

    let _permit = sdus.reserve().await.unwrap();
    drop(received);
    let dropped_at = tokio::time::Instant::now();
    assert!(join(handle).await.is_ok());
    assert!(dropped_at.elapsed() >= LINGER);
    assert!(dropped_at.elapsed() < LINGER * 2);
}

#[tokio::test(start_paused = true)]
async fn sdus_arriving_during_a_fatal_registration_are_counted_and_the_channel_is_closed() {
    // A6 (S0-03): a permit held across the error is used after the actor has left the wait.
    let service = FakeService::new();
    let Client {
        sdus,
        received: _received,
        counters,
        handle,
        ..
    } = start(&service, Role::Controller);
    let mut session = service.accept().await;

    let permit = sdus.reserve().await.unwrap();
    session.respond(SERVICE_MAC, 0x02).await;
    advance(LINGER / 2).await;
    assert!(!handle.is_finished(), "ended while a permit was held");
    assert!(
        sdus.is_closed(),
        "the channel is still open after the error"
    );
    permit.send(out(0, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"late"));

    let error = join(handle).await.unwrap_err();
    assert!(matches!(
        error,
        TransportError::RegistrationRejected { result: 0x02 }
    ));
    assert_eq!(counters.borrow().dropped_send_disconnected, 1);
}

/// Closes the service's write side of the data socket, so that the client reads end of
/// file while the service still reads what the client writes.
fn close_service_write_side(session: &mut ServiceSession) {
    // A placeholder takes the socket's place while it is converted and back.
    let placeholder = tokio::net::UnixStream::pair().unwrap().0;
    let data = std::mem::replace(&mut session.data, placeholder);
    let data = data.into_std().unwrap();
    data.shutdown(std::net::Shutdown::Write).unwrap();
    session.data = tokio::net::UnixStream::from_std(data).unwrap();
}

#[tokio::test(start_paused = true)]
async fn link_ends_as_the_last_session_without_an_al_mac_after_each_kind_of_stop() {
    // A6 (S0-06): `None` is published after the linger, never before it.
    // The three stops: sender dropped, both ends dropped, end of file with a dropped receiver.
    for kind in 0..3 {
        let (_service, mut session, client) = registered(Role::Agent).await;
        let Client {
            sdus,
            received,
            link,
            handle,
            ..
        } = client;
        match kind {
            0 => drop(sdus),
            1 => drop((sdus, received)),
            _ => {
                close_service_write_side(&mut session);
                drop((sdus, received));
            }
        }
        advance(LINGER / 2).await;
        assert_eq!(
            *link.borrow(),
            registered_link(1, SERVICE_MAC),
            "kind {kind}: link changed during the linger"
        );
        assert!(join(handle).await.is_ok());
        let last = LinkState {
            session: 1,
            al_mac: None,
        };
        assert_eq!(*link.borrow(), last, "kind {kind}");
    }
}

#[tokio::test(start_paused = true)]
async fn unfinished_fragment_set_is_counted_when_the_session_ends_with_a_stop() {
    // A6 (S0-05)
    let (_service, mut session, client) = registered(Role::Agent).await;
    let Client {
        sdus,
        counters,
        handle,
        ..
    } = client;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 0], b"ab"))
        .await;
    advance(Duration::from_millis(10)).await;

    drop(sdus);
    assert!(join(handle).await.is_ok());
    assert_eq!(counters.borrow().dropped_receive_fragments, 1);
}

/// Counts what is left in the received channel after the actor has ended.
async fn count_until_closed(received: &mut mpsc::Receiver<Sdu>) -> u64 {
    let mut count = 0;
    while bounded("the received channel to close", received.recv())
        .await
        .is_some()
    {
        count += 1;
    }
    count
}

#[tokio::test(start_paused = true)]
async fn received_sdus_left_in_the_queue_when_the_sender_is_dropped_are_counted() {
    // A6: sdus_received = moved into the channel + overflow + stopped.
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    fill_the_received_channel(&mut session, &mut client).await;
    let Client {
        sdus,
        mut received,
        counters,
        handle,
        ..
    } = client;

    drop(sdus);
    assert!(join(handle).await.is_ok());
    let in_channel = count_until_closed(&mut received).await;
    let counters = *counters.borrow();
    assert!(counters.dropped_receive_stopped > 0);
    assert_eq!(
        counters.sdus_received,
        in_channel + counters.dropped_receive_overflow + counters.dropped_receive_stopped
    );
}

#[tokio::test(start_paused = true)]
async fn received_sdus_left_in_the_queue_at_a_fatal_registration_error_are_counted() {
    let (service, mut session, mut client) = registered(Role::Agent).await;
    fill_the_received_channel(&mut session, &mut client).await;
    let Client {
        sdus: _sdus,
        mut received,
        mut link,
        counters,
        handle,
    } = client;

    // The session ends with SDUs still queued, and the next registration is rejected.
    drop(session);
    wait_until_disconnected(&mut link).await;
    let mut again = service.accept().await;
    again.respond(SERVICE_MAC, 0x02).await;
    assert!(join(handle).await.is_err());

    let in_channel = count_until_closed(&mut received).await;
    let counters = *counters.borrow();
    assert!(counters.dropped_receive_stopped > 0);
    assert_eq!(
        counters.sdus_received,
        in_channel + counters.dropped_receive_overflow + counters.dropped_receive_stopped
    );
}

#[tokio::test]
async fn receiver_dropped_while_a_sender_keeps_sending_still_ends_the_actor() {
    // A5 test 2, second half. Real time and a producer on its own thread: every drain
    // is bounded by the channel's capacity, so the actor cannot be held.
    let (_service, session, client) = registered(Role::Agent).await;
    let ServiceSession { control, data, .. } = session;
    let (mut service_read, service_write) = data.into_split();
    let drain = tokio::spawn(async move {
        let mut buffer = [0u8; 4096];
        while matches!(service_read.read(&mut buffer).await, Ok(count) if count > 0) {}
    });

    let Client {
        sdus,
        received,
        handle,
        ..
    } = client;
    let producer = std::thread::spawn(move || {
        loop {
            match sdus.try_send(out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"x")) {
                Err(TrySendError::Closed(_)) => break,
                Ok(()) | Err(TrySendError::Full(_)) => {}
            }
        }
    });

    drop(received);
    assert!(join_within(Duration::from_secs(5), handle).await.is_ok());
    producer.join().unwrap();
    drain.abort();
    drop((control, service_write));
}

#[tokio::test(start_paused = true)]
async fn dropping_the_link_and_counters_watches_stops_nothing() {
    // A5 test 3
    let (_service, mut session, client) = registered(Role::Agent).await;
    let Client {
        sdus,
        mut received,
        link,
        counters,
        handle,
    } = client;
    drop((link, counters));

    sdus.send(out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"out"))
        .await
        .unwrap();
    assert_eq!(&session.read_data_frame().await[19..], b"out");

    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"in"))
        .await;
    assert_eq!(
        next_received(&mut received).await,
        sdu(SERVICE_MAC, PEER_MAC, b"in")
    );

    // Still running after a while.
    advance(Duration::from_secs(1)).await;
    assert!(!handle.is_finished());
}

#[tokio::test(start_paused = true)]
async fn receiver_dropped_while_unregistered_ends_at_once_and_counts_the_queued_sdus() {
    let service = FakeService::new();
    let Client {
        sdus,
        received,
        counters,
        handle,
        ..
    } = start(&service, Role::Agent);
    // Accepted but never answered: the actor waits for the response.
    let _session = service.accept().await;

    send_burst(&sdus, 1, 3);
    drop(received);
    let dropped_at = tokio::time::Instant::now();

    assert!(join(handle).await.is_ok());
    assert!(dropped_at.elapsed() < LINGER, "unregistered: no linger");
    assert_eq!(counters.borrow().dropped_send_disconnected, 3);
    drop(sdus);
}

// The tests that stop the actor by dropping the received receiver run in real time. If the
// actor does not stop, a paused clock would never reach the limit, because the actor keeps
// the runtime busy.
const STOP_LIMIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn actor_ends_ok_when_the_receiver_is_dropped_while_waiting_for_the_response() {
    let service = FakeService::new();
    let Client {
        sdus: _sdus,
        received,
        handle,
        ..
    } = start(&service, Role::Agent);
    // Accepted but never answered: the actor waits for the response.
    let _session = service.accept().await;

    drop(received);
    assert!(join_within(STOP_LIMIT, handle).await.is_ok());
}

#[tokio::test]
async fn actor_ends_ok_when_the_receiver_is_dropped_while_registered_and_idle() {
    let (_service, _session, client) = registered(Role::Agent).await;
    let Client {
        sdus: _sdus,
        received,
        handle,
        ..
    } = client;

    drop(received);
    assert!(join_within(STOP_LIMIT, handle).await.is_ok());
}

/// Sends enough SDUs that the received channel is full and SDUs wait in the actor.
async fn fill_the_received_channel(session: &mut ServiceSession, client: &mut Client) {
    const SENT: usize = 100;
    let mut bytes = Vec::new();
    for _ in 0..SENT {
        bytes.extend_from_slice(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"x"));
    }
    session.write_data(&bytes).await;
    wait_for_counters(client, |c| c.sdus_received == SENT as u64).await;
}

#[tokio::test]
async fn actor_ends_ok_when_the_receiver_is_dropped_with_sdus_waiting_in_a_session() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    fill_the_received_channel(&mut session, &mut client).await;
    let Client {
        sdus: _sdus,
        received,
        handle,
        ..
    } = client;

    drop(received);
    assert!(join_within(STOP_LIMIT, handle).await.is_ok());
}

#[tokio::test]
async fn actor_ends_ok_when_the_receiver_is_dropped_with_sdus_waiting_after_a_disconnect() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    fill_the_received_channel(&mut session, &mut client).await;
    let Client {
        sdus: _sdus,
        received,
        handle,
        ..
    } = client;

    // The actor ends the session and waits for the reconnect delay with SDUs still queued.
    drop(session);
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(received);
    assert!(join_within(STOP_LIMIT, handle).await.is_ok());
}

/// Real time and a producer on its own thread: on one thread the producer would only run
/// when the actor yields, which hides the starvation.
#[tokio::test]
async fn continuously_ready_sender_does_not_starve_reads() {
    let (_service, session, client) = registered(Role::Agent).await;
    let ServiceSession { control, data, .. } = session;
    let (mut service_read, mut service_write) = data.into_split();

    // The service drains what the client writes, so that writes never stall.
    let drain = tokio::spawn(async move {
        let mut buffer = [0u8; 4096];
        while matches!(service_read.read(&mut buffer).await, Ok(count) if count > 0) {}
    });

    let Client {
        sdus,
        mut received,
        handle,
        ..
    } = client;
    // The producer refills the channel the moment a slot frees, so the actor always has
    // an SDU ready. It ends when the actor does and drops the receiver.
    let producer = std::thread::spawn(move || {
        let local = [0x02, 0, 0, 0, 0, 0x0a];
        loop {
            match sdus.try_send(out(1, local, PEER_MAC, b"x")) {
                Err(TrySendError::Closed(_)) => break,
                Ok(()) | Err(TrySendError::Full(_)) => {}
            }
        }
    });

    for index in 0..20u8 {
        let frame = sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, &[index]);
        service_write.write_all(&frame).await.unwrap();
        let received_sdu = tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap_or_else(|_| panic!("frame {index} was not delivered under sustained sends"));
        assert_eq!(received_sdu, Some(sdu(SERVICE_MAC, PEER_MAC, &[index])));
    }

    drop(received);
    assert!(join_within(STOP_LIMIT, handle).await.is_ok());
    producer.join().unwrap();
    drain.abort();
    drop((control, service_write));
}

#[tokio::test(start_paused = true)]
async fn control_socket_stays_open_during_a_session() {
    let (_service, mut session, _client) = registered(Role::Agent).await;
    let read = tokio::time::timeout(Duration::from_secs(1), read_one(&mut session.control)).await;
    assert!(read.is_err(), "the client closed the control socket");
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn sdu_channel_takes_a_burst_of_64_without_waiting() {
    let (_service, _session, client) = registered(Role::Agent).await;
    for index in 0..64u8 {
        client
            .sdus
            .try_send(out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, &[index]))
            .unwrap_or_else(|error| panic!("SDU {index} did not fit: {error}"));
    }
}

#[tokio::test(start_paused = true)]
async fn sdu_over_256_fragments_is_dropped_and_the_session_goes_on() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    let local = [0x02, 0, 0, 0, 0, 0x0a];

    let too_big = vec![0u8; MAX_SDU_PAYLOAD + 1];
    client
        .sdus
        .send(out(1, local, PEER_MAC, &too_big))
        .await
        .unwrap();
    wait_for_counters(&mut client, |c| c.dropped_send_failed >= 1).await;

    // The session is still up: the next SDU goes out.
    client
        .sdus
        .send(out(1, local, PEER_MAC, b"next"))
        .await
        .unwrap();
    let frame = session.read_data_frame().await;
    assert_eq!(&frame[19..], b"next");

    wait_for_counters(&mut client, |c| c.sdus_sent >= 1).await;
    let counters = *client.counters.borrow();
    assert_eq!(counters.dropped_send_failed, 1);
    assert_eq!(counters.sdus_sent, 1);
    assert_eq!(counters.registrations, 1);
}

#[tokio::test(start_paused = true)]
async fn failed_write_ends_the_session_and_counts_one_failed_send() {
    let (_service, session, mut client) = registered(Role::Agent).await;
    let ServiceSession { control, data, .. } = session;
    // The service stops reading its data socket but keeps it open, so the client's reads
    // see no end of file and only its writes fail.
    let data = data.into_std().unwrap();
    data.shutdown(std::net::Shutdown::Read).unwrap();

    client
        .sdus
        .send(out(1, [0x02, 0, 0, 0, 0, 0x0a], PEER_MAC, b"lost"))
        .await
        .unwrap();

    wait_until_disconnected(&mut client.link).await;
    let counters = *client.counters.borrow();
    assert_eq!(counters.dropped_send_failed, 1);
    assert_eq!(counters.sdus_sent, 0);
    drop((control, data));
}

// ---------------------------------------------------------------------------
// Receiving
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn incomplete_fragment_set_is_counted_when_the_session_ends() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 0], b"ab"))
        .await;
    drop(session);

    wait_until_disconnected(&mut client.link).await;
    assert_eq!(client.counters.borrow().dropped_receive_fragments, 1);
}

#[tokio::test(start_paused = true)]
async fn frame_with_an_empty_payload_is_delivered() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b""))
        .await;
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"")).await;
}

// ---------------------------------------------------------------------------
// Registration and the reconnect loop
// ---------------------------------------------------------------------------

#[test]
fn default_reconnect_delay_is_two_seconds() {
    let config = TransportConfig::new("control".into(), "data".into(), Role::Agent);
    assert_eq!(config.reconnect_delay, Duration::from_secs(2));
}

/// With the control socket live and the data socket not usable, every attempt reaches
/// the control listener once, one reconnect delay apart.
async fn attempts_are_one_reconnect_delay_apart(data_file_is_stale: bool) {
    // intentional-difference:retry-connect (see `retries_while_the_socket_files_are_missing`)
    let dir = TempDir::new();
    let control_path = dir.path().join("control");
    let data_path = dir.path().join("data");
    let control = UnixListener::bind(&control_path).unwrap();
    if data_file_is_stale {
        drop(UnixListener::bind(&data_path).unwrap());
    }
    let mut config = TransportConfig::new(control_path, dir.path().join("data"), Role::Agent);
    config.reconnect_delay = RECONNECT_DELAY;
    let mut transport = spawn(config);

    settle().await;
    assert_eq!(take_pending_connections(&control).await, 1, "first attempt");
    // One millisecond short of RECONNECT_DELAY.
    advance(Duration::from_millis(1999)).await;
    assert_eq!(
        take_pending_connections(&control).await,
        0,
        "attempt before the reconnect delay"
    );
    advance(Duration::from_millis(1)).await;
    assert_eq!(
        take_pending_connections(&control).await,
        1,
        "second attempt"
    );
    advance(RECONNECT_DELAY).await;
    assert_eq!(take_pending_connections(&control).await, 1, "third attempt");

    // The data socket comes up: the next attempt registers.
    if data_file_is_stale {
        std::fs::remove_file(&data_path).unwrap();
    }
    let data = UnixListener::bind(&data_path).unwrap();
    let live = FakeService { dir, control, data };
    let mut session = live.accept().await;
    session.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut transport.link).await;
}

#[tokio::test(start_paused = true)]
async fn missing_data_socket_is_retried_one_reconnect_delay_apart() {
    attempts_are_one_reconnect_delay_apart(false).await;
}

#[tokio::test(start_paused = true)]
async fn stale_data_socket_file_is_retried_one_reconnect_delay_apart() {
    attempts_are_one_reconnect_delay_apart(true).await;
}

#[tokio::test(start_paused = true)]
async fn control_is_connected_before_data() {
    let dir = TempDir::new();
    let control = UnixListener::bind(dir.path().join("control")).unwrap();
    let mut config = TransportConfig::new(
        dir.path().join("control"),
        dir.path().join("data"),
        Role::Agent,
    );
    config.reconnect_delay = RECONNECT_DELAY;
    let mut transport = spawn(config);

    // The data socket does not exist yet. The first attempt still reaches the control
    // socket, because that one is connected first.
    let first = tokio::time::timeout(RECONNECT_DELAY / 2, control.accept()).await;
    let (mut first_control, _) = first.expect("the data socket was connected first").unwrap();
    // The client closes it when it finds no data socket. Only then is the attempt over
    // and it safe to bring the data socket up.
    assert_eq!(read_one(&mut first_control).await, 0);

    let data = UnixListener::bind(dir.path().join("data")).unwrap();
    let live = FakeService { dir, control, data };
    let mut session = live.accept().await;
    session.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut transport.link).await;
}

#[tokio::test(start_paused = true)]
async fn socket_path_the_system_rejects_ends_the_actor_with_an_io_error() {
    // A path above the length of `sun_path` is invalid input, not a service restart.
    let dir = TempDir::new();
    let config = TransportConfig::new(
        dir.path().join("x".repeat(200)),
        dir.path().join("data"),
        Role::Agent,
    );
    let transport = spawn(config);

    let error = join(transport.task).await.unwrap_err();
    let TransportError::Io { operation, source } = error else {
        panic!("expected an I/O error, got {error:?}");
    };
    assert_eq!(operation, "connect control socket");
    assert_eq!(source.kind(), std::io::ErrorKind::InvalidInput);
}

#[tokio::test(start_paused = true)]
async fn connection_reset_before_the_response_is_retried() {
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);

    // The request stays unread: closing the control socket with data in its queue
    // makes the client's read fail with a reset.
    let (control, data) = service.accept_without_reading().await;
    bounded("the request to arrive", control.readable())
        .await
        .unwrap();
    drop((control, data));

    let mut second = service.accept().await;
    second.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn response_frame_above_the_frame_limit_is_retried() {
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);

    let mut first = service.accept().await;
    let mut response = 70_000u32.to_be_bytes().to_vec();
    response.extend(std::iter::repeat_n(0x02, 70_000));
    first.write_control(&response).await;

    let mut second = service.accept().await;
    second.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn response_shorter_than_seven_octets_is_retried() {
    let service = FakeService::new();
    let mut client = start(&service, Role::Agent);

    let mut first = service.accept().await;
    first.write_control(&[0, 0, 0, 6, 2, 0, 0, 0, 0, 1]).await;

    let mut second = service.accept().await;
    second.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn rejection_wins_over_an_all_zero_al_mac() {
    let service = FakeService::new();
    let client = start(&service, Role::Controller);
    let mut session = service.accept().await;
    session.respond([0; 6], 0x02).await;

    let error = join(client.handle).await.unwrap_err();
    assert!(
        matches!(error, TransportError::RegistrationRejected { result: 0x02 }),
        "unexpected error {error:?}"
    );
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[test]
fn rejected_registration_displays_the_result_in_hex() {
    let error = TransportError::RegistrationRejected { result: 0x02 };
    assert_eq!(
        error.to_string(),
        "registration rejected by the service, result 0x02"
    );
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn io_error_displays_the_operation_and_has_its_source() {
    let error = TransportError::Io {
        operation: "connect control socket",
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path too long"),
    };
    assert_eq!(error.to_string(), "connect control socket: path too long");
    let source = std::error::Error::source(&error).expect("the I/O error is the source");
    assert_eq!(source.to_string(), "path too long");
}
