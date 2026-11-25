// SPDX-License-Identifier: Apache-2.0

//! Tests of the transport on the event loop, with real UNIX sockets and a scripted fake service
//! (0016 D2, D8 step 4b-3). Every scenario of `session.rs` that still has a meaning on the loop
//! is ported here. `session.rs` tested the old actor; it was deleted in step 5b and can be read
//! at `b6995f2` (`crates/em-transport/tests/session.rs`); the harness is in `loop_support`. Tests tagged
//! `intentional-difference:<id>` pin a row of the 0016 table of intentional differences or a
//! place where this client does not do what upstream's does (0011 D3).
//!
//! # How `session.rs` maps to this file (66 tests)
//!
//! The harness differs from the actor's: the stop signal is dropping `sdus` (the application
//! channel closing, `AppClosed`); dropping `received` is no signal. The received SDUs reach the
//! test through a channel of 64 that the role fills only while it has room, and the test wakes the
//! loop after it reads (`LoopClient::next_received`, `drain_received`).
//!
//! Ported, same meaning (47; 4 of them renamed, the new name is given): `registration_request_bytes_for_both_roles`,
//! `registered_link_state_carries_the_al_mac_and_the_session`,
//! `rejected_registration_ends_the_actor_with_the_result` (here `..._ends_the_loop_with_the_result`),
//! `rejected_registration_is_fatal_for_the_agent_role_too`,
//! `response_split_across_two_reads_is_waited_for`,
//! `retries_while_the_socket_files_have_no_listener`, `retries_while_the_socket_files_are_missing`,
//! `retries_after_end_of_file_before_the_response`, `all_zero_al_mac_is_retried_not_used`,
//! `sent_sdu_bytes_follow_the_sdu_codec`, `address_mapping_in_both_directions`,
//! `payload_of_65517_is_one_frame_and_65518_is_two`, `received_fragments_are_joined`,
//! `fragment_out_of_order_is_dropped_and_id_zero_resyncs`,
//! `continuation_from_another_peer_is_dropped_and_the_session_goes_on`,
//! `fragment_set_over_1_mib_is_refused_counted_once_and_the_session_goes_on`,
//! `fragment_set_of_exactly_1_mib_is_delivered`,
//! `fragment_set_of_1_mib_plus_one_octet_is_refused_and_the_session_goes_on` (these three tagged
//! `rx-sdu-cap-1mib`), `oversize_frame_is_skipped_and_the_stream_stays_in_step`,
//! `frame_shorter_than_an_sdu_header_is_dropped`, `reconnects_after_the_service_closes_data`,
//! `sdus_sent_while_disconnected_are_dropped_not_delivered_late`,
//! `sdus_left_over_from_the_old_session_are_not_written_into_the_new_one`,
//! `link_already_shows_the_session_of_the_first_received_sdu`,
//! `actor_ends_when_the_sender_is_dropped_while_not_registered` (here `loop_ends_...`),
//! `actor_stays_for_the_linger_after_the_sender_is_dropped_and_then_ends` (here `loop_stays_...`),
//! `service_sees_no_end_of_file_until_the_linger_has_passed`,
//! `unfinished_fragment_set_is_counted_when_the_session_ends_with_a_stop`,
//! `received_sdus_left_in_the_queue_when_the_sender_is_dropped_are_counted`,
//! `received_sdus_left_in_the_queue_at_a_fatal_registration_error_are_counted`,
//! `dropping_the_link_and_counters_watches_stops_nothing`,
//! `continuously_ready_sender_does_not_starve_reads` (tagged `rx-read-between-passes`),
//! `control_socket_stays_open_during_a_session`,
//! `sdu_over_256_fragments_is_dropped_and_the_session_goes_on`,
//! `incomplete_fragment_set_is_counted_when_the_session_ends`,
//! `frame_with_an_empty_payload_is_delivered`, `default_reconnect_delay_is_two_seconds`,
//! `missing_data_socket_is_retried_one_reconnect_delay_apart`,
//! `stale_data_socket_file_is_retried_one_reconnect_delay_apart`,
//! `control_is_connected_before_data`, `socket_path_the_system_rejects_ends_the_actor_with_an_io_error`
//! (here `..._ends_the_loop_...`), `connection_reset_before_the_response_is_retried`,
//! `response_frame_above_the_frame_limit_is_retried`,
//! `response_shorter_than_seven_octets_is_retried`, `rejection_wins_over_an_all_zero_al_mac`,
//! `rejected_registration_displays_the_result_in_hex`,
//! `io_error_displays_the_operation_and_has_its_source` (the last three, with the default delay,
//! test no loop; they are copied so that deleting `session.rs` with the actor loses nothing).
//!
//! Changed by the counting rule of 0016 D3b (1): `failed_write_ends_the_session_and_counts_one_failed_send`
//! -> `failed_write_ends_the_session_and_counts_the_unstarted_sdu_as_disconnected`.
//!
//! Changed by an intentional difference (0016 table), ported to the new rule (5):
//! * `full_received_channel_drops_the_oldest_and_counts_them` -> `rx-queue-1024-4mib`: the queue
//!   keeps the newest 1024 SDUs, was 256 (+64 in the channel):
//!   `received_queue_keeps_the_newest_1024_sdus_and_counts_the_rest`.
//! * `many_reconnects_with_an_unread_receiver_keep_the_newest_and_the_last_link` ->
//!   `rx-queue-1024-4mib`: `many_reconnects_with_an_unread_role_keep_the_newest_and_the_last_link`
//!   (1600 SDUs over 4 sessions, the queue keeps 1024 of them).
//! * `sdu_channel_takes_a_burst_of_64_without_waiting` -> `tx-buffer`: the send channel's burst of
//!   64 is no longer the bound (here it is the harness channel), the outbound buffer is:
//!   `outbound_buffer_of_4_mib_drops_the_oldest_unstarted_sdus_and_counts_them`.
//! * `dropping_both_ends_in_one_poll_still_writes_every_queued_sdu_and_lingers` ->
//!   `dropping_the_sender_after_a_burst_of_64_still_writes_every_queued_sdu_and_lingers` (only
//!   the sender is a stop signal).
//! * `receiver_dropped_while_unregistered_ends_at_once_and_counts_the_queued_sdus` ->
//!   `sender_dropped_while_unregistered_ends_at_once_and_counts_the_queued_sdus`.
//!
//! New tests for the other differences and for the stop bound: `tx-write-stall-5s`:
//! `write_stall_of_5_seconds_ends_the_session_counts_it_and_reconnects`; `rx-queue-1024-4mib`
//! byte limit: `received_queue_keeps_at_most_4_mib_of_cmdu`; `rx-read-between-passes`, the other
//! direction: `continuously_ready_reads_do_not_starve_the_sender`; U8:
//! `stop_whose_writes_cannot_finish_ends_within_900_ms_plus_the_linger`. Busy application channel
//! (0016 E11 item 7): `registration_completes_while_the_application_channel_is_always_busy`,
//! `blocked_write_resumes_while_the_application_channel_is_always_busy`.
//!
//! Merged into a test above (a variant that only differs in what drops, 5):
//! * `actor_ends_after_a_linger_when_the_sender_is_dropped` -> `dropping_the_sender_writes_the_queued_sdus_lingers_and_ends_ok`.
//! * `actor_ends_ok_when_the_receiver_is_dropped_while_waiting_for_the_response` ->
//!   `loop_ends_when_the_sender_is_dropped_while_not_registered`.
//! * `actor_ends_ok_when_the_receiver_is_dropped_while_registered_and_idle` ->
//!   `loop_stays_for_the_linger_after_the_sender_is_dropped_and_then_ends`.
//! * `actor_ends_ok_when_the_receiver_is_dropped_with_sdus_waiting_in_a_session` ->
//!   `received_sdus_left_in_the_queue_when_the_sender_is_dropped_are_counted` (36 SDUs wait).
//! * `link_ends_as_the_last_session_without_an_al_mac_after_each_kind_of_stop` ->
//!   `link_ends_as_the_last_session_without_an_al_mac_after_a_stop` (idle and with queued SDUs);
//!   the kind "end of file with a dropped receiver" has no loop meaning: the read source is
//!   served before the stop signal (0016 D3b), so the session ends by itself and nothing lingers.
//!
//! Ported with the receiver-drop replaced by the sender-drop (1):
//! `actor_ends_ok_when_the_receiver_is_dropped_with_sdus_waiting_after_a_disconnect` ->
//! `sender_dropped_with_sdus_waiting_after_a_disconnect_ends_ok`.
//!
//! Changed to the new rule (1): `sdus_arriving_during_a_fatal_registration_are_counted_and_the_channel_is_closed`
//! -> same name; no permit is held, an SDU sent while registering is counted and the channel is
//! closed once the loop ends.
//!
//! Retired (D8: "held send permit, close-and-length cutoff, linger timing via permits,
//! `abort()` skipping the final counts"; or a receiver-drop stop that has no loop meaning) (6):
//! * `sends_after_the_stop_began_fail_and_are_not_written`: after the stop signal (every sender
//!   dropped) no sender exists to send. The core's rule that a send after a stop is counted and
//!   not written is `the_role_gets_no_input_after_a_stop` in `transport_core/tests.rs`.
//! * `permit_held_across_the_stop_and_used_in_the_linger_is_counted_not_written`,
//!   `permit_held_across_a_stop_while_unregistered_is_waited_for_for_at_most_the_linger` and
//!   `permit_that_is_never_used_cannot_hold_a_stop_longer_than_the_linger`: there are no permits;
//!   replacements are the stop tests above (an unregistered stop ends at once, an in-session
//!   stop lingers) and the 1 s bound test.
//! * `dropping_only_the_receiver_writes_the_queued_sdus_and_lingers`: dropping `received` is no
//!   stop; the stop with queued SDUs is `dropping_the_sender_writes_the_queued_sdus_lingers_and_ends_ok`.
//! * `receiver_dropped_while_a_sender_keeps_sending_still_ends_the_actor`: a stop is the channel
//!   closing, which needs every sender dropped, so a producer that keeps sending asks for no stop.
//!   The starvation it guarded is `continuously_ready_sender_does_not_starve_reads` (loop on its
//!   own thread), the same with the loop on the test's one thread
//!   (`..._on_a_current_thread_runtime`), and the reverse.

