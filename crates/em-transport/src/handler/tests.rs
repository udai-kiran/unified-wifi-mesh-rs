// SPDX-License-Identifier: Apache-2.0

//! Tests of `TransportHandler` with a real `LoopCore` and no sockets: the test plays the `Io` by
//! calling `on_event` with what the sockets would report.

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use em_event_loop::{HandlerId, LoopCore, SourceId, Turn};
use em_types::MacAddr;

use super::{
    RegistrationStep, TransportEvent, TransportHandler, TransportIoAction, TransportOutcome,
    TransportPolicy, TransportSources,
};
use crate::frame_splitter::Frame;
use crate::wire::encode_sdu_frames;
use crate::{OutboundSdu, Sdu};

const LOCAL: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 0xaa]);
const REMOTE: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 0xbb]);

const SOURCES: TransportSources = TransportSources {
    read: SourceId(0),
    registration: SourceId(1),
    write: SourceId(2),
    timer: HandlerId(0),
};

fn ms(count: u64) -> Duration {
    Duration::from_millis(count)
}

fn sdu(tag: u8) -> Sdu {
    Sdu {
        local: LOCAL,
        remote: REMOTE,
        cmdu: vec![tag; 4],
    }
}

fn wire_bytes(sdu: &Sdu) -> Vec<u8> {
    encode_sdu_frames(sdu).unwrap().concat()
}

fn response(result: u8) -> Frame {
    let mut body = LOCAL.octets().to_vec();
    body.push(result);
    Frame::Complete(body)
}

/// A handler, the loop it runs on and the clock the test moves. Every source has budget 1, so one
/// pass is one turn per source and a test sees each step of the read source.
struct Rig {
    loop_core: LoopCore<Instant>,
    handler: TransportHandler,
    now: Instant,
}

impl Rig {
    fn new() -> Self {
        Self::with_policy(TransportPolicy::easymesh())
    }

    fn with_policy(policy: TransportPolicy) -> Self {
        let now = Instant::now();
        let mut loop_core = LoopCore::new(&[1, 1, 1], 1);
        let handler = TransportHandler::new(policy, now, SOURCES, &mut loop_core);
        Self {
            loop_core,
            handler,
            now,
        }
    }

    /// One pass at the current time; returns the actions it produced.
    fn pass(&mut self) -> Vec<TransportIoAction> {
        self.loop_core.begin_pass(self.now);
        let mut actions = Vec::new();
        loop {
            let turn = self.loop_core.next_turn();
            if turn == Turn::PassDone {
                return actions;
            }
            let served = self
                .handler
                .serve(turn, self.now, &mut self.loop_core, &mut actions);
            assert!(served, "the handler refused its own turn {turn:?}");
        }
    }

    fn event(&mut self, event: TransportEvent) {
        self.handler.on_event(event, &mut self.loop_core);
    }

    fn advance(&mut self, by: Duration) {
        self.now += by;
    }

    /// Connects and registers. The read source is served before the registration source in a
    /// pass, so the first read is armed by the next pass.
    fn register(&mut self) {
        assert_eq!(self.pass(), [TransportIoAction::Connect]);
        self.event(TransportEvent::Registered(response(1)));
        assert_eq!(self.pass(), []);
        assert_eq!(self.handler.link().session, 1);
    }

    /// Registers and arms the first read.
    fn register_and_arm_read(&mut self) {
        self.register();
        assert_eq!(self.pass(), [TransportIoAction::Read]);
    }

    fn send(&mut self, sdu: Sdu) {
        let session = self.handler.link().session;
        self.handler
            .send(self.now, OutboundSdu { session, sdu }, &mut self.loop_core);
    }

    fn stop(&mut self) -> Vec<TransportIoAction> {
        let mut actions = Vec::new();
        self.handler
            .stop(self.now, &mut self.loop_core, &mut actions);
        actions
    }
}

#[test]
fn the_first_tick_connects() {
    let mut rig = Rig::new();
    assert_eq!(rig.pass(), [TransportIoAction::Connect]);
    assert_eq!(rig.pass(), []);
}

#[test]
fn a_registration_arms_one_read() {
    let mut rig = Rig::new();
    rig.register();
    assert_eq!(rig.pass(), [TransportIoAction::Read]);
}

#[test]
fn no_second_read_while_one_is_armed() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    assert_eq!(rig.pass(), []);
    assert_eq!(rig.pass(), []);
}

#[test]
fn bytes_with_a_partial_frame_arm_the_next_read() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    let bytes = wire_bytes(&sdu(1));
    let (first, rest) = bytes.split_at(bytes.len() - 3);

    rig.event(TransportEvent::Bytes(first.to_vec()));
    assert_eq!(rig.pass(), [TransportIoAction::Read]);
    assert_eq!(rig.handler.counters().sdus_received, 0);

    rig.event(TransportEvent::Bytes(rest.to_vec()));
    assert_eq!(rig.pass(), [TransportIoAction::Read]);
    assert_eq!(rig.handler.counters().sdus_received, 1);
    assert!(rig.handler.received_ready());
}

