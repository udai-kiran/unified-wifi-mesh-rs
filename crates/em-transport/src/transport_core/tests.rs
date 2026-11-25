// SPDX-License-Identifier: Apache-2.0

//! Scenario tests of `TransportCore`, and a seeded random test of its invariants.

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use em_types::MacAddr;

use super::{
    RegistrationStep, State, TransportAction, TransportCore, TransportOutcome, TransportPolicy,
};
use crate::frame_splitter::Frame;
use crate::test_prng::Prng;
use crate::wire::{MAX_SDU_PAYLOAD, encode_sdu_frames};
use crate::{LinkState, OutboundSdu, Sdu, TransportCounters};

const LOCAL: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 0xaa]);
const REMOTE: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 0xbb]);

fn ms(count: u64) -> Duration {
    Duration::from_millis(count)
}

/// An SDU whose `local` carries `tag`, so that a test can tell SDUs apart in the wire bytes.
fn sdu(tag: u8, length: usize) -> Sdu {
    Sdu {
        local: MacAddr::new([2, 0, 0, 0, 0, tag]),
        remote: REMOTE,
        cmdu: vec![tag; length],
    }
}

/// The octets of one SDU on the wire, as the service writes them to the data socket.
fn wire_bytes(sdu: &Sdu) -> Vec<u8> {
    encode_sdu_frames(sdu).unwrap().concat()
}

/// One frame with the given SDU header fields, with its length prefix.
fn raw_frame(is_fragment: u8, is_last: u8, id: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&LOCAL.octets());
    body.extend_from_slice(&REMOTE.octets());
    body.extend_from_slice(&[is_fragment, is_last, id]);
    body.extend_from_slice(payload);
    let mut frame = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
    frame.extend_from_slice(&body);
    frame
}

fn response(al_mac: MacAddr, result: u8) -> Frame {
    let mut body = al_mac.octets().to_vec();
    body.push(result);
    Frame::Complete(body)
}

/// A core, the clock the test moves, and the actions collected since the last `take_actions`.
struct Rig {
    core: TransportCore,
    now: Instant,
    actions: Vec<TransportAction>,
}

impl Rig {
    fn with_policy(policy: TransportPolicy) -> Self {
        let now = Instant::now();
        Self {
            core: TransportCore::new(policy, now),
            now,
            actions: Vec::new(),
        }
    }

    fn easymesh() -> Self {
        Self::with_policy(TransportPolicy::easymesh())
    }

    fn take_actions(&mut self) -> Vec<TransportAction> {
        std::mem::take(&mut self.actions)
    }

    fn tick(&mut self) {
        self.core.tick(self.now, &mut self.actions);
    }

    fn advance(&mut self, by: Duration) {
        self.now += by;
        self.tick();
    }

    /// The first tick: `Connect`.
    fn connect(&mut self) {
        self.tick();
        assert_eq!(self.take_actions(), [TransportAction::Connect]);
    }

    fn registration_frame(&mut self, frame: Frame) {
        self.core
            .registration_frame(self.now, frame, &mut self.actions);
    }

    fn registration_failed(&mut self, step: RegistrationStep, kind: ErrorKind) {
        self.core
            .registration_failed(self.now, step, kind, &mut self.actions);
    }

    /// Connect and register; the session is the next one.
    fn register(&mut self) {
        self.connect();
        self.registration_frame(response(LOCAL, 1));
        assert_eq!(self.take_actions(), []);
    }

    /// From a session, end it, wait the reconnect delay and register again.
    fn reregister(&mut self) {
        // The end of the earlier session is still in the list.
        assert!(
            self.actions
                .iter()
                .all(|action| *action == TransportAction::CloseSession)
        );
        self.actions.clear();
        self.advance(ms(2000));
        assert_eq!(self.take_actions(), [TransportAction::Connect]);
        self.registration_frame(response(LOCAL, 1));
        assert_eq!(self.take_actions(), []);
    }

    fn send(&mut self, session: u64, sdu: Sdu) {
        self.core.send(self.now, OutboundSdu { session, sdu });
    }

    /// Sends into the current session.
    fn send_now(&mut self, sdu: Sdu) {
        let session = self.core.link().session;
        self.send(session, sdu);
    }

    fn stop(&mut self) {
        self.core.stop(self.now, &mut self.actions);
    }

    fn read_ended(&mut self) {
        self.core.read_ended(self.now, &mut self.actions);
    }

    fn write_failed(&mut self) {
        self.core.write_failed(self.now, &mut self.actions);
    }

    /// Feeds bytes and serves every frame in them.
    fn receive(&mut self, bytes: &[u8]) {
        self.core.bytes_read(bytes);
        while self.core.read_ready() {
            self.core.serve_read(self.now);
        }
    }

    /// Writes everything the buffer holds, as a socket that always accepts.
    fn write_all(&mut self) {
        while let Some(length) = self.core.write_chunk(self.now).map(<[u8]>::len) {
            self.core.wrote(self.now, length);
        }
    }

    fn counters(&self) -> TransportCounters {
        self.core.counters()
    }

    fn take_all_received(&mut self) -> Vec<Sdu> {
        let mut taken = Vec::new();
        while let Some(sdu) = self.core.take_received() {
            taken.push(sdu);
        }
        taken
    }
}

// ----- construction and registration -----

#[test]
fn a_new_core_asks_to_connect_at_once() {
    let mut rig = Rig::easymesh();
    assert_eq!(rig.core.next_deadline(), Some(rig.now));
    assert_eq!(rig.core.link(), LinkState::default());
    assert_eq!(rig.counters(), TransportCounters::default());
    rig.connect();
    assert_eq!(rig.core.next_deadline(), None);
}

#[test]
fn the_easymesh_policy_has_the_decided_numbers() {
    // 0016 D3b: U1, U2, U3, U4, U8, U12 and the 2 s reconnect delay.
    let policy = TransportPolicy::easymesh();
    assert_eq!(policy.reconnect_delay, Duration::from_secs(2));
    assert_eq!(policy.rx_queue_sdus, 1024);
    assert_eq!(policy.rx_queue_bytes, 4 * 1024 * 1024);
    assert_eq!(policy.rx_sdu_cap, 1024 * 1024);
    assert_eq!(policy.tx_buffer_bytes, 4 * 1024 * 1024);
    assert_eq!(policy.write_stall, Duration::from_secs(5));
    assert_eq!(policy.stop_write_limit, ms(900));
    assert_eq!(policy.linger, ms(100));
}

#[test]
fn registration_starts_a_session() {
    let mut rig = Rig::easymesh();
    rig.register();
    assert_eq!(rig.counters().registrations, 1);
    assert_eq!(
        rig.core.link(),
        LinkState {
            session: 1,
            al_mac: Some(LOCAL)
        }
    );
    assert_eq!(rig.core.next_deadline(), None);
}

#[test]
fn registration_ignores_octets_after_the_seventh() {
    let mut rig = Rig::easymesh();
    rig.connect();
    let mut body = LOCAL.octets().to_vec();
    body.extend_from_slice(&[1, 9, 9, 9]);
    rig.registration_frame(Frame::Complete(body));
    assert_eq!(rig.core.link().al_mac, Some(LOCAL));
}