// Each test file is its own crate, and this one uses only part of the shared support.
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

mod loop_support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use em_transport::{
    LinkState, MAX_FRAGMENT_PAYLOAD, MAX_SDU_PAYLOAD, OutboundSdu, Role, Sdu, TransportConfig,
    TransportCounters, TransportError,
};
use em_types::MacAddr;
use loop_support::{
    LoopClient, SduSender, start_loop, start_loop_on_its_own_thread, start_loop_with_config,
    start_loop_with_received_capacity,
};
use support::{
    FakeService, RECONNECT_DELAY, ServiceSession, TempDir, Watchdog, bounded, join_within,
    next_received, read_frame, read_one, sdu_frame,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::OwnedReadHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

const SERVICE_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x01];
const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x02];
const LOCAL_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x0a];
const SUCCESS: u8 = 0x01;
const UNFRAGMENTED: [u8; 3] = [0, 1, 0];
const LINGER: Duration = Duration::from_millis(100);
/// 0016 U4 (`policy::WRITE_STALL`), which an integration test cannot import.
const WRITE_STALL: Duration = Duration::from_secs(5);
/// 0016 U8 (`policy::STOP_WRITE_LIMIT`).
const STOP_WRITE_LIMIT: Duration = Duration::from_millis(900);
/// 1 ms short of the stop write limit, and 100 ms short of the write stall.
const JUST_BEFORE_THE_STOP_LIMIT: Duration = Duration::from_millis(899);
const JUST_BEFORE_THE_STALL: Duration = Duration::from_millis(4900);
/// The real-time tests wait at most this long for the loop to end.
const STOP_LIMIT: Duration = Duration::from_secs(5);
/// 0016 U1 (`policy::RX_QUEUE_SDUS`).
const RX_QUEUE_SDUS: usize = 1024;
/// 0016 U2, U6 (`policy::RX_SDU_CAP`).
const RX_SDU_CAP: usize = 1_048_576;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Waits for the loop's task to end and returns its result.
async fn join_loop(task: JoinHandle<Result<(), TransportError>>) -> Result<(), TransportError> {
    bounded("the loop to end", task)
        .await
        .expect("the loop task panicked")
}

/// Waits until `check` holds for the link state and returns that state.
async fn wait_for_link(
    link: &mut watch::Receiver<LinkState>,
    check: impl Fn(&LinkState) -> bool,
) -> LinkState {
    *bounded(
        "the link state to reach the expected value",
        link.wait_for(check),
    )
    .await
    .unwrap()
}

async fn wait_until_registered(link: &mut watch::Receiver<LinkState>) -> LinkState {
    wait_for_link(link, |state| state.al_mac.is_some()).await
}

async fn wait_until_disconnected(link: &mut watch::Receiver<LinkState>) {
    wait_for_link(link, |state| state.al_mac.is_none()).await;
}

/// Waits until `check` holds for the counters.
async fn wait_for_counters(client: &mut LoopClient, check: impl Fn(&TransportCounters) -> bool) {
    bounded(
        "the counters to reach the expected value",
        client.counters.wait_for(check),
    )
    .await
    .unwrap();
}

fn registered_link(session: u64, al_mac: [u8; 6]) -> LinkState {
    LinkState {
        session,
        al_mac: Some(MacAddr::new(al_mac)),
    }
}

/// Starts the loop and completes a successful registration.
async fn registered(role: Role) -> (FakeService, ServiceSession, LoopClient) {
    let service = FakeService::new();
    let client = start_loop(&service, role);
    complete_registration(service, client).await
}

/// Like `registered`, with a channel of `received_capacity` SDUs to the test.
async fn registered_with_received_capacity(
    received_capacity: usize,
) -> (FakeService, ServiceSession, LoopClient) {
    let service = FakeService::new();
    let client = start_loop_with_received_capacity(&service, Role::Agent, received_capacity);
    complete_registration(service, client).await
}

async fn complete_registration(
    service: FakeService,
    mut client: LoopClient,
) -> (FakeService, ServiceSession, LoopClient) {
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

async fn expect_received(client: &mut LoopClient, expected: Sdu) {
    assert_eq!(client.next_received().await, expected);
}

/// Lets the loop and the runtime's I/O driver run until nothing is ready.
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

/// Advances the paused clock, then lets the loop react.
async fn advance(by: Duration) {
    tokio::time::advance(by).await;
    settle().await;
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

/// Writes `joined` to the service side as one fragmented SDU: full fragments and a last one.
async fn write_fragmented_sdu(session: &mut ServiceSession, joined: &[u8]) {
    let chunks: Vec<&[u8]> = joined.chunks(MAX_FRAGMENT_PAYLOAD).collect();
    let last_index = chunks.len() - 1;
    for (index, chunk) in chunks.into_iter().enumerate() {
        let is_last = u8::from(index == last_index);
        let id = u8::try_from(index).unwrap();
        session
            .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, is_last, id], chunk))
            .await;
    }
}

/// A set of 16 full fragments and a last one of `last_len` octets, written to the service
/// side. Returns the joined payload that was sent. Octets count up, so a wrong order or a
/// wrong join shows in the delivered bytes.
async fn write_set_of_16_full_fragments_and_a_last_of(
    session: &mut ServiceSession,
    last_len: usize,
) -> Vec<u8> {
    let total = 16 * MAX_FRAGMENT_PAYLOAD + last_len;
    let joined: Vec<u8> = (0..total)
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect();
    write_fragmented_sdu(session, &joined).await;
    joined
}