#[test]
fn bytes_with_frames_arm_the_next_read_only_when_the_splitter_is_empty() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    let mut bytes = wire_bytes(&sdu(1));
    bytes.extend(wire_bytes(&sdu(2)));
    bytes.extend(wire_bytes(&sdu(3)));

    rig.event(TransportEvent::Bytes(bytes));
    // One frame per pass (budget 1); the read is armed after the third.
    assert_eq!(rig.pass(), []);
    assert_eq!(rig.pass(), []);
    assert_eq!(rig.pass(), [TransportIoAction::Read]);
    assert_eq!(rig.handler.counters().sdus_received, 3);
    assert_eq!(rig.pass(), []);
}

#[test]
fn at_most_one_write_is_in_flight() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    rig.send(sdu(1));
    rig.send(sdu(2));

    let expected = TransportIoAction::Write(wire_bytes(&sdu(1)));
    assert_eq!(rig.pass(), [expected]);
    assert_eq!(rig.pass(), []);

    // The result frees the slot, and the next pass writes the second SDU.
    rig.event(TransportEvent::Wrote(wire_bytes(&sdu(1)).len()));
    assert_eq!(rig.pass(), [TransportIoAction::Write(wire_bytes(&sdu(2)))]);
    assert_eq!(rig.handler.counters().sdus_sent, 1);
}

#[test]
fn a_partial_write_leaves_the_rest_for_the_next_write() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    rig.send(sdu(1));
    let bytes = wire_bytes(&sdu(1));
    assert_eq!(rig.pass(), [TransportIoAction::Write(wire_bytes(&sdu(1)))]);

    rig.event(TransportEvent::Wrote(5));
    assert_eq!(rig.pass(), [TransportIoAction::Write(bytes[5..].to_vec())]);
}

#[test]
fn a_send_in_the_pass_of_a_write_does_not_evict_the_sdu_being_written() {
    let policy = TransportPolicy {
        tx_buffer_bytes: 50,
        ..TransportPolicy::easymesh()
    };
    let mut rig = Rig::with_policy(policy);
    rig.register_and_arm_read();
    let long = |tag| Sdu {
        cmdu: vec![tag; 8],
        ..sdu(tag)
    };
    // 23 and 27 octets stored: both fit.
    rig.send(sdu(1));
    rig.send(long(2));
    let first = wire_bytes(&sdu(1));
    assert_eq!(rig.pass(), [TransportIoAction::Write(wire_bytes(&sdu(1)))]);

    // A later turn of the same pass, or the next one, sends before the result arrives.
    rig.send(sdu(3));
    assert_eq!(rig.handler.counters().dropped_send_overflow, 1);

    rig.event(TransportEvent::Wrote(first.len()));
    assert_eq!(rig.handler.counters().sdus_sent, 1);
    assert_eq!(rig.pass(), [TransportIoAction::Write(wire_bytes(&sdu(3)))]);
}

#[test]
fn a_blocked_write_waits_for_writable() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    rig.send(sdu(1));
    assert_eq!(rig.pass().len(), 1);

    rig.event(TransportEvent::WriteBlocked);
    assert_eq!(rig.pass(), []);

    rig.event(TransportEvent::Writable);
    assert_eq!(rig.pass(), [TransportIoAction::Write(wire_bytes(&sdu(1)))]);
}

#[test]
fn wrote_is_judged_at_the_time_the_write_was_issued() {
    // 0016 U4: queued bytes that make no progress for 5 s end the session. The write was issued
    // at t0 and completed in part; the result arrives between passes, after the clock has moved
    // across the stall deadline. The remaining bytes' clock restarts at t0, so the deadline
    // (t0 + 5 s) is due at the next pass. With the later time it would restart at the pass time
    // and the session would survive.
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    let issued_at = rig.now;
    rig.send(sdu(1));
    assert_eq!(rig.pass().len(), 1);

    rig.advance(Duration::from_secs(6));
    rig.event(TransportEvent::Wrote(5));

    assert_eq!(
        rig.loop_core.next_deadline(),
        Some(issued_at + Duration::from_secs(5))
    );
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
}

#[test]
fn close_session_forgets_the_armed_read_and_the_write_in_flight() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    rig.send(sdu(1));
    assert_eq!(rig.pass().len(), 1);

    // No progress: the stall ends the session while the read is armed and the write in flight.
    rig.advance(Duration::from_secs(5));
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);

    rig.advance(Duration::from_secs(2));
    assert_eq!(rig.pass(), [TransportIoAction::Connect]);
    rig.event(TransportEvent::Registered(response(1)));
    assert_eq!(rig.pass(), []);
    assert_eq!(rig.handler.link().session, 2);
    // A new read is armed, and the new session's SDU is written.
    assert_eq!(rig.pass(), [TransportIoAction::Read]);
    rig.send(sdu(2));
    assert_eq!(rig.pass(), [TransportIoAction::Write(wire_bytes(&sdu(2)))]);
}