#[test]
fn a_registration_error_is_retried_or_fatal_by_step_and_kind() {
    // `actor.rs` classify_connect and classify_registration_io.
    let connect_retries = [ErrorKind::NotFound, ErrorKind::ConnectionRefused];
    let io_retries = [
        ErrorKind::UnexpectedEof,
        ErrorKind::ConnectionReset,
        ErrorKind::ConnectionAborted,
        ErrorKind::BrokenPipe,
    ];
    let kinds = [
        ErrorKind::NotFound,
        ErrorKind::ConnectionRefused,
        ErrorKind::UnexpectedEof,
        ErrorKind::ConnectionReset,
        ErrorKind::ConnectionAborted,
        ErrorKind::BrokenPipe,
        ErrorKind::PermissionDenied,
        ErrorKind::TimedOut,
        ErrorKind::InvalidData,
        ErrorKind::Other,
    ];
    let steps = [
        (
            RegistrationStep::ConnectControl,
            "connect control socket",
            &connect_retries[..],
        ),
        (
            RegistrationStep::ConnectData,
            "connect data socket",
            &connect_retries[..],
        ),
        (
            RegistrationStep::SendRequest,
            "send registration request",
            &io_retries[..],
        ),
        (
            RegistrationStep::ReadResponse,
            "read registration response",
            &io_retries[..],
        ),
    ];
    for (step, operation, retries) in steps {
        for kind in kinds {
            let mut rig = Rig::easymesh();
            rig.connect();
            rig.registration_failed(step, kind);
            if retries.contains(&kind) {
                assert_eq!(
                    rig.take_actions(),
                    [TransportAction::CloseSession],
                    "{operation} {kind:?}"
                );
                assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(2000)));
            } else {
                assert_eq!(
                    rig.take_actions(),
                    [
                        TransportAction::CloseSession,
                        TransportAction::Finish(TransportOutcome::IoFailed { operation })
                    ],
                    "{operation} {kind:?}"
                );
                assert_eq!(rig.core.next_deadline(), None);
            }
        }
    }
}

#[test]
fn a_retry_waits_the_reconnect_delay_then_connects() {
    let mut rig = Rig::easymesh();
    rig.connect();
    rig.registration_failed(RegistrationStep::ConnectControl, ErrorKind::NotFound);
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    rig.advance(ms(1999));
    assert_eq!(rig.take_actions(), []);
    rig.advance(ms(1));
    assert_eq!(rig.take_actions(), [TransportAction::Connect]);
}

#[test]
fn a_bad_response_frame_is_retried() {
    let bad_frames = [
        Frame::Oversize { length: 70_000 },
        Frame::Complete(vec![2, 0, 0, 0, 0, 1]),
        Frame::Complete(Vec::new()),
        response(MacAddr::new([0; 6]), 1),
    ];
    for frame in bad_frames {
        let mut rig = Rig::easymesh();
        rig.connect();
        rig.registration_frame(frame);
        assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
        assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(2000)));
        assert_eq!(rig.counters().registrations, 0);
        assert_eq!(rig.core.link(), LinkState::default());
    }
}

#[test]
fn a_rejection_finishes_with_the_result() {
    for result in [0x00, 0x02, 0xff] {
        let mut rig = Rig::easymesh();
        rig.connect();
        rig.registration_frame(response(LOCAL, result));
        assert_eq!(
            rig.take_actions(),
            [
                TransportAction::CloseSession,
                TransportAction::Finish(TransportOutcome::Rejected { result })
            ]
        );
        assert_eq!(rig.counters().registrations, 0);
    }
}

#[test]
fn the_rejection_is_checked_before_the_all_zero_mac() {
    let mut rig = Rig::easymesh();
    rig.connect();
    rig.registration_frame(response(MacAddr::new([0; 6]), 2));
    assert_eq!(
        rig.take_actions().last(),
        Some(&TransportAction::Finish(TransportOutcome::Rejected {
            result: 2
        }))
    );
}

#[test]
fn a_fatal_verdict_counts_the_received_queue_as_stopped() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.receive(&wire_bytes(&sdu(1, 3)));
    rig.receive(&wire_bytes(&sdu(2, 3)));
    rig.read_ended();
    rig.reregister_to_rejection();
    assert_eq!(rig.counters().dropped_receive_stopped, 2);
    assert_eq!(rig.counters().sdus_received, 2);
    assert!(!rig.core.received_ready());
    assert_eq!(rig.core.take_received(), None);
}

impl Rig {
    fn reregister_to_rejection(&mut self) {
        self.actions.clear();
        self.advance(ms(2000));
        assert_eq!(self.take_actions(), [TransportAction::Connect]);
        self.registration_frame(response(LOCAL, 2));
        assert_eq!(
            self.take_actions(),
            [
                TransportAction::CloseSession,
                TransportAction::Finish(TransportOutcome::Rejected { result: 2 })
            ]
        );
    }
}

#[test]
fn registration_inputs_outside_an_attempt_are_ignored() {
    let mut rig = Rig::easymesh();
    // Before the first tick: disconnected, no attempt.
    rig.registration_frame(response(LOCAL, 1));
    rig.registration_failed(RegistrationStep::ConnectData, ErrorKind::Other);
    assert_eq!(rig.take_actions(), []);
    assert_eq!(rig.counters().registrations, 0);
    // In a session.
    rig.register();
    rig.registration_frame(response(LOCAL, 2));
    rig.registration_failed(RegistrationStep::ReadResponse, ErrorKind::Other);
    assert_eq!(rig.take_actions(), []);
    assert_eq!(rig.core.link().session, 1);
}

#[test]
fn sessions_are_numbered_and_the_link_follows() {
    let mut rig = Rig::easymesh();
    rig.register();
    assert_eq!(rig.core.link().session, 1);
    rig.read_ended();
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    // The session number stays, the AL MAC goes.
    assert_eq!(
        rig.core.link(),
        LinkState {
            session: 1,
            al_mac: None
        }
    );
    rig.reregister();
    assert_eq!(
        rig.core.link(),
        LinkState {
            session: 2,
            al_mac: Some(LOCAL)
        }
    );
    assert_eq!(rig.counters().registrations, 2);
}

// ----- sending -----

#[test]
fn a_send_while_not_in_a_session_is_counted_as_disconnected() {
    let mut rig = Rig::easymesh();
    rig.send(0, sdu(1, 1));
    rig.connect();
    rig.send(0, sdu(2, 1));
    assert_eq!(rig.counters().dropped_send_disconnected, 2);
    assert!(!rig.core.write_ready(rig.now));
    assert_eq!(rig.core.write_chunk(rig.now), None);
}

#[test]
fn an_sdu_for_another_session_is_dropped_and_the_current_one_is_written() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.read_ended();
    rig.reregister();
    assert_eq!(rig.core.link().session, 2);
    rig.send(1, sdu(1, 1));
    rig.send(3, sdu(2, 1));
    assert_eq!(rig.counters().dropped_send_disconnected, 2);
    assert!(!rig.core.write_ready(rig.now));
    rig.send(2, sdu(3, 1));
    assert!(rig.core.write_ready(rig.now));
    assert_eq!(
        rig.core.write_chunk(rig.now),
        Some(&wire_bytes(&sdu(3, 1))[..])
    );
}

#[test]
fn a_send_after_a_session_ended_is_disconnected_not_buffered() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.read_ended();
    let session = rig.core.link().session;
    rig.send(session, sdu(1, 1));
    assert_eq!(rig.counters().dropped_send_disconnected, 1);
    rig.reregister();
    assert!(!rig.core.write_ready(rig.now));
}

#[test]
fn writes_count_when_an_sdu_completes() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.send_now(sdu(1, 10));
    let whole = wire_bytes(&sdu(1, 10));
    assert_eq!(rig.core.write_chunk(rig.now), Some(&whole[..]));
    rig.core.wrote(rig.now, 10);
    assert_eq!(rig.counters().sdus_sent, 0);
    assert_eq!(rig.core.write_chunk(rig.now), Some(&whole[10..]));
    rig.core.wrote(rig.now, whole.len() - 10);
    assert_eq!(rig.counters().sdus_sent, 1);
    assert_eq!(rig.core.write_chunk(rig.now), None);
    assert!(!rig.core.write_ready(rig.now));
}