/// Sends `count` one-octet SDUs without yielding, so they all sit in the channel.
fn send_burst(sdus: &SduSender, session: u64, count: u8) {
    for index in 0..count {
        sdus.try_send(out(session, LOCAL_MAC, PEER_MAC, &[index]))
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

/// Counts what is left in the received channel after the loop has ended.
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

/// Sends enough SDUs that the received channel (64) is full and SDUs wait in the received queue.
async fn fill_the_received_channel(session: &mut ServiceSession, client: &mut LoopClient) {
    const SENT: usize = 100;
    let mut bytes = Vec::new();
    for _ in 0..SENT {
        bytes.extend_from_slice(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"x"));
    }
    session.write_data(&bytes).await;
    wait_for_counters(client, |c| c.sdus_received == SENT as u64).await;
    // The loop forwards one SDU per pass and yields between passes, so the channel may still be
    // filling when the count is reached.
    bounded("the received channel to fill", async {
        while client.received.capacity() > 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn registration_request_bytes_for_both_roles() {
    for (role, service_type) in [(Role::Agent, 0x01), (Role::Controller, 0x02)] {
        let service = FakeService::new();
        let _client = start_loop(&service, role);
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
async fn rejected_registration_ends_the_loop_with_the_result() {
    // 0x00 unknown, 0x02 not supported, and a value the protocol does not define.
    for result in [0x00, 0x02, 0x7f] {
        let service = FakeService::new();
        let client = start_loop(&service, Role::Controller);
        let mut session = service.accept().await;
        session.respond(SERVICE_MAC, result).await;
        let error = join_loop(client.task).await.unwrap_err();
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
    // This client ends the loop with the result for both roles (0011 D3 step 3).
    let service = FakeService::new();
    let client = start_loop(&service, Role::Agent);
    let mut session = service.accept().await;
    session.respond(SERVICE_MAC, 0x02).await;

    let error = join_loop(client.task).await.unwrap_err();
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
    let mut client = start_loop(&service, Role::Agent);
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
    let mut client = start_loop_with_config(&config);

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
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn retries_while_the_socket_files_are_missing() {
    // intentional-difference:retry-connect (see `retries_while_the_socket_files_have_no_listener`)
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
    let mut client = start_loop_with_config(&config);

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
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn retries_after_end_of_file_before_the_response() {
    // intentional-difference:retry-connect
    // Upstream's controller exits when registration fails (`em_ctrl.cpp:1718-1738`) and the
    // agent's constructor throws without catching it (`al_service_access_point.cpp:36-55`).
    // This client registers again after the reconnect delay.
    let service = FakeService::new();
    let mut client = start_loop(&service, Role::Agent);

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
    let mut client = start_loop(&service, Role::Agent);

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

// ---------------------------------------------------------------------------
// Sending and receiving SDUs
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn one_sdu_each_way() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;

    client
        .sdus
        .send(out(1, SERVICE_MAC, PEER_MAC, b"to the service"))
        .await;
    assert_eq!(
        session.read_data_frame().await,
        sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"to the service")
    );

    session
        .write_data(&sdu_frame(
            SERVICE_MAC,
            PEER_MAC,
            UNFRAGMENTED,
            b"from the service",
        ))
        .await;
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"from the service")).await;
    let counters = *client.counters.borrow();
    assert_eq!(counters.sdus_sent, 1);
    assert_eq!(counters.sdus_received, 1);
}

#[tokio::test(start_paused = true)]
async fn sent_sdu_bytes_follow_the_sdu_codec() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    let remote = [0x02, 0, 0, 0, 0, 0x0b];
    client
        .sdus
        .send(out(1, LOCAL_MAC, remote, &[0x80, 0x01, 0xff]))
        .await;

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
        .await;
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

    client
        .sdus
        .send(out(
            1,
            LOCAL_MAC,
            PEER_MAC,
            &vec![0xaa; MAX_FRAGMENT_PAYLOAD],
        ))
        .await;
    let whole = session.read_data_frame().await;
    assert_eq!(whole.len(), 65_536);
    assert_eq!(whole[16..19], [0, 1, 0]);

    client
        .sdus
        .send(out(
            1,
            LOCAL_MAC,
            PEER_MAC,
            &vec![0xbb; MAX_FRAGMENT_PAYLOAD + 1],
        ))
        .await;
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
async fn continuation_from_another_peer_is_dropped_and_the_session_goes_on() {
    // intentional-difference:continuation-mac
    // Upstream keeps the first fragment's addresses and never compares later ones
    // (`al_service_access_point.cpp:267-278`), and it throws on B0 as out of order
    // (`:267-268`). Without the check this client would join B0 and A1, since B0 starts
    // a new set. Here A1 is dropped with B0's set (3 fragments counted: A0 abandoned,
    // B0 and A1 dropped).
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    let other_peer = [0x02, 0, 0, 0, 0, 0x03];

    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 0], b"a0"))
        .await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, other_peer, [1, 0, 0], b"b0"))
        .await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 1, 1], b"a1"))
        .await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"whole"))
        .await;
    // nothing was delivered before the unfragmented frame
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"whole")).await;

    assert_eq!(client.counters.borrow().registrations, 1);
    assert_eq!(client.counters.borrow().dropped_receive_fragments, 3);
}

#[tokio::test(start_paused = true)]
async fn fragment_set_over_1_mib_is_refused_counted_once_and_the_session_goes_on() {
    // intentional-difference:rx-sdu-cap-1mib
    // Upstream has no cap below the framing limit (`al_service_access_point.cpp:224-300`).
    // 16 fragments of 65 517 octets fit 1 MiB, the 17th does not. The set is counted once in
    // `dropped_receive_too_large`; its remaining fragments are skipped without counting.
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    let payload = vec![0x5a; 65_517];

    for id in 0..19_u8 {
        let is_last = u8::from(id == 18);
        session
            .write_data(&sdu_frame(
                SERVICE_MAC,
                PEER_MAC,
                [1, is_last, id],
                &payload,
            ))
            .await;
    }
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"whole"))
        .await;
    // nothing of the refused set was delivered before this one
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"whole")).await;

    let counters = *client.counters.borrow();
    assert_eq!(counters.registrations, 1);
    assert_eq!(counters.dropped_receive_too_large, 1);
    assert_eq!(counters.dropped_receive_fragments, 0);
    assert_eq!(counters.sdus_received, 1);
}

#[tokio::test(start_paused = true)]
async fn fragment_set_of_exactly_1_mib_is_delivered() {
    // intentional-difference:rx-sdu-cap-1mib
    // The cap is 1 MiB of joined payload: 16 fragments of 65 517 octets and a last one of 304
    // make exactly 1 048 576, which passes.
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    let last_len = RX_SDU_CAP - 16 * MAX_FRAGMENT_PAYLOAD;
    assert_eq!(last_len, 304);

    let joined = write_set_of_16_full_fragments_and_a_last_of(&mut session, last_len).await;
    assert_eq!(joined.len(), RX_SDU_CAP);
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, &joined)).await;

    let counters = *client.counters.borrow();
    assert_eq!(counters.registrations, 1);
    assert_eq!(counters.sdus_received, 1);
    assert_eq!(counters.dropped_receive_too_large, 0);
    assert_eq!(counters.dropped_receive_fragments, 0);
}

#[tokio::test(start_paused = true)]
async fn fragment_set_of_1_mib_plus_one_octet_is_refused_and_the_session_goes_on() {
    // intentional-difference:rx-sdu-cap-1mib
    // The same set with a last fragment of 305 octets is 1 048 577: refused once, none of it
    // delivered, and the next whole frame still arrives.
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    let last_len = RX_SDU_CAP - 16 * MAX_FRAGMENT_PAYLOAD + 1;
    assert_eq!(last_len, 305);

    write_set_of_16_full_fragments_and_a_last_of(&mut session, last_len).await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b"whole"))
        .await;
    // nothing of the refused set was delivered before this one
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"whole")).await;

    let counters = *client.counters.borrow();
    assert_eq!(counters.registrations, 1);
    assert_eq!(counters.sdus_received, 1);
    assert_eq!(counters.dropped_receive_too_large, 1);
    assert_eq!(counters.dropped_receive_fragments, 0);
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
async fn frame_with_an_empty_payload_is_delivered() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, b""))
        .await;
    expect_received(&mut client, sdu(SERVICE_MAC, PEER_MAC, b"")).await;
}