#[test]
fn close_session_forgets_stored_socket_events() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    rig.send(sdu(1));
    assert_eq!(rig.pass().len(), 1);

    // Both are stored; the read turn ends the session first, and the write failure of the
    // closed session is void.
    rig.event(TransportEvent::WriteFailed);
    rig.event(TransportEvent::ReadEnded);
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
    assert_eq!(rig.pass(), []);

    rig.advance(Duration::from_secs(2));
    assert_eq!(rig.pass(), [TransportIoAction::Connect]);
    rig.event(TransportEvent::Registered(response(1)));
    assert_eq!(rig.pass(), []);
    assert_eq!(rig.pass(), [TransportIoAction::Read]);
    assert_eq!(rig.pass(), []);
}

#[test]
fn read_ended_is_handled_on_the_read_turn() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();

    rig.event(TransportEvent::ReadEnded);
    // Reported, not handled: the link stays until the read turn.
    assert!(rig.handler.link().al_mac.is_some());
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
    assert!(rig.handler.link().al_mac.is_none());
}

#[test]
fn write_failed_is_handled_on_the_write_turn() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    rig.send(sdu(1));
    assert_eq!(rig.pass().len(), 1);

    rig.event(TransportEvent::WriteFailed);
    assert!(rig.handler.link().al_mac.is_some());
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
    assert!(rig.handler.link().al_mac.is_none());
    assert_eq!(rig.handler.counters().dropped_send_disconnected, 1);
}

#[test]
fn a_rejected_registration_finishes_with_an_outcome() {
    let mut rig = Rig::new();
    assert_eq!(rig.pass(), [TransportIoAction::Connect]);
    assert_eq!(rig.handler.outcome(), None);

    rig.event(TransportEvent::Registered(response(0)));
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
    assert_eq!(
        rig.handler.outcome(),
        Some(TransportOutcome::Rejected { result: 0 })
    );
}

#[test]
fn a_fatal_registration_error_finishes_with_an_io_outcome() {
    let mut rig = Rig::new();
    assert_eq!(rig.pass(), [TransportIoAction::Connect]);

    rig.event(TransportEvent::RegistrationFailed {
        step: RegistrationStep::ConnectData,
        kind: ErrorKind::PermissionDenied,
    });
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
    assert_eq!(
        rig.handler.outcome(),
        Some(TransportOutcome::IoFailed {
            operation: "connect data socket"
        })
    );
}

#[test]
fn a_retried_registration_connects_again_after_the_delay() {
    let mut rig = Rig::new();
    assert_eq!(rig.pass(), [TransportIoAction::Connect]);

    rig.event(TransportEvent::RegistrationFailed {
        step: RegistrationStep::ConnectControl,
        kind: ErrorKind::NotFound,
    });
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
    assert_eq!(rig.handler.outcome(), None);

    rig.advance(Duration::from_secs(2));
    assert_eq!(rig.pass(), [TransportIoAction::Connect]);
}

#[test]
fn a_stop_in_a_session_writes_lingers_and_finishes() {
    let mut rig = Rig::new();
    rig.register_and_arm_read();
    rig.send(sdu(1));

    assert_eq!(rig.stop(), []);
    assert_eq!(rig.pass(), [TransportIoAction::Write(wire_bytes(&sdu(1)))]);
    rig.event(TransportEvent::Wrote(wire_bytes(&sdu(1)).len()));
    assert_eq!(rig.pass(), []);
    assert_eq!(rig.handler.outcome(), None);

    rig.advance(ms(100));
    assert_eq!(rig.pass(), [TransportIoAction::CloseSession]);
    assert_eq!(rig.handler.outcome(), Some(TransportOutcome::Stopped));
    assert_eq!(rig.handler.counters().sdus_sent, 1);
}

#[test]
fn a_stop_while_disconnected_finishes_at_once() {
    let mut rig = Rig::new();
    assert_eq!(rig.stop(), []);
    assert_eq!(rig.handler.outcome(), Some(TransportOutcome::Stopped));
}

#[test]
fn a_foreign_turn_is_not_served() {
    let mut rig = Rig::new();
    let mut actions: Vec<TransportIoAction> = Vec::new();
    for turn in [
        Turn::Serve(SourceId(7)),
        Turn::Deadline(HandlerId(7)),
        Turn::PassDone,
    ] {
        assert!(
            !rig.handler
                .serve(turn, rig.now, &mut rig.loop_core, &mut actions)
        );
    }
    assert_eq!(actions, []);
}