#[test]
fn wrote_ignores_zero_and_a_missing_chunk() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.core.wrote(rig.now, 5);
    rig.send_now(sdu(1, 10));
    rig.core.wrote(rig.now, 0);
    assert_eq!(rig.core.write_chunk(rig.now).map(<[u8]>::len), Some(29));
}

#[test]
fn the_write_source_is_not_ready_while_the_socket_is_blocked() {
    // 0016 E10.
    let mut rig = Rig::easymesh();
    rig.register();
    assert!(!rig.core.write_ready(rig.now));
    rig.send_now(sdu(1, 10));
    assert!(rig.core.write_ready(rig.now));
    rig.core.write_blocked();
    assert!(!rig.core.write_ready(rig.now));
    // The chunk is still there for a caller that wants to look.
    assert!(rig.core.write_chunk(rig.now).is_some());
    rig.core.writable();
    assert!(rig.core.write_ready(rig.now));
}

#[test]
fn a_new_session_starts_with_a_writable_socket() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.core.write_blocked();
    rig.read_ended();
    rig.reregister();
    rig.send_now(sdu(1, 1));
    assert!(rig.core.write_ready(rig.now));
}

#[test]
fn the_outbound_buffer_overflow_counts_and_drops_the_oldest_not_started() {
    // intentional-difference:tx-buffer
    // The old `PendingFrames` held 64 frames; the buffer holds 4 MiB and counts its own counter.
    let mut rig = Rig::easymesh();
    rig.register();
    for tag in 1..=4 {
        rig.send_now(sdu(tag, 1_000_000));
    }
    assert_eq!(rig.counters().dropped_send_overflow, 0);
    rig.send_now(sdu(5, 1_000_000));
    assert_eq!(rig.counters().dropped_send_overflow, 1);
    // SDU 1 went; SDU 2 is now the oldest. The chunk starts with the prefix and the local MAC.
    let chunk = rig.core.write_chunk(rig.now).unwrap();
    assert_eq!(&chunk[4..10], &sdu(2, 0).local.octets());
    // Nothing else is counted yet.
    assert_eq!(rig.counters().dropped_send_failed, 0);
    assert_eq!(rig.counters().dropped_send_disconnected, 0);
}

#[test]
fn an_sdu_that_needs_more_than_256_fragments_counts_as_failed() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.send_now(sdu(1, MAX_SDU_PAYLOAD + 1));
    assert_eq!(rig.counters().dropped_send_failed, 1);
    assert_eq!(rig.core.write_chunk(rig.now), None);
    assert_eq!(rig.core.next_deadline(), None);
}

// ----- write stall -----

#[test]
fn a_stalled_write_ends_the_session_at_exactly_the_stall_time() {
    // intentional-difference:tx-write-stall-5s
    // The old actor had no time limit on a write (0011 A7).
    let mut rig = Rig::easymesh();
    rig.register();
    rig.send_now(sdu(1, 10));
    assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(5000)));
    rig.advance(ms(4999));
    assert_eq!(rig.take_actions(), []);
    assert!(rig.core.link().al_mac.is_some());
    rig.advance(ms(1));
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    assert_eq!(rig.core.link().al_mac, None);
    assert_eq!(rig.counters().dropped_send_disconnected, 1);
    // And it reconnects.
    assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(2000)));
    rig.reregister();
    assert_eq!(rig.core.link().session, 2);
}

#[test]
fn a_blocked_socket_does_not_stop_the_stall_clock() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.send_now(sdu(1, 10));
    rig.core.write_blocked();
    rig.advance(ms(5000));
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
}

#[test]
fn progress_restarts_the_stall_clock() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.send_now(sdu(1, 10));
    rig.advance(ms(4000));
    rig.core.wrote(rig.now, 1);
    assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(5000)));
    rig.advance(ms(4999));
    assert_eq!(rig.take_actions(), []);
    rig.core.wrote(rig.now, 1);
    rig.advance(ms(4999));
    assert_eq!(rig.take_actions(), []);
    rig.advance(ms(1));
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    // The partly written SDU is a failed one.
    assert_eq!(rig.counters().dropped_send_failed, 1);
}

#[test]
fn a_write_one_tick_before_the_stall_deadline_is_allowed_and_resets_the_clock() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.send_now(sdu(1, 10));
    let started = rig.now;
    rig.now = started + ms(4999);
    assert!(rig.core.write_ready(rig.now));
    assert!(rig.core.write_chunk(rig.now).is_some());
    rig.core.wrote(rig.now, 1);
    assert_eq!(rig.core.next_deadline(), Some(started + ms(4999 + 5000)));
    // The old deadline passes without ending the session.
    rig.advance(ms(1));
    assert_eq!(rig.take_actions(), []);
    assert!(rig.core.write_ready(rig.now));
}

#[test]
fn a_due_stall_deadline_wins_over_a_write_at_the_same_now() {
    // The event loop serves the write source before the deadline tick. At and after the stall
    // deadline no write starts, so it cannot reset the clock of a session that stalled.
    for late in [0, 1, 500] {
        let mut rig = Rig::easymesh();
        rig.register();
        rig.send_now(sdu(1, 10));
        rig.now += ms(5000 + late);
        assert!(!rig.core.write_ready(rig.now), "late {late}");
        assert_eq!(rig.core.write_chunk(rig.now), None, "late {late}");
        rig.tick();
        assert_eq!(
            rig.take_actions(),
            [TransportAction::CloseSession],
            "late {late}"
        );
        assert_eq!(rig.counters().dropped_send_disconnected, 1, "late {late}");
        assert_eq!(rig.counters().dropped_send_failed, 0, "late {late}");
        assert_eq!(rig.counters().sdus_sent, 0, "late {late}");
    }
}

#[test]
fn the_stall_clock_is_not_running_while_the_buffer_is_empty() {
    let mut rig = Rig::easymesh();
    rig.register();
    assert_eq!(rig.core.next_deadline(), None);
    rig.send_now(sdu(1, 10));
    rig.write_all();
    assert_eq!(rig.counters().sdus_sent, 1);
    assert_eq!(rig.core.next_deadline(), None);
    rig.advance(ms(60_000));
    assert_eq!(rig.take_actions(), []);
}

#[test]
fn the_stall_clock_starts_when_the_buffer_becomes_non_empty_and_a_later_send_does_not_restart_it() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.advance(ms(1000));
    let started = rig.now;
    rig.send_now(sdu(1, 10));
    rig.advance(ms(3000));
    rig.send_now(sdu(2, 10));
    assert_eq!(rig.core.next_deadline(), Some(started + ms(5000)));
}

// ----- the counting rule at a session end -----

#[derive(Clone, Copy, Debug)]
enum EndCause {
    ReadEnded,
    WriteFailed,
    Stall,
}

/// A session with one SDU partly written, two not started, an unfinished fragment set and half a
/// frame in the splitter.
fn session_with_everything_in_flight(rig: &mut Rig) {
    rig.register();
    rig.send_now(sdu(1, 10));
    rig.send_now(sdu(2, 10));
    rig.send_now(sdu(3, 10));
    rig.core.wrote(rig.now, 5);
    rig.receive(&raw_frame(1, 0, 0, &[1, 2, 3]));
    rig.core.bytes_read(&[0, 0, 0]);
}