#[tokio::test(start_paused = true)]
async fn sdu_over_256_fragments_is_dropped_and_the_session_goes_on() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;

    let too_big = vec![0u8; MAX_SDU_PAYLOAD + 1];
    client
        .sdus
        .send(out(1, LOCAL_MAC, PEER_MAC, &too_big))
        .await;
    wait_for_counters(&mut client, |c| c.dropped_send_failed >= 1).await;

    // The session is still up: the next SDU goes out.
    client.sdus.send(out(1, LOCAL_MAC, PEER_MAC, b"next")).await;
    let frame = session.read_data_frame().await;
    assert_eq!(&frame[19..], b"next");

    wait_for_counters(&mut client, |c| c.sdus_sent >= 1).await;
    let counters = *client.counters.borrow();
    assert_eq!(counters.dropped_send_failed, 1);
    assert_eq!(counters.sdus_sent, 1);
    assert_eq!(counters.registrations, 1);
}

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
async fn control_socket_stays_open_during_a_session() {
    let (_service, mut session, _client) = registered(Role::Agent).await;
    let read = tokio::time::timeout(Duration::from_secs(1), read_one(&mut session.control)).await;
    assert!(read.is_err(), "the client closed the control socket");
}

#[tokio::test(start_paused = true)]
async fn failed_write_ends_the_session_and_counts_the_unstarted_sdu_as_disconnected() {
    // intentional-difference:tx-failed-write-count
    // The counting rule of 0016 D3b: only a partly written SDU is a failed one. The actor
    // counted the SDU of the failed write as `dropped_send_failed`; this write fails before
    // any octet of the SDU is written, so it is `dropped_send_disconnected` (the core test
    // `a_write_error_before_any_octet_counts_the_sdu_as_disconnected` pins the same rule).
    let (_service, session, mut client) = registered(Role::Agent).await;
    let ServiceSession { control, data, .. } = session;
    // The service stops reading its data socket but keeps it open, so the client's reads
    // see no end of file and only its writes fail.
    let data = data.into_std().unwrap();
    data.shutdown(std::net::Shutdown::Read).unwrap();

    client.sdus.send(out(1, LOCAL_MAC, PEER_MAC, b"lost")).await;

    wait_until_disconnected(&mut client.link).await;
    let counters = *client.counters.borrow();
    assert_eq!(counters.dropped_send_failed, 0);
    assert_eq!(counters.dropped_send_disconnected, 1);
    assert_eq!(counters.sdus_sent, 0);
    drop((control, data));
}

// ---------------------------------------------------------------------------
// Reconnecting
// ---------------------------------------------------------------------------

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

    client
        .sdus
        .send(out(1, LOCAL_MAC, PEER_MAC, b"stale"))
        .await;
    wait_for_counters(&mut client, |c| c.dropped_send_disconnected == 1).await;

    let mut again = service.accept().await;
    again.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut client.link).await;

    // The first frame the service reads is the one sent after registering.
    client
        .sdus
        .send(out(2, LOCAL_MAC, PEER_MAC, b"fresh"))
        .await;
    let frame = again.read_data_frame().await;
    assert_eq!(&frame[19..], b"fresh");
}

#[tokio::test(start_paused = true)]
async fn sdus_left_over_from_the_old_session_are_not_written_into_the_new_one() {
    // A6 (S0-01): the registration does not drain the channel. Sent together with the
    // response, the SDUs are counted either while unregistered or, as stale, in the new
    // session. They are never written.
    let (service, session, mut client) = registered(Role::Agent).await;
    drop(session);
    wait_until_disconnected(&mut client.link).await;

    let mut again = service.accept().await;
    // No await between the sends and the response, so the loop sees both together.
    send_burst(&client.sdus, 1, 5);
    let mut response = vec![0, 0, 0, 7];
    response.extend_from_slice(&SERVICE_MAC);
    response.push(SUCCESS);
    again.control.try_write(&response).unwrap();

    wait_until_registered(&mut client.link).await;
    wait_for_counters(&mut client, |c| c.dropped_send_disconnected == 5).await;

    client
        .sdus
        .send(out(2, LOCAL_MAC, PEER_MAC, b"fresh"))
        .await;
    assert_eq!(&again.read_data_frame().await[19..], b"fresh");
    assert_eq!(client.counters.borrow().sdus_sent, 1);
}

#[tokio::test(start_paused = true)]
async fn link_already_shows_the_session_of_the_first_received_sdu() {
    // A5 test 5: the role reads the SDU first and only then looks at the link.
    let service = FakeService::new();
    let mut client = start_loop(&service, Role::Agent);
    for session_number in 1..=2u8 {
        let al_mac = [0x02, 0, 0, 0, 0, 0x20 + session_number];
        let mut session = service.accept().await;
        session.respond(al_mac, SUCCESS).await;
        session
            .write_data(&sdu_frame(al_mac, PEER_MAC, UNFRAGMENTED, b"first"))
            .await;

        let first = client.next_received().await;
        assert_eq!(first.local, MacAddr::new(al_mac));
        assert_eq!(
            *client.link.borrow(),
            registered_link(u64::from(session_number), al_mac)
        );
        drop(session);
        wait_until_disconnected(&mut client.link).await;
    }
}

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
    let mut client = start_loop_with_config(&config);

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
    wait_until_registered(&mut client.link).await;
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
    let mut client = start_loop_with_config(&config);

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
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn socket_path_the_system_rejects_ends_the_loop_with_an_io_error() {
    // A path above the length of `sun_path` is invalid input, not a service restart.
    let dir = TempDir::new();
    let config = TransportConfig::new(
        dir.path().join("x".repeat(200)),
        dir.path().join("data"),
        Role::Agent,
    );
    let client = start_loop_with_config(&config);

    let error = join_loop(client.task).await.unwrap_err();
    let TransportError::Io { operation, source } = error else {
        panic!("expected an I/O error, got {error:?}");
    };
    assert_eq!(operation, "connect control socket");
    assert_eq!(source.kind(), std::io::ErrorKind::InvalidInput);
}

#[tokio::test(start_paused = true)]
async fn connection_reset_before_the_response_is_retried() {
    let service = FakeService::new();
    let mut client = start_loop(&service, Role::Agent);

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
    let mut client = start_loop(&service, Role::Agent);

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
    let mut client = start_loop(&service, Role::Agent);

    let mut first = service.accept().await;
    first.write_control(&[0, 0, 0, 6, 2, 0, 0, 0, 0, 1]).await;

    let mut second = service.accept().await;
    second.respond(SERVICE_MAC, SUCCESS).await;
    wait_until_registered(&mut client.link).await;
}

#[tokio::test(start_paused = true)]
async fn rejection_wins_over_an_all_zero_al_mac() {
    let service = FakeService::new();
    let client = start_loop(&service, Role::Controller);
    let mut session = service.accept().await;
    session.respond([0; 6], 0x02).await;

    let error = join_loop(client.task).await.unwrap_err();
    assert!(
        matches!(error, TransportError::RegistrationRejected { result: 0x02 }),
        "unexpected error {error:?}"
    );
}

// ---------------------------------------------------------------------------
// The received queue (rx-queue-1024-4mib)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn received_queue_keeps_the_newest_1024_sdus_and_counts_the_rest() {
    // intentional-difference:rx-queue-1024-4mib
    // The actor kept 256 SDUs behind a channel of 64 and counted the overflow. The queue now
    // keeps 1024. A channel of one SDU, filled first, makes the split exact: the loop forwards
    // one SDU per pass, so with a larger channel it would depend on how many passes the reads
    // left between them.
    const FLOOD: usize = 1500;
    let (_service, mut session, mut client) = registered_with_received_capacity(1).await;

    session.write_data(&numbered_sdu_frames(0, 1)).await;
    wait_for_counters(&mut client, |c| c.sdus_received == 1).await;
    // The role is not reading: the SDU sits in the channel and everything else in the queue.
    session.write_data(&numbered_sdu_frames(1, FLOOD)).await;
    wait_for_counters(&mut client, |c| c.sdus_received == 1 + FLOOD as u64).await;

    let counters = *client.counters.borrow();
    assert_eq!(
        counters.dropped_receive_overflow,
        (FLOOD - RX_QUEUE_SDUS) as u64
    );

    let tags: Vec<u16> = client.drain_received().await.iter().map(tag_of).collect();
    let newest = (FLOOD - RX_QUEUE_SDUS + 1..=FLOOD).map(|tag| u16::try_from(tag).unwrap());
    let expected: Vec<u16> = std::iter::once(0).chain(newest).collect();
    assert_eq!(tags, expected);
}

#[tokio::test(start_paused = true)]
async fn received_queue_keeps_at_most_4_mib_of_cmdu() {
    // intentional-difference:rx-queue-1024-4mib (the byte limit)
    // Six SDUs of 1 000 000 octets (under the 1 MiB cap) behind one small SDU that fills the
    // channel: the queue holds 4 of them (4 000 000 <= 4 MiB < 5 000 000), the 2 oldest go.
    const BIG: usize = 1_000_000;
    let (_service, mut session, mut client) = registered_with_received_capacity(1).await;

    session.write_data(&numbered_sdu_frames(0, 1)).await;
    wait_for_counters(&mut client, |c| c.sdus_received == 1).await;
    for index in 0..6u8 {
        write_fragmented_sdu(&mut session, &vec![index; BIG]).await;
    }
    wait_for_counters(&mut client, |c| c.sdus_received == 7).await;
    assert_eq!(client.counters.borrow().dropped_receive_overflow, 2);

    let received = client.drain_received().await;
    let sizes: Vec<usize> = received.iter().map(|sdu| sdu.cmdu.len()).collect();
    assert_eq!(sizes, [2, BIG, BIG, BIG, BIG]);
    let firsts: Vec<u8> = received[1..].iter().map(|sdu| sdu.cmdu[0]).collect();
    assert_eq!(firsts, [2, 3, 4, 5], "the newest four are kept");
}

#[tokio::test(start_paused = true)]
async fn many_reconnects_with_an_unread_role_keep_the_newest_and_the_last_link() {
    // intentional-difference:rx-queue-1024-4mib
    // More than one SDU plus 1024 over several sessions, a different AL MAC each: the queue
    // outlives the sessions and keeps the newest 1024. A channel of one SDU, filled by the
    // first SDU, makes the split exact (see `received_queue_keeps_the_newest_1024_sdus...`).
    const SESSIONS: usize = 4;
    const PER_SESSION: usize = 400;
    const TOTAL: usize = SESSIONS * PER_SESSION;
    let service = FakeService::new();
    let mut client = start_loop_with_received_capacity(&service, Role::Agent, 1);

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

    // One SDU sits in the channel and the queue keeps the newest 1024 of the other 1599.
    let overflow = (TOTAL - 1 - RX_QUEUE_SDUS) as u64;
    assert_eq!(client.counters.borrow().dropped_receive_overflow, overflow);
    assert_eq!(
        *client.link.borrow(),
        registered_link(SESSIONS as u64, last_mac)
    );

    let tags: Vec<u16> = client.drain_received().await.iter().map(tag_of).collect();
    let newest = (TOTAL - RX_QUEUE_SDUS..TOTAL).map(|tag| u16::try_from(tag).unwrap());
    let expected: Vec<u16> = std::iter::once(0).chain(newest).collect();
    assert_eq!(tags, expected);
    drop(last_session);
}

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn dropping_the_sender_writes_the_queued_sdus_lingers_and_ends_ok() {
    let (_service, mut session, client) = registered(Role::Agent).await;
    let LoopClient { sdus, task, .. } = client;

    for tag in 0..2 {
        sdus.send(out(1, SERVICE_MAC, PEER_MAC, &[tag])).await;
    }
    drop(sdus);
    let dropped_at = tokio::time::Instant::now();

    // Both writes leave, then the sockets stay open for the linger.
    expect_burst(&mut session, 2).await;
    assert_eq!(read_one(&mut session.data).await, 0);
    assert!(
        dropped_at.elapsed() >= LINGER,
        "the data socket closed {:?} after the stop, before the linger of {LINGER:?}",
        dropped_at.elapsed()
    );
    assert_eq!(read_one(&mut session.control).await, 0);

    let result = join_loop(task).await;
    assert!(result.is_ok(), "unexpected result {result:?}");
}

#[tokio::test(start_paused = true)]
async fn loop_ends_when_the_sender_is_dropped_while_not_registered() {
    let service = FakeService::new();
    let client = start_loop(&service, Role::Agent);
    // Accepted but never answered: the client waits for the response.
    let _session = service.accept().await;
    let dropped_at = tokio::time::Instant::now();
    drop(client.sdus);
    assert!(join_loop(client.task).await.is_ok());
    assert!(dropped_at.elapsed() < LINGER, "unregistered: no linger");
}

#[tokio::test(start_paused = true)]
async fn loop_stays_for_the_linger_after_the_sender_is_dropped_and_then_ends() {
    let (_service, _session, client) = registered(Role::Agent).await;
    let LoopClient { sdus, task, .. } = client;

    drop(sdus);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!task.is_finished(), "ended before the linger of 100 ms");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(task.is_finished(), "still running 150 ms after the drop");
    assert!(join_loop(task).await.is_ok());
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

#[tokio::test(start_paused = true)]
async fn dropping_the_sender_after_a_burst_of_64_still_writes_every_queued_sdu_and_lingers() {
    // A5 test 1: 64 SDUs, then the sender dropped in the same poll. The channel is drained
    // before it reports that it is closed, so every SDU reaches the core.
    let (_service, mut session, client) = registered(Role::Agent).await;
    let LoopClient {
        sdus,
        counters,
        task,
        ..
    } = client;

    send_burst(&sdus, 1, 64);
    drop(sdus);
    let dropped_at = tokio::time::Instant::now();

    expect_burst(&mut session, 64).await;
    expect_linger_then_end_of_file(&mut session, dropped_at).await;
    assert!(join_loop(task).await.is_ok());
    assert_eq!(counters.borrow().sdus_sent, 64);
}

#[tokio::test(start_paused = true)]
async fn sdus_arriving_during_a_fatal_registration_are_counted_and_the_channel_is_closed() {
    // A6 (S0-03). No permit is held now: an SDU sent while registering is counted as not
    // written, and once the loop has ended the channel is closed.
    let service = FakeService::new();
    let mut client = start_loop(&service, Role::Controller);
    let mut session = service.accept().await;

    client
        .sdus
        .send(out(0, LOCAL_MAC, PEER_MAC, b"early"))
        .await;
    wait_for_counters(&mut client, |c| c.dropped_send_disconnected == 1).await;
    session.respond(SERVICE_MAC, 0x02).await;

    let error = join_loop(client.task).await.unwrap_err();
    assert!(matches!(
        error,
        TransportError::RegistrationRejected { result: 0x02 }
    ));
    assert_eq!(client.counters.borrow().dropped_send_disconnected, 1);
    assert!(
        client.sdus.is_closed(),
        "the channel is still open after the error"
    );
}