#[test]
fn every_session_end_counts_by_the_same_rule() {
    for cause in [EndCause::ReadEnded, EndCause::WriteFailed, EndCause::Stall] {
        let mut rig = Rig::easymesh();
        session_with_everything_in_flight(&mut rig);
        match cause {
            EndCause::ReadEnded => rig.read_ended(),
            EndCause::WriteFailed => rig.write_failed(),
            EndCause::Stall => rig.advance(ms(5000)),
        }
        let counters = rig.counters();
        assert_eq!(counters.dropped_send_failed, 1, "{cause:?}");
        assert_eq!(counters.dropped_send_disconnected, 2, "{cause:?}");
        assert_eq!(counters.dropped_receive_fragments, 1, "{cause:?}");
        assert_eq!(counters.sdus_sent, 0, "{cause:?}");
        assert_eq!(rig.core.link().al_mac, None, "{cause:?}");
        assert_eq!(
            rig.take_actions(),
            [TransportAction::CloseSession],
            "{cause:?}"
        );
        assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(2000)));
        assert!(!rig.core.write_ready(rig.now));
        assert!(!rig.core.read_ready());

        // The splitter starts again: the three octets of the old half frame are gone.
        rig.reregister();
        rig.receive(&wire_bytes(&sdu(9, 4)));
        assert_eq!(rig.take_all_received(), [sdu(9, 4)], "{cause:?}");
        assert_eq!(rig.counters().dropped_receive_malformed, 0, "{cause:?}");
        assert_eq!(rig.counters().dropped_receive_fragments, 1, "{cause:?}");
    }
}

#[test]
fn a_write_error_before_any_octet_counts_the_sdu_as_disconnected() {
    // The counting rule: only a partly written SDU is a failed one.
    let mut rig = Rig::easymesh();
    rig.register();
    rig.send_now(sdu(1, 10));
    rig.write_failed();
    assert_eq!(rig.counters().dropped_send_failed, 0);
    assert_eq!(rig.counters().dropped_send_disconnected, 1);
}

#[test]
fn the_received_queue_survives_a_reconnect() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.receive(&wire_bytes(&sdu(1, 3)));
    rig.receive(&wire_bytes(&sdu(2, 3)));
    rig.read_ended();
    assert!(rig.core.received_ready());
    rig.reregister();
    assert_eq!(rig.take_all_received(), [sdu(1, 3), sdu(2, 3)]);
    assert_eq!(rig.counters().dropped_receive_stopped, 0);
}

// ----- reading -----

#[test]
fn bytes_are_ignored_outside_a_session() {
    let mut rig = Rig::easymesh();
    rig.core.bytes_read(&wire_bytes(&sdu(1, 3)));
    assert!(!rig.core.read_ready());
    rig.register();
    assert!(!rig.core.read_ready());
    rig.read_ended();
    rig.core.bytes_read(&wire_bytes(&sdu(1, 3)));
    rig.reregister();
    assert!(!rig.core.read_ready());
}

#[test]
fn frames_left_in_the_splitter_keep_the_read_source_ready() {
    let mut rig = Rig::easymesh();
    rig.register();
    let mut bytes = wire_bytes(&sdu(1, 3));
    bytes.extend_from_slice(&wire_bytes(&sdu(2, 3)));
    rig.core.bytes_read(&bytes);
    assert!(rig.core.read_ready());
    assert!(!rig.core.received_ready());
    rig.core.serve_read(rig.now);
    assert!(rig.core.read_ready());
    assert!(rig.core.received_ready());
    rig.core.serve_read(rig.now);
    assert!(!rig.core.read_ready());
    // With nothing to serve it does nothing.
    rig.core.serve_read(rig.now);
    assert_eq!(rig.counters().sdus_received, 2);
    assert_eq!(rig.take_all_received(), [sdu(1, 3), sdu(2, 3)]);
}

#[test]
fn receive_errors_are_counted_and_keep_the_session() {
    let mut rig = Rig::easymesh();
    rig.register();
    // Oversize: prefix 65 537, then that many octets, then a good frame.
    let mut bytes = 65_537u32.to_be_bytes().to_vec();
    bytes.extend_from_slice(&vec![7; 65_537]);
    // Malformed: shorter than the 15-octet SDU header.
    bytes.extend_from_slice(&[0, 0, 0, 3, 1, 2, 3]);
    // A stray continuation, then a good frame.
    bytes.extend_from_slice(&raw_frame(1, 1, 4, &[1]));
    bytes.extend_from_slice(&wire_bytes(&sdu(5, 2)));
    rig.receive(&bytes);
    let counters = rig.counters();
    assert_eq!(counters.dropped_receive_oversize, 1);
    assert_eq!(counters.dropped_receive_malformed, 1);
    assert_eq!(counters.dropped_receive_fragments, 1);
    assert_eq!(counters.sdus_received, 1);
    assert_eq!(rig.take_all_received(), [sdu(5, 2)]);
    assert!(rig.core.link().al_mac.is_some());
}

#[test]
fn fragments_are_joined() {
    let mut rig = Rig::easymesh();
    rig.register();
    let mut bytes = raw_frame(1, 0, 0, &[1, 2]);
    bytes.extend_from_slice(&raw_frame(1, 1, 1, &[3]));
    rig.receive(&bytes);
    let joined = Sdu {
        local: LOCAL,
        remote: REMOTE,
        cmdu: vec![1, 2, 3],
    };
    assert_eq!(rig.take_all_received(), [joined]);
}

#[test]
fn a_set_over_one_mib_is_refused_and_counted_once() {
    // intentional-difference:rx-sdu-cap-1mib
    // The old actor had no cap below the framing limit.
    let mut rig = Rig::easymesh();
    rig.register();
    rig.receive(&wire_bytes(&sdu(1, 1_100_000)));
    let counters = rig.counters();
    assert_eq!(counters.dropped_receive_too_large, 1);
    assert_eq!(counters.dropped_receive_fragments, 0);
    assert_eq!(counters.sdus_received, 0);
    assert!(!rig.core.received_ready());
    // A set exactly at the cap is delivered.
    rig.receive(&wire_bytes(&sdu(2, 1_048_576)));
    assert_eq!(rig.counters().dropped_receive_too_large, 1);
    assert_eq!(rig.counters().sdus_received, 1);
}

#[test]
fn the_received_queue_holds_1024_sdus() {
    // intentional-difference:rx-queue-1024-4mib
    // The old actor queue held 256 SDUs.
    let mut rig = Rig::easymesh();
    rig.register();
    for tag in 0..=255u8 {
        rig.receive(&wire_bytes(&sdu(tag, 1)));
    }
    assert_eq!(rig.counters().dropped_receive_overflow, 0);
    for _ in 0..(1024 - 256) {
        rig.receive(&wire_bytes(&sdu(0, 1)));
    }
    assert_eq!(rig.counters().dropped_receive_overflow, 0);
    rig.receive(&wire_bytes(&sdu(9, 1)));
    assert_eq!(rig.counters().dropped_receive_overflow, 1);
    assert_eq!(rig.counters().sdus_received, 1025);
    assert_eq!(rig.take_all_received().len(), 1024);
}

#[test]
fn an_sdu_above_the_byte_limit_is_dropped_alone() {
    // 0016 E6 through the core.
    let mut policy = TransportPolicy::easymesh();
    policy.rx_queue_bytes = 10;
    let mut rig = Rig::with_policy(policy);
    rig.register();
    rig.receive(&wire_bytes(&sdu(1, 4)));
    rig.receive(&wire_bytes(&sdu(2, 11)));
    assert_eq!(rig.counters().sdus_received, 2);
    assert_eq!(rig.counters().dropped_receive_overflow, 1);
    assert_eq!(rig.take_all_received(), [sdu(1, 4)]);
}