#[tokio::test(start_paused = true)]
async fn link_ends_as_the_last_session_without_an_al_mac_after_a_stop() {
    // A6 (S0-06): `None` is published after the linger, never before it. Idle, and with SDUs
    // queued for writing.
    for queued in [0, 5] {
        let (_service, _session, client) = registered(Role::Agent).await;
        let LoopClient {
            sdus, link, task, ..
        } = client;
        send_burst(&sdus, 1, queued);
        drop(sdus);

        advance(LINGER / 2).await;
        assert_eq!(
            *link.borrow(),
            registered_link(1, SERVICE_MAC),
            "{queued} queued: link changed during the linger"
        );
        assert!(join_loop(task).await.is_ok());
        let last = LinkState {
            session: 1,
            al_mac: None,
        };
        assert_eq!(*link.borrow(), last, "{queued} queued");
    }
}

#[tokio::test(start_paused = true)]
async fn unfinished_fragment_set_is_counted_when_the_session_ends_with_a_stop() {
    // A6 (S0-05)
    let (_service, mut session, client) = registered(Role::Agent).await;
    let LoopClient {
        sdus,
        counters,
        task,
        ..
    } = client;
    session
        .write_data(&sdu_frame(SERVICE_MAC, PEER_MAC, [1, 0, 0], b"ab"))
        .await;
    advance(Duration::from_millis(10)).await;

    drop(sdus);
    assert!(join_loop(task).await.is_ok());
    assert_eq!(counters.borrow().dropped_receive_fragments, 1);
}

#[tokio::test(start_paused = true)]
async fn received_sdus_left_in_the_queue_when_the_sender_is_dropped_are_counted() {
    // A6: sdus_received = moved into the channel + overflow + stopped. With 100 SDUs and a
    // channel of 64, 36 wait in the queue when the loop stops.
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    fill_the_received_channel(&mut session, &mut client).await;
    let LoopClient {
        sdus,
        mut received,
        counters,
        task,
        ..
    } = client;

    drop(sdus);
    assert!(join_loop(task).await.is_ok());
    let in_channel = count_until_closed(&mut received).await;
    let counters = *counters.borrow();
    assert_eq!(counters.dropped_receive_stopped, 36);
    assert_eq!(
        counters.sdus_received,
        in_channel + counters.dropped_receive_overflow + counters.dropped_receive_stopped
    );
}

#[tokio::test(start_paused = true)]
async fn received_sdus_left_in_the_queue_at_a_fatal_registration_error_are_counted() {
    let (service, mut session, mut client) = registered(Role::Agent).await;
    fill_the_received_channel(&mut session, &mut client).await;
    let LoopClient {
        sdus: _sdus,
        mut received,
        mut link,
        counters,
        task,
    } = client;

    // The session ends with SDUs still queued, and the next registration is rejected.
    drop(session);
    wait_until_disconnected(&mut link).await;
    let mut again = service.accept().await;
    again.respond(SERVICE_MAC, 0x02).await;
    assert!(join_loop(task).await.is_err());

    let in_channel = count_until_closed(&mut received).await;
    let counters = *counters.borrow();
    assert!(counters.dropped_receive_stopped > 0);
    assert_eq!(
        counters.sdus_received,
        in_channel + counters.dropped_receive_overflow + counters.dropped_receive_stopped
    );
}

#[tokio::test(start_paused = true)]
async fn dropping_the_link_and_counters_watches_stops_nothing() {
    // A5 test 3
    let (_service, mut session, client) = registered(Role::Agent).await;
    let LoopClient {
        sdus,
        mut received,
        link,
        counters,
        task,
    } = client;
    drop((link, counters));

    sdus.send(out(1, LOCAL_MAC, PEER_MAC, b"out")).await;
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
    assert!(!task.is_finished());
}

#[tokio::test(start_paused = true)]
async fn sender_dropped_while_unregistered_ends_at_once_and_counts_the_queued_sdus() {
    let service = FakeService::new();
    let LoopClient {
        sdus,
        counters,
        task,
        ..
    } = start_loop(&service, Role::Agent);
    // Accepted but never answered: the loop waits for the response.
    let _session = service.accept().await;

    send_burst(&sdus, 1, 3);
    drop(sdus);
    let dropped_at = tokio::time::Instant::now();

    assert!(join_loop(task).await.is_ok());
    assert!(dropped_at.elapsed() < LINGER, "unregistered: no linger");
    assert_eq!(counters.borrow().dropped_send_disconnected, 3);
}