// ----- stop -----

/// A session with two SDUs queued, the stop requested at `rig.now`.
fn stopping_with_two_sdus(rig: &mut Rig) {
    rig.register();
    rig.send_now(sdu(1, 10));
    rig.send_now(sdu(2, 10));
    rig.stop();
    assert_eq!(rig.take_actions(), []);
}

#[test]
fn a_stop_writes_until_the_limit_then_lingers_then_finishes() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    let stopped_at = rig.now;
    assert!(rig.core.write_ready(rig.now));
    assert_eq!(rig.core.next_deadline(), Some(stopped_at + ms(900)));

    // The first SDU leaves, the second does not.
    let first = wire_bytes(&sdu(1, 10)).len();
    rig.core.wrote(rig.now, first);
    assert_eq!(rig.counters().sdus_sent, 1);
    rig.advance(ms(899));
    assert_eq!(rig.take_actions(), []);
    assert!(rig.core.write_ready(rig.now));

    // At the limit the rest is counted, the sockets stay open.
    rig.advance(ms(1));
    assert_eq!(rig.take_actions(), []);
    assert_eq!(rig.counters().dropped_send_disconnected, 1);
    assert_eq!(rig.counters().dropped_send_failed, 0);
    assert!(!rig.core.write_ready(rig.now));
    assert!(rig.core.link().al_mac.is_some());
    assert_eq!(rig.core.next_deadline(), Some(stopped_at + ms(1000)));

    rig.advance(ms(99));
    assert_eq!(rig.take_actions(), []);
    rig.advance(ms(1));
    assert_eq!(
        rig.take_actions(),
        [
            TransportAction::CloseSession,
            TransportAction::Finish(TransportOutcome::Stopped)
        ]
    );
    assert_eq!(rig.now - stopped_at, ms(1000));
    assert_eq!(rig.core.link().al_mac, None);
    assert_eq!(rig.core.next_deadline(), None);
}

#[test]
fn a_late_tick_still_ends_the_stop_by_the_limit_plus_the_linger() {
    // 0016 U8: the whole stop takes at most stop_write_limit + linger, even when the driver
    // wakes late.
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    rig.advance(ms(1500));
    assert_eq!(
        rig.take_actions(),
        [
            TransportAction::CloseSession,
            TransportAction::Finish(TransportOutcome::Stopped)
        ]
    );
    assert_eq!(rig.counters().dropped_send_disconnected, 2);
    assert_eq!(rig.core.next_deadline(), None);
}

#[test]
fn a_late_tick_after_the_limit_lingers_only_until_limit_plus_linger() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    let stopped_at = rig.now;
    rig.advance(ms(950));
    assert_eq!(rig.take_actions(), []);
    assert_eq!(rig.core.next_deadline(), Some(stopped_at + ms(1000)));
}

/// What a finished stop returns.
const CLOSE_AND_STOPPED: [TransportAction; 2] = [
    TransportAction::CloseSession,
    TransportAction::Finish(TransportOutcome::Stopped),
];

/// The late-input order of the driver: the write result is delivered at 950 ms, before the tick
/// that would have ended the writes at the 900 ms limit. The stop must still end at 1000 ms.
fn assert_late_input_ends_the_stop_at_1000(rig: &mut Rig, stopped_at: Instant) {
    assert_eq!(
        rig.core.next_deadline(),
        Some(stopped_at + ms(1000)),
        "the linger runs from the limit"
    );
    rig.tick();
    assert_eq!(rig.take_actions(), []);
    rig.now = stopped_at + ms(1000);
    rig.tick();
    assert_eq!(rig.take_actions(), CLOSE_AND_STOPPED);
    assert_eq!(rig.core.next_deadline(), None);
}

#[test]
fn a_write_failure_after_the_limit_still_ends_the_stop_by_limit_plus_linger() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    let stopped_at = rig.now;
    rig.now = stopped_at + ms(950);
    rig.write_failed();
    assert_eq!(rig.counters().dropped_send_disconnected, 2);
    assert_late_input_ends_the_stop_at_1000(&mut rig, stopped_at);
}

#[test]
fn a_write_that_empties_the_buffer_after_the_limit_is_not_accepted() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    let stopped_at = rig.now;
    // The write was issued before the limit; its result comes late.
    let everything = rig.core.write_chunk(rig.now).map(<[u8]>::len).unwrap();
    rig.now = stopped_at + ms(950);
    rig.core.wrote(rig.now, everything);
    // Nothing is written past the limit: the SDUs are counted by the rule, not as sent.
    assert_eq!(rig.counters().sdus_sent, 0);
    assert_eq!(rig.counters().dropped_send_disconnected, 2);
    assert_late_input_ends_the_stop_at_1000(&mut rig, stopped_at);
}

#[test]
fn a_write_that_does_not_empty_the_buffer_after_the_limit_is_not_accepted() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    let stopped_at = rig.now;
    rig.now = stopped_at + ms(950);
    rig.core.wrote(rig.now, 3);
    // The partly written SDU would have counted as failed had the write been accepted.
    assert_eq!(rig.counters().dropped_send_failed, 0);
    assert_eq!(rig.counters().dropped_send_disconnected, 2);
    assert_late_input_ends_the_stop_at_1000(&mut rig, stopped_at);
}

#[test]
fn a_write_one_tick_before_the_stop_limit_is_allowed() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    rig.now += ms(899);
    assert!(rig.core.write_ready(rig.now));
    assert!(rig.core.write_chunk(rig.now).is_some());
    let first = wire_bytes(&sdu(1, 10)).len();
    rig.core.wrote(rig.now, first);
    assert_eq!(rig.counters().sdus_sent, 1);
}

#[test]
fn a_due_stop_limit_wins_over_a_write_at_the_same_now() {
    // At and after the limit no write starts, so the octets that did not leave are not counted
    // as sent and the tick counts them by the rule.
    for late in [0, 1, 50] {
        let mut rig = Rig::easymesh();
        stopping_with_two_sdus(&mut rig);
        let stopped_at = rig.now;
        rig.now = stopped_at + ms(900 + late);
        assert!(!rig.core.write_ready(rig.now), "late {late}");
        assert_eq!(rig.core.write_chunk(rig.now), None, "late {late}");
        rig.tick();
        assert_eq!(rig.take_actions(), [], "late {late}");
        assert_eq!(rig.counters().sdus_sent, 0, "late {late}");
        assert_eq!(rig.counters().dropped_send_disconnected, 2, "late {late}");
        assert_eq!(rig.counters().dropped_send_failed, 0, "late {late}");
        assert_eq!(
            rig.core.next_deadline(),
            Some(stopped_at + ms(1000)),
            "late {late}"
        );
    }
}

#[test]
fn a_stop_counts_a_partly_written_sdu_as_failed_at_the_limit() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    rig.core.wrote(rig.now, 3);
    rig.advance(ms(900));
    assert_eq!(rig.counters().dropped_send_failed, 1);
    assert_eq!(rig.counters().dropped_send_disconnected, 1);
    assert_eq!(rig.counters().sdus_sent, 0);
}

#[test]
fn a_stop_ends_the_writes_when_the_buffer_empties_early() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    rig.advance(ms(10));
    rig.write_all();
    assert_eq!(rig.counters().sdus_sent, 2);
    // The linger counts from the end of the writes.
    assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(100)));
    rig.advance(ms(99));
    assert_eq!(rig.take_actions(), []);
    rig.advance(ms(1));
    assert_eq!(
        rig.take_actions(),
        [
            TransportAction::CloseSession,
            TransportAction::Finish(TransportOutcome::Stopped)
        ]
    );
}