#[tokio::test(start_paused = true)]
async fn sender_dropped_with_sdus_waiting_after_a_disconnect_ends_ok() {
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    fill_the_received_channel(&mut session, &mut client).await;
    let LoopClient { sdus, task, .. } = client;

    // The loop ends the session and waits for the reconnect delay with SDUs still queued.
    drop(session);
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(sdus);
    assert!(join_loop(task).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn stop_whose_writes_cannot_finish_ends_within_900_ms_plus_the_linger() {
    // U8: the service keeps its data socket open and never reads, so the socket fills and the
    // queued SDUs cannot be written. The stop writes for 900 ms, counts what is left, lingers
    // 100 ms and ends: 1 s in all, not the 5 s of the write stall.
    const SENT: usize = 50;
    let (_service, session, client) = registered(Role::Agent).await;
    let LoopClient {
        sdus,
        link,
        counters,
        task,
        ..
    } = client;
    for _ in 0..SENT {
        sdus.send(out(1, LOCAL_MAC, PEER_MAC, &vec![0x5a; 65_000]))
            .await;
    }

    let stopped_at = tokio::time::Instant::now();
    drop(sdus);
    tokio::time::sleep(JUST_BEFORE_THE_STOP_LIMIT).await;
    assert!(!task.is_finished(), "ended inside the 900 ms of writing");
    assert_eq!(*link.borrow(), registered_link(1, SERVICE_MAC));

    assert!(join_loop(task).await.is_ok());
    let elapsed = stopped_at.elapsed();
    assert_eq!(elapsed, STOP_WRITE_LIMIT + LINGER);
    assert_eq!(link.borrow().al_mac, None);

    // The one counting rule: a partly written SDU failed, the rest were not started.
    let counters = *counters.borrow();
    assert!(counters.dropped_send_failed <= 1);
    assert!(counters.sdus_sent < SENT as u64, "the socket never filled");
    assert_eq!(
        counters.sdus_sent + counters.dropped_send_failed + counters.dropped_send_disconnected,
        SENT as u64
    );
    drop(session);
}

// ---------------------------------------------------------------------------
// The outbound side: the write stall and the buffer
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn write_stall_of_5_seconds_ends_the_session_counts_it_and_reconnects() {
    // intentional-difference:tx-write-stall-5s
    // The actor had no time limit on a write (0011 A7). Here the service holds its data socket
    // open and never reads. The client fills the socket and the rest waits in the outbound
    // buffer (50 SDUs of 65 019 octets, below the 4 MiB bound). 5 s after the last progress the
    // session ends, by the same counting rule as every session end, and the client registers
    // again after the reconnect delay.
    const SENT: usize = 50;
    let (service, session, mut client) = registered(Role::Agent).await;
    for _ in 0..SENT {
        client
            .sdus
            .send(out(1, LOCAL_MAC, PEER_MAC, &vec![0x5a; 65_000]))
            .await;
    }
    settle().await;
    let last_progress = tokio::time::Instant::now();

    tokio::time::sleep(JUST_BEFORE_THE_STALL).await;
    assert_eq!(
        *client.link.borrow(),
        registered_link(1, SERVICE_MAC),
        "the session ended before the stall of {WRITE_STALL:?}"
    );
    wait_until_disconnected(&mut client.link).await;
    assert_eq!(last_progress.elapsed(), WRITE_STALL);

    let counters = *client.counters.borrow();
    assert!(counters.dropped_send_failed <= 1);
    assert!(counters.sdus_sent < SENT as u64, "the socket never filled");
    assert_eq!(
        counters.sdus_sent + counters.dropped_send_failed + counters.dropped_send_disconnected,
        SENT as u64
    );

    let mut again = service.accept().await;
    again.respond(SERVICE_MAC, SUCCESS).await;
    assert_eq!(
        wait_until_registered(&mut client.link).await,
        registered_link(2, SERVICE_MAC)
    );
    assert_eq!(client.counters.borrow().registrations, 2);
    drop(session);
}

#[tokio::test(start_paused = true)]
async fn outbound_buffer_of_4_mib_drops_the_oldest_unstarted_sdus_and_counts_them() {
    // intentional-difference:tx-buffer
    // The actor kept 64 SDUs behind a channel of 64 and dropped the oldest. The per-session
    // buffer now holds 4 MiB. The service reads nothing at first, so the first SDU (1 MB, more
    // than the socket takes) is started and stays; the next five arrive behind it. Four SDUs
    // fit (4 000 000 octets plus headers), so the 5th and 6th each drop the oldest that has
    // not started: SDU 2, then SDU 3. The service then reads what is left.
    const BIG: usize = 1_000_000;
    let (_service, mut session, mut client) = registered(Role::Agent).await;
    for index in 1..=6u8 {
        client
            .sdus
            .send(out(1, LOCAL_MAC, PEER_MAC, &vec![index; BIG]))
            .await;
    }
    wait_for_counters(&mut client, |c| c.dropped_send_overflow == 2).await;

    // 4 SDUs of 16 frames each: the first frame of an SDU has fragment id 0.
    let mut firsts = Vec::new();
    for _ in 0..4 * 16 {
        let frame = session.read_data_frame().await;
        if frame[18] == 0 {
            firsts.push(frame[19]);
        }
    }
    assert_eq!(firsts, [1, 4, 5, 6]);
    wait_for_counters(&mut client, |c| c.sdus_sent == 4).await;
    let counters = *client.counters.borrow();
    assert_eq!(counters.dropped_send_overflow, 2);
    assert_eq!(counters.dropped_send_disconnected, 0);
    assert_eq!(counters.dropped_send_failed, 0);
}

// ---------------------------------------------------------------------------
// Reads and sends between passes (rx-read-between-passes)
// ---------------------------------------------------------------------------

/// Tells the producer thread of `continuously_ready_sender_does_not_starve_reads` to stop.
static STOP_PRODUCER: AtomicBool = AtomicBool::new(false);
/// The same, for the test on a current-thread runtime.
static STOP_PRODUCER_ON_ONE_THREAD: AtomicBool = AtomicBool::new(false);

/// A thread that keeps the loop's application channel full: it refills the channel the moment a
/// slot frees, so the loop always has an SDU ready. It ends when `stop` is set or when the loop
/// has ended. Dropping it stops and joins the thread, so a failed assertion in the test reports
/// instead of leaving a thread that spins on.
struct Producer {
    stop: &'static AtomicBool,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Producer {
    fn start(stop: &'static AtomicBool, sdus: SduSender) -> Self {
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let sent = sdus.try_send(out(1, LOCAL_MAC, PEER_MAC, b"x"));
                if matches!(sent, Err(TrySendError::Closed(_))) {
                    break;
                }
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // A panic of the producer must not turn a failing test into an abort.
            let _ = thread.join();
        }
    }
}

/// A task of the test's runtime that tops up the loop's application channel each time the loop
/// yields, so that the loop finds an input ready after every pass and never calls `Io::wait`.
/// A thread cannot promise that: one that is slow to start or to be scheduled lets the channel run
/// empty once, and that one `wait` advances what the test wants a busy channel to starve.
/// Dropping it ends the task, which drops the sender.
struct BusyChannel(tokio::task::JoinHandle<()>);

impl BusyChannel {
    fn start(sdus: SduSender) -> Self {
        // Fill it before the loop's first pass, which comes when the test first awaits.
        while sdus.try_wake().is_ok() {}
        Self(tokio::spawn(async move {
            while !sdus.is_closed() {
                while sdus.try_wake().is_ok() {}
                tokio::task::yield_now().await;
            }
        }))
    }
}

impl Drop for BusyChannel {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Reads and discards what the client writes, so that its writes never stall.
fn spawn_drain(mut service_read: OwnedReadHalf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut buffer = [0u8; 4096];
        while matches!(service_read.read(&mut buffer).await, Ok(count) if count > 0) {}
    })
}

/// The service writes 20 frames one by one while a producer thread keeps the loop's SDU
/// channel full; each frame must reach `received` within 5 s. Real time.
async fn run_sender_does_not_starve_reads(
    stop_producer: &'static AtomicBool,
    start: fn(&FakeService, Role) -> LoopClient,
) {
    let service = FakeService::new();
    let client = start(&service, Role::Agent);
    let (_service, session, client) = complete_registration(service, client).await;
    let ServiceSession { control, data, .. } = session;
    let (service_read, mut service_write) = data.into_split();
    let drain = spawn_drain(service_read);

    let LoopClient {
        sdus,
        mut received,
        task,
        ..
    } = client;
    let producer = Producer::start(stop_producer, sdus);

    // Real time. With the loop yielding between passes the 20 frames take well under 0.5 s; a
    // loop that starves the runtime took 15 s to more than 60 s.
    let started = std::time::Instant::now();
    for index in 0..20u8 {
        let frame = sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, &[index]);
        service_write.write_all(&frame).await.unwrap();
        let received_sdu = tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap_or_else(|_| panic!("frame {index} was not delivered under sustained sends"));
        assert_eq!(received_sdu, Some(sdu(SERVICE_MAC, PEER_MAC, &[index])));
    }
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "20 frames took {:?} under sustained sends",
        started.elapsed()
    );

    drop(producer);
    assert!(join_within(STOP_LIMIT, task).await.is_ok());
    drain.abort();
    drop((control, service_write));
}

/// The loop on a thread of its own, as a daemon that runs it beside other tasks would: a
/// continuously ready sender does not keep reads waiting.
#[tokio::test]
async fn continuously_ready_sender_does_not_starve_reads() {
    // intentional-difference:rx-read-between-passes
    // The actor read in parallel with the role. The loop reads between passes, 64 frames per
    // pass, and serves the application source once per pass, so neither can starve the other.
    run_sender_does_not_starve_reads(&STOP_PRODUCER, start_loop_on_its_own_thread).await;
}

/// The same with the loop in a task of the test's current-thread runtime. While a source stays
/// ready the driver does not wait; it calls `Io::yield_now`, and `TransportIo` yields to tokio
/// there. Without that yield a sender that is always ready kept the thread: the I/O driver was not
/// turned, `try_read` saw no readiness, and the service's frames waited (2 of 3 runs failed after
/// 15 s and 21 s; the passing run took 41 s).
#[tokio::test]
async fn continuously_ready_sender_does_not_starve_reads_on_a_current_thread_runtime() {
    run_sender_does_not_starve_reads(&STOP_PRODUCER_ON_ONE_THREAD, start_loop).await;
}

/// The registration attempt is a future the loop polls. While the application channel is busy
/// from before the first connect the driver calls `Io::yield_now` and never `wait`, so
/// `yield_now` has to advance the attempt too. Real time.
#[tokio::test]
async fn registration_completes_while_the_application_channel_is_always_busy() {
    let service = FakeService::new();
    let LoopClient {
        sdus,
        received,
        mut link,
        counters,
        task,
    } = start_loop(&service, Role::Agent);
    // The loop is a task of this runtime and has not run yet: the channel is busy before the
    // first connect.
    let busy_channel = BusyChannel::start(sdus);

    let started = std::time::Instant::now();
    let registration = async {
        let mut session = service.accept().await;
        session.respond(SERVICE_MAC, SUCCESS).await;
        wait_until_registered(&mut link).await;
        session
    };
    let session = tokio::time::timeout(Duration::from_secs(3), registration)
        .await
        .expect("the registration did not complete within 3 s of a busy application channel");
    assert_eq!(*link.borrow(), registered_link(1, SERVICE_MAC));
    assert_eq!(counters.borrow().registrations, 1);
    assert!(started.elapsed() < Duration::from_secs(3));

    let ServiceSession { control, data, .. } = session;
    let (service_read, service_write) = data.into_split();
    let drain = spawn_drain(service_read);
    drop(busy_channel);
    assert!(join_within(STOP_LIMIT, task).await.is_ok());
    drain.abort();
    drop((control, service_write, received));
}