#[test]
fn a_stop_with_an_empty_buffer_lingers_at_once() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.stop();
    assert_eq!(rig.take_actions(), []);
    assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(100)));
    rig.advance(ms(100));
    assert_eq!(
        rig.take_actions(),
        [
            TransportAction::CloseSession,
            TransportAction::Finish(TransportOutcome::Stopped)
        ]
    );
}

#[test]
fn a_failed_write_while_stopping_still_lingers() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    rig.advance(ms(10));
    rig.core.wrote(rig.now, 3);
    rig.write_failed();
    assert_eq!(rig.take_actions(), []);
    // Nothing more is written; the rest is counted by the rule.
    assert!(!rig.core.write_ready(rig.now));
    assert_eq!(rig.counters().dropped_send_failed, 1);
    assert_eq!(rig.counters().dropped_send_disconnected, 1);
    assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(100)));
    rig.advance(ms(99));
    assert_eq!(rig.take_actions(), []);
    rig.advance(ms(1));
    assert_eq!(
        rig.take_actions(),
        [
            TransportAction::CloseSession,
            TransportAction::Finish(TransportOutcome::Stopped)
        ]
    );
}

#[test]
fn the_role_gets_no_input_after_a_stop() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.receive(&wire_bytes(&sdu(1, 3)));
    rig.receive(&wire_bytes(&sdu(2, 3)));
    rig.core.bytes_read(&wire_bytes(&sdu(3, 3)));
    rig.stop();
    // Counted at once, not taken.
    assert_eq!(rig.counters().dropped_receive_stopped, 2);
    assert!(!rig.core.received_ready());
    assert_eq!(rig.core.take_received(), None);
    // The data socket is no longer read.
    assert!(!rig.core.read_ready());
    rig.core.bytes_read(&wire_bytes(&sdu(4, 3)));
    rig.core.serve_read(rig.now);
    assert_eq!(rig.counters().sdus_received, 2);
    // A read error is not a session end now.
    rig.read_ended();
    assert_eq!(rig.take_actions(), []);
    // Later sends are dropped and counted.
    rig.send_now(sdu(5, 1));
    assert_eq!(rig.counters().dropped_send_disconnected, 1);
    assert!(!rig.core.write_ready(rig.now));
}

#[test]
fn a_stalled_stop_is_bounded_by_the_limit_not_the_stall_time() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    rig.core.write_blocked();
    rig.advance(ms(900));
    rig.advance(ms(100));
    assert_eq!(
        rig.take_actions(),
        [
            TransportAction::CloseSession,
            TransportAction::Finish(TransportOutcome::Stopped)
        ]
    );
    assert_eq!(rig.counters().dropped_send_disconnected, 2);
}

#[test]
fn a_stop_while_waiting_to_reconnect_finishes_at_once() {
    let mut rig = Rig::easymesh();
    // Before the first connect.
    rig.stop();
    assert_eq!(
        rig.take_actions(),
        [TransportAction::Finish(TransportOutcome::Stopped)]
    );

    let mut rig = Rig::easymesh();
    rig.register();
    rig.read_ended();
    rig.take_actions();
    rig.advance(ms(500));
    rig.stop();
    assert_eq!(
        rig.take_actions(),
        [TransportAction::Finish(TransportOutcome::Stopped)]
    );
    assert_eq!(rig.core.next_deadline(), None);
}

#[test]
fn a_stop_while_registering_closes_the_attempt_and_finishes_at_once() {
    let mut rig = Rig::easymesh();
    rig.connect();
    rig.stop();
    assert_eq!(
        rig.take_actions(),
        [
            TransportAction::CloseSession,
            TransportAction::Finish(TransportOutcome::Stopped)
        ]
    );
}

#[test]
fn a_stop_without_a_session_counts_the_received_queue() {
    let mut rig = Rig::easymesh();
    rig.register();
    rig.receive(&wire_bytes(&sdu(1, 3)));
    rig.read_ended();
    rig.take_actions();
    rig.stop();
    assert_eq!(rig.counters().dropped_receive_stopped, 1);
    assert_eq!(rig.counters().sdus_received, 1);
}

#[test]
fn a_second_stop_changes_nothing() {
    let mut rig = Rig::easymesh();
    stopping_with_two_sdus(&mut rig);
    let deadline = rig.core.next_deadline();
    rig.stop();
    assert_eq!(rig.take_actions(), []);
    assert_eq!(rig.core.next_deadline(), deadline);
}

#[test]
fn every_input_after_finish_is_ignored() {
    let mut rig = Rig::easymesh();
    rig.connect();
    rig.stop();
    assert_eq!(rig.take_actions().len(), 2);
    let before = rig.counters();

    rig.tick();
    rig.advance(ms(100_000));
    rig.registration_frame(response(LOCAL, 1));
    rig.registration_failed(RegistrationStep::ConnectControl, ErrorKind::NotFound);
    rig.core.bytes_read(&wire_bytes(&sdu(1, 3)));
    rig.core.serve_read(rig.now);
    rig.read_ended();
    rig.core.wrote(rig.now, 1);
    rig.core.write_blocked();
    rig.core.writable();
    rig.write_failed();
    rig.stop();
    assert_eq!(rig.take_actions(), []);
    assert_eq!(rig.counters(), before);
    assert_eq!(rig.core.next_deadline(), None);
    assert!(!rig.core.read_ready());
    assert!(!rig.core.write_ready(rig.now));
    assert!(!rig.core.received_ready());
    assert_eq!(rig.core.write_chunk(rig.now), None);
    assert_eq!(rig.core.take_received(), None);

    // A send is not written; it is counted so that every send is counted once.
    rig.send(0, sdu(1, 1));
    assert_eq!(
        rig.counters().dropped_send_disconnected,
        before.dropped_send_disconnected + 1
    );
    assert_eq!(rig.take_actions(), []);
}

// ----- the balances in one scenario -----

#[test]
fn the_counts_balance_over_a_session_with_a_stop() {
    let mut rig = Rig::easymesh();
    rig.register();
    for tag in 0..6 {
        rig.receive(&wire_bytes(&sdu(tag, 2)));
    }
    assert!(rig.core.take_received().is_some());
    rig.send_now(sdu(1, 5));
    rig.send_now(sdu(2, 5));
    rig.send(0, sdu(3, 5));
    rig.core.wrote(rig.now, 4);
    rig.stop();
    rig.send_now(sdu(4, 5));
    rig.advance(ms(900));
    rig.advance(ms(100));
    assert_eq!(rig.take_actions().len(), 2);

    let counters = rig.counters();
    assert_eq!(
        counters.sdus_received,
        1 + counters.dropped_receive_overflow + counters.dropped_receive_stopped
    );
    assert_eq!(counters.dropped_receive_stopped, 5);
    assert_eq!(
        4,
        counters.sdus_sent
            + counters.dropped_send_failed
            + counters.dropped_send_disconnected
            + counters.dropped_send_overflow
    );
}

// ----- seeded random test -----

/// Small limits, so that every limit is reached often.
fn random_policy() -> TransportPolicy {
    TransportPolicy {
        reconnect_delay: ms(200),
        rx_queue_sdus: 4,
        rx_queue_bytes: 120,
        rx_sdu_cap: 60,
        tx_buffer_bytes: 400,
        write_stall: ms(300),
        stop_write_limit: ms(90),
        linger: ms(10),
    }
}