/// A write that got `WouldBlock` waits for the socket to become writable. That wait is a branch
/// of `Io::yield_now` as well, or a busy application channel keeps the queued SDUs waiting until
/// the 5 s write stall ends the session. Real time.
#[tokio::test]
async fn blocked_write_resumes_while_the_application_channel_is_always_busy() {
    const SENT: usize = 50;
    let (service, session, client) = registered(Role::Agent).await;
    let LoopClient {
        sdus,
        received,
        link,
        counters,
        task,
    } = client;
    // The service does not read yet: the socket fills, the rest waits in the outbound buffer
    // (50 SDUs of 65 019 octets, below the 4 MiB bound). Each SDU starts with its number.
    for index in 0..SENT {
        let number = u8::try_from(index).unwrap();
        sdus.send(out(1, LOCAL_MAC, PEER_MAC, &vec![number; 65_000]))
            .await;
    }
    settle().await;
    assert!(
        counters.borrow().sdus_sent < SENT as u64,
        "the socket never filled"
    );

    let busy_channel = BusyChannel::start(sdus);
    let ServiceSession {
        control, mut data, ..
    } = session;

    let started = std::time::Instant::now();
    let reading = async {
        for index in 0..SENT {
            let frame = read_frame(&mut data).await;
            assert_eq!(frame[19], u8::try_from(index).unwrap(), "frame {index}");
        }
    };
    tokio::time::timeout(Duration::from_secs(3), reading)
        .await
        .expect("the queued SDUs did not arrive within 3 s of a busy application channel");
    assert!(started.elapsed() < Duration::from_secs(3));

    // One session all along: no write stall, no reconnect.
    assert_eq!(counters.borrow().registrations, 1);
    assert_eq!(counters.borrow().sdus_sent, SENT as u64);
    assert_eq!(*link.borrow(), registered_link(1, SERVICE_MAC));
    assert_eq!(take_pending_connections(&service.control).await, 0);

    let (service_read, service_write) = data.into_split();
    let drain = spawn_drain(service_read);
    drop(busy_channel);
    assert!(join_within(STOP_LIMIT, task).await.is_ok());
    drain.abort();
    drop((control, service_write, received));
}

static STOP_FLOOD: AtomicBool = AtomicBool::new(false);

/// A thread that writes frames to the client's data socket without pause. A task of the test's
/// runtime would not keep the socket full: the loop runs between its turns, and the short reads
/// that follow clear tokio's readiness until the driver turns again. The socket stays readable
/// all the time only if the writer runs beside the loop. Dropping it stops and joins the thread.
struct Flood {
    stop: &'static AtomicBool,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Flood {
    fn start(stop: &'static AtomicBool, service_write: std::os::unix::net::UnixStream) -> Self {
        let thread = std::thread::spawn(move || {
            // The socket is non-blocking (the file description is shared with the tokio end).
            // One frame, then a short pause, keeps the client's reads short and the socket
            // readable again by the time the loop waits.
            let frame = sdu_frame(SERVICE_MAC, PEER_MAC, UNFRAGMENTED, &vec![0; 20_000]);
            let mut writer = &service_write;
            // A partial write must go on where it stopped, or the stream is no longer frames.
            let mut offset = 0;
            while !stop.load(Ordering::Relaxed) {
                match std::io::Write::write(&mut writer, &frame[offset..]) {
                    Ok(written) => {
                        offset = (offset + written) % frame.len();
                        if offset == 0 {
                            std::thread::sleep(Duration::from_micros(100));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_micros(50));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Flood {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // A panic of the flood must not turn a failing test into an abort.
            let _ = thread.join();
        }
    }
}

/// The inbound side is never idle (the service floods frames) while a write is blocked: the
/// writable wait must still be stored, or the queued SDUs wait for the 5 s write stall, which
/// ends the session. `poll_branches` tries writable before readable for this reason. Real time,
/// the loop a task of the current-thread runtime.
#[tokio::test]
async fn blocked_write_resumes_while_the_service_floods_the_client() {
    const SENT: usize = 50;
    let (service, session, client) = registered(Role::Agent).await;
    let LoopClient {
        sdus,
        received,
        link,
        counters,
        task,
    } = client;
    let ServiceSession { control, data, .. } = session;
    // The flood writes through a second handle of the same socket; this end only reads.
    let data = data.into_std().unwrap();
    let flood = Flood::start(&STOP_FLOOD, data.try_clone().unwrap());
    let mut service_read = UnixStream::from_std(data).unwrap();

    // Nobody reads `received`: its queue drops the oldest and the reads go on, so the read side
    // is ready all the time.

    // The service does not read its data socket yet: it fills, and the rest of the 50 SDUs
    // (65 019 octets each, below the 4 MiB bound) waits in the outbound buffer.
    for index in 0..SENT {
        let number = u8::try_from(index).unwrap();
        sdus.send(out(1, LOCAL_MAC, PEER_MAC, &vec![number; 65_000]))
            .await;
    }
    settle().await;
    assert!(
        counters.borrow().sdus_sent < SENT as u64,
        "the socket never filled"
    );

    let started = std::time::Instant::now();
    let reading = async {
        for index in 0..SENT {
            let mut prefix = [0u8; 4];
            service_read.read_exact(&mut prefix).await.unwrap();
            let mut frame = vec![0u8; u32::from_be_bytes(prefix) as usize];
            service_read.read_exact(&mut frame).await.unwrap();
            assert_eq!(frame[15], u8::try_from(index).unwrap(), "frame {index}");
        }
    };
    tokio::time::timeout(Duration::from_secs(3), reading)
        .await
        .expect("the queued SDUs did not arrive within 3 s of a flood from the service");
    assert!(started.elapsed() < Duration::from_secs(3));

    // One session all along: no write stall, no reconnect.
    assert_eq!(counters.borrow().registrations, 1);
    assert_eq!(counters.borrow().sdus_sent, SENT as u64);
    assert_eq!(*link.borrow(), registered_link(1, SERVICE_MAC));
    assert_eq!(take_pending_connections(&service.control).await, 0);

    drop(flood);
    let (service_read, service_write) = service_read.into_split();
    let drain = spawn_drain(service_read);
    drop(sdus);
    assert!(join_within(STOP_LIMIT, task).await.is_ok());
    drain.abort();
    drop((control, received, service_write));
}

/// Real time: a service that sends without pause, a client that must still write.
#[tokio::test]
async fn continuously_ready_reads_do_not_starve_the_sender() {
    // intentional-difference:rx-read-between-passes (the other direction)
    let (_service, session, mut client) = registered(Role::Agent).await;
    let ServiceSession { control, data, .. } = session;
    let (mut service_read, mut service_write) = data.into_split();

    // Nobody reads `received`: the queue fills and drops its oldest, the reads go on.
    let flood = tokio::spawn(async move {
        let frames = numbered_sdu_frames(0, 64);
        while service_write.write_all(&frames).await.is_ok() {}
    });
    wait_for_counters(&mut client, |c| c.sdus_received >= 1000).await;

    client.sdus.send(out(1, LOCAL_MAC, PEER_MAC, b"ping")).await;
    let mut frame = [0u8; 4 + 15 + 4];
    let read = tokio::time::timeout(Duration::from_secs(5), service_read.read_exact(&mut frame));
    read.await
        .expect("the SDU was not written under sustained reads")
        .unwrap();
    assert_eq!(&frame[19..], b"ping");

    flood.abort();
    let LoopClient { sdus, task, .. } = client;
    drop(sdus);
    assert!(join_within(STOP_LIMIT, task).await.is_ok());
    drop((control, service_read));
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