/// What the test knows from outside the core.
#[derive(Default)]
struct Model {
    sends: u64,
    takes: u64,
    /// Sockets are open: between `Connect` and `CloseSession`.
    open: bool,
    finish_count: u32,
    outcome: Option<TransportOutcome>,
}

/// One chunk of bytes the data socket may deliver.
fn random_bytes(rng: &mut Prng) -> Vec<u8> {
    let length = rng.below(40);
    let payload: Vec<u8> = (0..length)
        .map(|index| u8::try_from(index).unwrap())
        .collect();
    match rng.below(10) {
        0..=3 => wire_bytes(&sdu(u8::try_from(rng.below(250)).unwrap(), length)),
        // A fragment: first, middle, last, with random ids and flags.
        4..=6 => {
            let is_fragment = u8::from(rng.below(5) != 0);
            let is_last = u8::from(rng.below(3) == 0);
            let id = u8::try_from(rng.below(4)).unwrap();
            raw_frame(is_fragment, is_last, id, &payload)
        }
        7 => (0..rng.below(20))
            .map(|_| u8::try_from(rng.next() & 0xff).unwrap())
            .collect(),
        8 => {
            // A frame too short for the header.
            let mut bytes = u32::try_from(length % 15).unwrap().to_be_bytes().to_vec();
            bytes.extend((0..length % 15).map(|_| 1));
            bytes
        }
        _ => {
            // An oversize frame, rarely, so that the run stays fast.
            if rng.below(8) == 0 {
                let mut bytes = 65_537u32.to_be_bytes().to_vec();
                bytes.extend(std::iter::repeat_n(0u8, 65_537));
                bytes
            } else {
                raw_frame(0, 1, 0, &payload)
            }
        }
    }
}

fn random_registration_frame(rng: &mut Prng) -> Frame {
    match rng.below(10) {
        0 => Frame::Oversize { length: 70_000 },
        1 => Frame::Complete(vec![1, 2, 3]),
        2 => response(MacAddr::new([0; 6]), 1),
        3 => response(LOCAL, 2),
        _ => response(LOCAL, 1),
    }
}

const STEPS: [RegistrationStep; 4] = [
    RegistrationStep::ConnectControl,
    RegistrationStep::ConnectData,
    RegistrationStep::SendRequest,
    RegistrationStep::ReadResponse,
];
const KINDS: [ErrorKind; 8] = [
    ErrorKind::NotFound,
    ErrorKind::ConnectionRefused,
    ErrorKind::UnexpectedEof,
    ErrorKind::ConnectionReset,
    ErrorKind::BrokenPipe,
    ErrorKind::ConnectionAborted,
    ErrorKind::PermissionDenied,
    ErrorKind::Other,
];

/// Checks the actions of one step against the sockets the model believes are open.
fn absorb_actions(rig: &mut Rig, model: &mut Model, seed: u64) {
    for action in rig.take_actions() {
        assert_eq!(model.finish_count, 0, "seed {seed}: action after Finish");
        match action {
            TransportAction::Connect => {
                assert!(!model.open, "seed {seed}: Connect with sockets open");
                assert_eq!(rig.core.state, State::Registering, "seed {seed}");
                model.open = true;
            }
            TransportAction::CloseSession => {
                assert!(model.open, "seed {seed}: CloseSession with no sockets");
                model.open = false;
            }
            TransportAction::Finish(outcome) => {
                assert!(!model.open, "seed {seed}: Finish with sockets open");
                model.finish_count += 1;
                model.outcome = Some(outcome);
            }
        }
    }
}

fn check_invariants(rig: &Rig, model: &Model, policy: TransportPolicy, seed: u64) {
    let core = &rig.core;
    let counters = core.counters();
    let finished = core.state == State::Finished;
    assert_eq!(finished, model.finish_count == 1, "seed {seed}");
    let sockets_expected = !matches!(core.state, State::Disconnected { .. } | State::Finished);
    assert_eq!(
        model.open, sockets_expected,
        "seed {seed}: {:?}",
        core.state
    );
    if core.link().al_mac.is_some() {
        assert!(
            matches!(
                core.state,
                State::InSession | State::StopWriting { .. } | State::StopLingering { .. }
            ),
            "seed {seed}: AL MAC with state {:?}",
            core.state
        );
    }
    assert_eq!(core.link().session, counters.registrations, "seed {seed}");

    // Limits.
    assert!(
        core.outbound.byte_count() <= policy.tx_buffer_bytes,
        "seed {seed}"
    );
    assert!(core.received.len() <= policy.rx_queue_sdus, "seed {seed}");
    assert!(
        core.received.byte_count() <= policy.rx_queue_bytes,
        "seed {seed}"
    );

    // Readiness.
    if core.read_ready() {
        assert_eq!(core.state, State::InSession, "seed {seed}");
    }
    if core.write_ready(rig.now) {
        assert!(core.write_chunk(rig.now).is_some(), "seed {seed}");
    }
    assert_eq!(
        core.received_ready(),
        !core.received.is_empty(),
        "seed {seed}"
    );

    // The receive balance holds at every step: stopped is counted at the stop.
    assert_eq!(
        counters.sdus_received,
        model.takes
            + counters.dropped_receive_overflow
            + counters.dropped_receive_stopped
            + core.received.len() as u64,
        "seed {seed}"
    );

    if finished {
        assert_eq!(core.next_deadline(), None, "seed {seed}");
        assert!(!core.read_ready() && !core.write_ready(rig.now) && !core.received_ready());
        assert!(core.outbound.is_empty(), "seed {seed}");
        // The send balance: every send is counted exactly once.
        assert_eq!(
            model.sends,
            counters.sdus_sent
                + counters.dropped_send_failed
                + counters.dropped_send_disconnected
                + counters.dropped_send_overflow,
            "seed {seed}"
        );
        assert_eq!(
            counters.sdus_received,
            model.takes + counters.dropped_receive_overflow + counters.dropped_receive_stopped,
            "seed {seed}"
        );
    }
}

const STEPS_PER_SEED: usize = 200;

/// Runs one seed; returns the outcome if the core finished.
fn run_random_seed(seed: u64) -> (Option<TransportOutcome>, u64) {
    let mut dice = Prng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let policy = random_policy();
    let mut rig = Rig::with_policy(policy);
    let mut model = Model::default();

    for _ in 0..STEPS_PER_SEED {
        let step = dice.below(100);
        match step {
            // Time passes; the tick leaves no deadline in the past.
            0..=14 => {
                rig.now += ms(dice.below(120) as u64);
                rig.tick();
                let deadline = rig.core.next_deadline();
                assert!(deadline.is_none_or(|due| due > rig.now), "seed {seed}");
            }
            15..=22 => {
                if let Some(due) = rig.core.next_deadline() {
                    rig.now = rig.now.max(due);
                }
                rig.tick();
                let deadline = rig.core.next_deadline();
                assert!(deadline.is_none_or(|due| due > rig.now), "seed {seed}");
            }
            23..=30 => rig.registration_frame(random_registration_frame(&mut dice)),
            31..=34 => {
                let step = STEPS[dice.below(STEPS.len())];
                let kind = KINDS[dice.below(KINDS.len())];
                rig.registration_failed(step, kind);
            }
            35..=46 => {
                let bytes = random_bytes(&mut dice);
                // Delivered whole or in two pieces.
                if bytes.len() > 1 && dice.below(2) == 0 {
                    let cut = 1 + dice.below(bytes.len() - 1);
                    rig.core.bytes_read(&bytes[..cut]);
                    rig.core.bytes_read(&bytes[cut..]);
                } else {
                    rig.core.bytes_read(&bytes);
                }
            }
            47..=56 => {
                for _ in 0..=dice.below(3) {
                    if rig.core.read_ready() {
                        rig.core.serve_read(rig.now);
                    }
                }
            }
            57..=62 => {
                let ready = rig.core.received_ready();
                let taken = rig.core.take_received();
                assert_eq!(ready, taken.is_some(), "seed {seed}");
                if taken.is_some() {
                    model.takes += 1;
                }
            }
            63..=74 => {
                let current = rig.core.link().session;
                let session = match dice.below(4) {
                    0 => current.saturating_sub(1),
                    1 => current + 1,
                    _ => current,
                };
                rig.send(session, sdu(1, dice.below(70)));
                model.sends += 1;
            }
            75..=88 => {
                if let Some(length) = rig.core.write_chunk(rig.now).map(<[u8]>::len) {
                    if rig.core.write_ready(rig.now) || dice.below(4) == 0 {
                        let written = if dice.below(3) == 0 {
                            length
                        } else {
                            1 + dice.below(length)
                        };
                        rig.core.wrote(rig.now, written);
                    }
                } else if rig.core.queued_chunk().is_none() {
                    // Nothing queued: a stray result is ignored.
                    rig.core.wrote(rig.now, 1 + dice.below(5));
                }
            }
            89 => rig.core.write_blocked(),
            91..=92 => rig.read_ended(),
            93..=94 => rig.write_failed(),
            95 => {
                if dice.below(3) == 0 {
                    rig.stop();
                }
            }
            _ => rig.core.writable(),
        }
        absorb_actions(&mut rig, &mut model, seed);
        check_invariants(&rig, &model, policy, seed);
    }
    (model.outcome, rig.core.counters().registrations)
}

#[test]
fn random_inputs_keep_the_invariants() {
    let mut stopped = 0;
    let mut rejected = 0;
    let mut io_failed = 0;
    let mut unfinished = 0;
    let mut reconnected = 0;
    for seed in 1..=400 {
        let (outcome, registrations) = run_random_seed(seed);
        match outcome {
            Some(TransportOutcome::Stopped) => stopped += 1,
            Some(TransportOutcome::Rejected { .. }) => rejected += 1,
            Some(TransportOutcome::IoFailed { .. }) => io_failed += 1,
            None => unfinished += 1,
        }
        if registrations > 1 {
            reconnected += 1;
        }
    }
    // The generator reaches every ending and every reconnect, or the test proves little.
    assert!(stopped > 20, "stopped {stopped}");
    assert!(rejected > 20, "rejected {rejected}");
    assert!(io_failed > 5, "io_failed {io_failed}");
    assert!(unfinished > 20, "unfinished {unfinished}");
    assert!(reconnected > 100, "reconnected {reconnected}");
}

// ----- review fixes (0016 D8 step 4b-2b) -----

#[test]
fn a_complete_registration_body_above_the_frame_limit_is_retried() {
    // The actor's FrameReader never yields such a body; the core must not accept it.
    let mut rig = Rig::easymesh();
    rig.connect();
    let mut body = LOCAL.octets().to_vec();
    body.push(1);
    body.resize(65_537, 0);
    rig.registration_frame(Frame::Complete(body));
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    assert_eq!(rig.counters().registrations, 0);
    assert_eq!(rig.core.link(), LinkState::default());
    assert_eq!(rig.core.next_deadline(), Some(rig.now + ms(2000)));
}

#[test]
fn a_complete_registration_body_of_exactly_the_frame_limit_is_accepted() {
    let mut rig = Rig::easymesh();
    rig.connect();
    let mut body = LOCAL.octets().to_vec();
    body.push(1);
    body.resize(65_536, 0);
    rig.registration_frame(Frame::Complete(body));
    assert_eq!(rig.counters().registrations, 1);
}

#[test]
fn buffered_frames_are_handled_before_the_end_of_the_stream_ends_the_session() {
    let mut rig = Rig::easymesh();
    rig.register();
    let mut bytes = wire_bytes(&sdu(1, 10));
    bytes.extend_from_slice(&wire_bytes(&sdu(2, 10)));
    rig.core.bytes_read(&bytes);
    rig.core.serve_read(rig.now);
    assert!(rig.core.read_ready());
    rig.read_ended();
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    assert_eq!(rig.counters().sdus_received, 2);
    assert_eq!(rig.take_all_received(), [sdu(1, 10), sdu(2, 10)]);
    assert_eq!(rig.core.link().al_mac, None);
}

#[test]
fn the_end_of_the_stream_applies_the_receive_rules_to_buffered_frames() {
    let mut rig = Rig::easymesh();
    rig.register();
    // A frame shorter than an SDU header, then a good one.
    let mut bytes = 3_u32.to_be_bytes().to_vec();
    bytes.extend_from_slice(&[1, 2, 3]);
    bytes.extend_from_slice(&wire_bytes(&sdu(3, 10)));
    rig.core.bytes_read(&bytes);
    rig.read_ended();
    assert_eq!(rig.counters().dropped_receive_malformed, 1);
    assert_eq!(rig.counters().sdus_received, 1);
    assert_eq!(rig.take_all_received(), [sdu(3, 10)]);
}

fn never_policy() -> TransportPolicy {
    TransportPolicy {
        reconnect_delay: Duration::MAX,
        write_stall: Duration::MAX,
        stop_write_limit: Duration::MAX,
        linger: Duration::MAX,
        ..TransportPolicy::easymesh()
    }
}

#[test]
fn a_deadline_that_does_not_fit_is_never_and_does_not_panic() {
    // Retry: the reconnect never comes.
    let mut rig = Rig::with_policy(never_policy());
    rig.connect();
    rig.registration_failed(RegistrationStep::ConnectControl, ErrorKind::NotFound);
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    assert_eq!(rig.core.next_deadline(), None);
    rig.advance(ms(10_000));
    assert_eq!(rig.take_actions(), []);

    // Stall: queued bytes have no stall deadline.
    let mut rig = Rig::with_policy(never_policy());
    rig.register();
    rig.send_now(sdu(1, 10));
    assert_eq!(rig.core.next_deadline(), None);
    rig.advance(ms(10_000));
    assert_eq!(rig.take_actions(), []);

    // Session end by a failed write: the reconnect never comes.
    rig.write_failed();
    assert_eq!(rig.take_actions(), [TransportAction::CloseSession]);
    assert_eq!(rig.core.next_deadline(), None);

    // Stop with bytes queued: no write limit; finishing the writes starts a linger that never ends.
    let mut rig = Rig::with_policy(never_policy());
    rig.register();
    rig.send_now(sdu(1, 10));
    rig.stop();
    assert_eq!(rig.core.next_deadline(), None);
    rig.advance(ms(10_000));
    assert!(rig.core.write_ready(rig.now));
    rig.write_all();
    assert_eq!(rig.counters().sdus_sent, 1);
    assert_eq!(rig.core.next_deadline(), None);
    rig.advance(ms(10_000));
    assert_eq!(rig.take_actions(), []);

    // Stop with an empty buffer, and a failed write while stopping.
    let mut rig = Rig::with_policy(never_policy());
    rig.register();
    rig.stop();
    assert_eq!(rig.core.next_deadline(), None);
    let mut rig = Rig::with_policy(never_policy());
    rig.register();
    rig.send_now(sdu(1, 10));
    rig.stop();
    rig.write_failed();
    assert_eq!(rig.core.next_deadline(), None);
}
