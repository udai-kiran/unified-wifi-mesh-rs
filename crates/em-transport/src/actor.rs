// SPDX-License-Identifier: Apache-2.0

//! The transport actor: registration, the session loop and the reconnect loop (0011 D2, D3).

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::pin;
use std::time::Duration;

use em_types::MacAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::time::{Instant, sleep, sleep_until, timeout_at};

use crate::frame_reader::FrameReader;
use crate::frame_splitter::Frame;
use crate::policy;
use crate::wire::{
    self, RESULT_SUCCESS, Reassembler, ReassemblyStep, decode_registration_response,
    decode_sdu_frame, encode_registration_request, encode_sdu_frames,
};
use crate::{LinkState, OutboundSdu, Sdu, TransportConfig, TransportCounters, TransportError};

/// Room in the channel from the role actor to this one.
pub(crate) const SDU_CHANNEL_CAPACITY: usize = 64;

/// Room in the channel to the role actor.
pub(crate) const RECEIVED_CHANNEL_CAPACITY: usize = 64;

/// Received SDUs kept inside the actor while the received channel is full. When the queue
/// is full the oldest one is dropped (0011 D2, user decision; A5).
pub(crate) const RECEIVED_QUEUE_CAPACITY: usize = 256;

/// How long the actor stays in a session after the role actor has gone, so that the last
/// writes leave (0011, "SDUs just before a close can be lost"; the stop path, A5, A6).
const LINGER: Duration = policy::LINGER;

/// Applies `change` to the counters and tells the watchers.
fn count(counters: &watch::Sender<TransportCounters>, change: impl FnOnce(&mut TransportCounters)) {
    counters.send_modify(change);
}

/// Adds `sdu` to the queue. Returns true if the oldest SDU was dropped to make room.
fn push_drop_oldest(queue: &mut VecDeque<Sdu>, sdu: Sdu, capacity: usize) -> bool {
    let dropped = queue.len() >= capacity;
    if dropped {
        queue.pop_front();
    }
    queue.push_back(sdu);
    dropped
}

/// Result of waiting while not registered.
enum Waited<T> {
    Finished(T),
    /// The role actor dropped its sender or the received receiver: the actor should end.
    /// The send channel is already closed, and `discard_sends` has counted what it held (A6).
    Stop,
}

/// How the session loop ended.
enum SessionExit {
    /// The role actor dropped its sender or the received receiver.
    RoleActorGone,
    /// A read on the data socket failed or reached end of file.
    ReadFailed,
    /// A write on the data socket failed. The stream may be out of step (0011 A7).
    WriteFailed,
}

/// How a session ended.
enum SessionEnd {
    /// The data socket closed or failed and the role actor is still there.
    Disconnected,
    /// The role actor has gone. The actor has already dealt with the queued SDUs (written them,
    /// or counted them after a failed write) and lingered.
    Stop,
}

/// Why a registration attempt did not finish.
enum RegisterFailure {
    /// Expected while the service restarts or another client holds the session.
    Retry(&'static str),
    Fatal(TransportError),
}

/// A finished registration: the two sockets stay open for the session.
struct Registration {
    al_mac: MacAddr,
    /// Never read again after the response, never written again; closing it ends the
    /// registration on this side.
    _control: FrameReader<UnixStream>,
    data: UnixStream,
}

struct Actor {
    sdus: mpsc::Receiver<OutboundSdu>,
    received: mpsc::Sender<Sdu>,
    /// Received SDUs waiting for room in `received`, oldest first.
    queue: VecDeque<Sdu>,
    link: watch::Sender<LinkState>,
    counters: watch::Sender<TransportCounters>,
}

/// Runs until the role actor drops its sender or the received receiver, the registration
/// is rejected, or a socket error that a reconnect cannot fix occurs.
pub(crate) async fn run(
    config: TransportConfig,
    sdus: mpsc::Receiver<OutboundSdu>,
    received: mpsc::Sender<Sdu>,
    link: watch::Sender<LinkState>,
    counters: watch::Sender<TransportCounters>,
) -> Result<(), TransportError> {
    let mut actor = Actor {
        sdus,
        received,
        queue: VecDeque::new(),
        link,
        counters,
    };
    let result = actor.run_until_end(&config).await;

    // The one place every return of the actor passes, fatal errors included (0011 A6).
    // Aborting the task skips it.
    let stopped = actor.queue.len() as u64;
    if stopped > 0 {
        count(&actor.counters, |c| c.dropped_receive_stopped += stopped);
    }
    result
}

impl Actor {
    async fn run_until_end(&mut self, config: &TransportConfig) -> Result<(), TransportError> {
        loop {
            let attempt = match self.wait_while_disconnected(register(config)).await {
                Waited::Finished(attempt) => attempt,
                Waited::Stop => return Ok(()),
            };

            match attempt {
                Ok(registration) => match self.run_session(registration).await {
                    SessionEnd::Stop => return Ok(()),
                    SessionEnd::Disconnected => {}
                },
                Err(RegisterFailure::Retry(reason)) => {
                    tracing::debug!("registration did not complete, will retry: {reason}");
                }
                Err(RegisterFailure::Fatal(error)) => {
                    tracing::error!("registration failed: {error}");
                    self.discard_sends(false).await;
                    return Err(error);
                }
            }

            match self
                .wait_while_disconnected(sleep(config.reconnect_delay))
                .await
            {
                Waited::Finished(()) => {}
                Waited::Stop => return Ok(()),
            }
        }
    }

    /// Waits for `future` while not registered: forwards queued SDUs to the role actor and
    /// drops the SDUs the role actor sends, counting them (0011 D3 step 5).
    ///
    /// When the role actor has gone it stops at once, unless a send permit is outstanding:
    /// then it waits for the permit, at most [`LINGER`] (A6). Nothing is written while not
    /// registered, so there is no linger.
    async fn wait_while_disconnected<F: Future>(&mut self, future: F) -> Waited<F::Output> {
        let mut future = pin!(future);
        // The select's output borrows `self`, so the arms only decide and the work follows it.
        let output = loop {
            // Not biased: a role actor that always has an SDU ready must not keep
            // `future` from being polled.
            tokio::select! {
                permit = self.received.reserve(), if !self.queue.is_empty() => {
                    let Ok(permit) = permit else {
                        break None;
                    };
                    if let Some(sdu) = self.queue.pop_front() {
                        permit.send(sdu);
                    }
                }
                // Without this arm a dropped receiver is noticed only when an SDU is queued.
                () = self.received.closed() => break None,
                sdu = self.sdus.recv() => {
                    if sdu.is_none() {
                        break None;
                    }
                    self.count_sdu_dropped();
                }
                output = &mut future => break Some(output),
            }
        };
        // Stale SDUs need no drain at registration: they fail the session comparison when
        // dequeued (A6). On a stop the channel is closed and what is left is counted.
        if let Some(output) = output {
            return Waited::Finished(output);
        }
        self.discard_sends(false).await;
        Waited::Stop
    }

    fn count_sdu_dropped(&self) {
        tracing::debug!("SDU dropped: not written");
        count(&self.counters, |c| c.dropped_send_disconnected += 1);
    }

    /// Closes the send channel, so later sends fail at the caller, and counts every SDU
    /// that is left or still arrives through a permit held across the close, until
    /// `recv()` returns `None` or [`LINGER`] has passed (0011 A6). SDUs whose permit is
    /// still held at that time are lost uncounted. With `full_linger` the actor waits out
    /// the rest of [`LINGER`] even if nothing is left to wait for.
    async fn discard_sends(&mut self, full_linger: bool) {
        self.sdus.close();
        let deadline = Instant::now() + LINGER;
        let counters = &self.counters;
        let sdus = &mut self.sdus;
        let discard = async {
            while sdus.recv().await.is_some() {
                tracing::debug!("SDU dropped: the actor is stopping");
                count(counters, |c| c.dropped_send_disconnected += 1);
            }
        };
        // An error here only means the time ran out.
        let _ = timeout_at(deadline, discard).await;
        if full_linger {
            sleep_until(deadline).await;
        }
    }

    /// The role actor has gone while a session is open, or a socket failed and the role actor
    /// has gone too (A5, A6): closes the send channel, writes the SDUs that were in it and
    /// belong to this session, stays for [`LINGER`] with the sockets open so the service can
    /// forward them before it sees the close.
    ///
    /// With `may_write` false (after a failed write) nothing is written: the partial frame
    /// may have left the stream out of step, so the queued SDUs are only counted (0011 A7).
    ///
    /// The writes have no time limit (0011 A7): a peer that stays alive and stops reading
    /// can block a write for good. A caller that must bound a stop times out on `&mut task`
    /// and then calls `abort()`.
    async fn stop_in_session(
        &mut self,
        write_half: &mut OwnedWriteHalf,
        session: u64,
        may_write: bool,
    ) -> SessionEnd {
        self.sdus.close();
        // The cutoff (0011 A7): the batch is the length read just after `close()`. A send
        // finished before the read is in it; one in progress may fall on either side. A
        // permit that is used later sends into the linger, where its SDU is counted.
        let queued = self.sdus.len();
        let mut writing = may_write;
        for _ in 0..queued {
            let Ok(outbound) = self.sdus.try_recv() else {
                break;
            };
            if !writing || outbound.session != session {
                self.count_sdu_dropped();
                continue;
            }
            if let Err(error) = send_sdu(&self.counters, write_half, &outbound.sdu).await {
                tracing::warn!("write to the data socket failed while stopping: {error}");
                writing = false;
            }
        }

        self.discard_sends(true).await;
        SessionEnd::Stop
    }

    /// True when the role actor has dropped the received receiver or every sender.
    fn role_actor_gone(&self) -> bool {
        self.received.is_closed() || self.sdus.is_closed()
    }

    /// Runs one registered session until the data socket ends or the role actor leaves.
    async fn run_session(&mut self, registration: Registration) -> SessionEnd {
        let Registration {
            al_mac,
            _control,
            data,
        } = registration;
        tracing::info!("registered, local AL MAC {al_mac}");
        count(&self.counters, |c| c.registrations += 1);
        // Publishing here, before any frame is read, means that when the role actor
        // handles an SDU of this session the link already shows this session or a later one.
        let session = self.link.borrow().session + 1;
        self.link.send_replace(LinkState {
            session,
            al_mac: Some(al_mac),
        });

        let (read_half, mut write_half) = data.into_split();
        let mut reader: FrameReader<OwnedReadHalf> = FrameReader::new(read_half);
        let mut reassembler = Reassembler::new(policy::RX_SDU_CAP);

        // The select's output borrows `self`, so the arms only decide and the work follows it.
        let exit = loop {
            // Not biased: a continuously ready sender must not starve reads, nor a busy
            // service starve sends.
            tokio::select! {
                permit = self.received.reserve(), if !self.queue.is_empty() => {
                    let Ok(permit) = permit else {
                        break SessionExit::RoleActorGone;
                    };
                    if let Some(sdu) = self.queue.pop_front() {
                        permit.send(sdu);
                    }
                }
                () = self.received.closed() => break SessionExit::RoleActorGone,
                outbound = self.sdus.recv() => {
                    let Some(outbound) = outbound else {
                        break SessionExit::RoleActorGone;
                    };
                    // An SDU is written only into the session it was built for (A6).
                    if outbound.session != session {
                        tracing::debug!(
                            "SDU dropped: built for session {}, this is session {session}",
                            outbound.session
                        );
                        self.count_sdu_dropped();
                    } else if let Err(error) =
                        send_sdu(&self.counters, &mut write_half, &outbound.sdu).await
                    {
                        tracing::warn!("write to the data socket failed, ending the session: {error}");
                        break SessionExit::WriteFailed;
                    }
                }
                frame = reader.next_frame() => {
                    match frame {
                        Ok(frame) => handle_frame(&mut self.queue, &self.counters, &mut reassembler, frame),
                        Err(error) => {
                            tracing::info!("data socket ended: {error}");
                            break SessionExit::ReadFailed;
                        }
                    }
                }
            }
        };

        let end = match exit {
            // The socket failure may hide that the role actor has gone too (A6).
            SessionExit::ReadFailed | SessionExit::WriteFailed if !self.role_actor_gone() => {
                SessionEnd::Disconnected
            }
            SessionExit::RoleActorGone | SessionExit::ReadFailed => {
                self.stop_in_session(&mut write_half, session, true).await
            }
            SessionExit::WriteFailed => self.stop_in_session(&mut write_half, session, false).await,
        };

        // The same teardown for every exit, after the linger on a stop (A6).
        let abandoned = reassembler.abandon_set();
        count(&self.counters, |c| c.dropped_receive_fragments += abandoned);
        self.link.send_modify(|link| link.al_mac = None);
        end
    }
}

/// Writes one SDU as one or more frames.
async fn send_sdu(
    counters: &watch::Sender<TransportCounters>,
    writer: &mut OwnedWriteHalf,
    sdu: &Sdu,
) -> io::Result<()> {
    let Ok(frames) = encode_sdu_frames(sdu) else {
        tracing::error!(
            "SDU of {} octets needs more than 256 fragments, dropped",
            sdu.cmdu.len()
        );
        count(counters, |c| c.dropped_send_failed += 1);
        return Ok(());
    };
    for frame in frames {
        if let Err(error) = writer.write_all(&frame).await {
            count(counters, |c| c.dropped_send_failed += 1);
            return Err(error);
        }
    }
    count(counters, |c| c.sdus_sent += 1);
    Ok(())
}

/// Handles one frame of the data socket. Receive errors drop the frame and keep
/// the session (0011 D3 step 6; a refused oversized set is one more, 0016 D4).
fn handle_frame(
    queue: &mut VecDeque<Sdu>,
    counters: &watch::Sender<TransportCounters>,
    reassembler: &mut Reassembler,
    frame: Frame,
) {
    let body = match frame {
        Frame::Complete(body) => body,
        Frame::Oversize { length } => {
            tracing::warn!(
                "frame of {length} octets skipped: above {}",
                wire::MAX_FRAME_LEN
            );
            count(counters, |c| c.dropped_receive_oversize += 1);
            return;
        }
    };
    let Some(sdu_frame) = decode_sdu_frame(&body) else {
        tracing::warn!(
            "frame of {} octets dropped: shorter than an SDU header",
            body.len()
        );
        count(counters, |c| c.dropped_receive_malformed += 1);
        return;
    };

    let ReassemblyStep {
        sdu,
        dropped,
        too_large,
    } = reassembler.push(sdu_frame);
    if dropped > 0 {
        tracing::warn!(
            "{dropped} fragment(s) dropped: out of order, address mismatch or incomplete set"
        );
        count(counters, |c| c.dropped_receive_fragments += dropped);
    }
    if too_large {
        tracing::warn!(
            "fragment set refused: joined payload would pass {} octets",
            policy::RX_SDU_CAP
        );
        count(counters, |c| c.dropped_receive_too_large += 1);
    }
    let Some(sdu) = sdu else {
        return;
    };
    count(counters, |c| c.sdus_received += 1);
    if push_drop_oldest(queue, sdu, RECEIVED_QUEUE_CAPACITY) {
        tracing::warn!("received queue full: oldest received SDU dropped");
        count(counters, |c| c.dropped_receive_overflow += 1);
    }
}

/// Retry when the socket file is missing or nothing listens on it.
fn classify_connect(error: io::Error, operation: &'static str) -> RegisterFailure {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
            RegisterFailure::Retry(operation)
        }
        _ => RegisterFailure::Fatal(TransportError::Io {
            operation,
            source: error,
        }),
    }
}

/// Retry when the service closed or reset the socket before the response arrived.
fn classify_registration_io(error: io::Error, operation: &'static str) -> RegisterFailure {
    match error.kind() {
        io::ErrorKind::UnexpectedEof
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe => RegisterFailure::Retry(operation),
        _ => RegisterFailure::Fatal(TransportError::Io {
            operation,
            source: error,
        }),
    }
}

/// Connects control, then data, sends the request and reads one framed response
/// (0011 D3 steps 1 to 3). Upstream reads the response with one `recv`
/// (`al_service_access_point.cpp:119-121`), which can return half a frame; the
/// frame reader waits for the rest.
async fn register(config: &TransportConfig) -> Result<Registration, RegisterFailure> {
    let mut control = UnixStream::connect(&config.control_path)
        .await
        .map_err(|error| classify_connect(error, "connect control socket"))?;
    let data = UnixStream::connect(&config.data_path)
        .await
        .map_err(|error| classify_connect(error, "connect data socket"))?;

    control
        .write_all(&encode_registration_request(config.role))
        .await
        .map_err(|error| classify_registration_io(error, "send registration request"))?;

    let mut control = FrameReader::new(control);
    let frame = control
        .next_frame()
        .await
        .map_err(|error| classify_registration_io(error, "read registration response"))?;
    let Frame::Complete(body) = frame else {
        return Err(RegisterFailure::Retry("registration response too long"));
    };
    let Some(response) = decode_registration_response(&body) else {
        return Err(RegisterFailure::Retry("registration response too short"));
    };

    if response.result != RESULT_SUCCESS {
        return Err(RegisterFailure::Fatal(
            TransportError::RegistrationRejected {
                result: response.result,
            },
        ));
    }
    if response.al_mac.is_zero() {
        // The service sends all zeros when it cannot find its interface (`al_sap.rs:277-282`).
        tracing::warn!("service answered with an all-zero AL MAC, will retry");
        return Err(RegisterFailure::Retry("all-zero AL MAC"));
    }
    Ok(Registration {
        al_mac: response.al_mac,
        _control: control,
        data,
    })
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn sdu(tag: u8) -> Sdu {
        Sdu {
            local: MacAddr::new([2, 0, 0, 0, 0, 1]),
            remote: MacAddr::new([2, 0, 0, 0, 0, 2]),
            cmdu: vec![tag],
        }
    }

    #[test]
    fn full_queue_drops_the_oldest_and_keeps_the_order() {
        let mut queue = VecDeque::new();
        assert!(!push_drop_oldest(&mut queue, sdu(1), 2));
        assert!(!push_drop_oldest(&mut queue, sdu(2), 2));
        assert!(push_drop_oldest(&mut queue, sdu(3), 2));

        let tags: Vec<u8> = queue.iter().map(|sdu| sdu.cmdu[0]).collect();
        assert_eq!(tags, [2, 3]);
    }

    // Stamp and teardown tests. They place SDUs in the channel before the session runs, so
    // they do not depend on how the tasks are scheduled (0011 A6).

    /// Length of the frame `sdu(tag)` is written as: prefix 4, two MACs 12, flags 3, payload 1.
    const FRAME_LEN: usize = 20;

    struct Harness {
        actor: Actor,
        sdus: mpsc::Sender<OutboundSdu>,
        received: mpsc::Receiver<Sdu>,
        link: watch::Receiver<LinkState>,
        counters: watch::Receiver<TransportCounters>,
    }

    /// An actor whose link is `link` before its next registration.
    fn harness(link: LinkState) -> Harness {
        let (sdus, sdu_receiver) = mpsc::channel(SDU_CHANNEL_CAPACITY);
        let (received_sender, received) = mpsc::channel(RECEIVED_CHANNEL_CAPACITY);
        let (link_sender, link) = watch::channel(link);
        let (counters_sender, counters) = watch::channel(TransportCounters::default());
        let actor = Actor {
            sdus: sdu_receiver,
            received: received_sender,
            queue: VecDeque::new(),
            link: link_sender,
            counters: counters_sender,
        };
        Harness {
            actor,
            sdus,
            received,
            link,
            counters,
        }
    }

    /// A registration over socket pairs, and the service's end of the data socket.
    fn registration() -> (Registration, UnixStream) {
        let (control, _service_control) = UnixStream::pair().unwrap();
        let (data, service_data) = UnixStream::pair().unwrap();
        let registration = Registration {
            al_mac: MacAddr::new([2, 0, 0, 0, 0, 9]),
            _control: FrameReader::new(control),
            data,
        };
        (registration, service_data)
    }

    fn outbound(session: u64, tag: u8) -> OutboundSdu {
        OutboundSdu {
            session,
            sdu: sdu(tag),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn only_an_sdu_built_for_this_session_is_written() {
        // A6 (S0-01). The link is new, so this is session 1: 0 is the old one and 2 a later one.
        let Harness {
            mut actor,
            sdus,
            received,
            link,
            counters,
        } = harness(LinkState::default());
        let (registration, mut service) = registration();
        for session in 0..=2u8 {
            sdus.try_send(outbound(u64::from(session), 10 + session))
                .unwrap();
        }
        drop(received);

        let end = actor.run_session(registration).await;
        assert!(matches!(end, SessionEnd::Stop));

        let mut written = Vec::new();
        service.read_to_end(&mut written).await.unwrap();
        assert_eq!(written.len(), FRAME_LEN);
        assert_eq!(written[FRAME_LEN - 1], 11);
        let counters = *counters.borrow();
        assert_eq!(counters.sdus_sent, 1);
        assert_eq!(counters.dropped_send_disconnected, 2);
        let last = LinkState {
            session: 1,
            al_mac: None,
        };
        assert_eq!(*link.borrow(), last);
    }

    #[tokio::test(start_paused = true)]
    async fn sends_waiting_for_room_while_disconnected_are_not_written_into_the_new_session() {
        // A6 (S0-01): the channel is full, and one more send waits for room, all built for
        // session 1. Registration then makes this session 2. A fresh SDU follows them.
        let Harness {
            mut actor,
            sdus,
            received,
            counters,
            ..
        } = harness(LinkState {
            session: 1,
            al_mac: None,
        });
        let (registration, mut service) = registration();
        let producer = tokio::spawn(async move {
            for tag in 0..=u8::try_from(SDU_CHANNEL_CAPACITY).unwrap() {
                sdus.send(outbound(1, tag)).await.unwrap();
            }
            sdus.send(outbound(2, 200)).await.unwrap();
            sdus
        });
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert!(
            !producer.is_finished(),
            "the last send did not have to wait"
        );

        let script = async {
            let mut frame = [0u8; FRAME_LEN];
            service.read_exact(&mut frame).await.unwrap();
            assert_eq!(frame[FRAME_LEN - 1], 200, "an old SDU was written");
            drop(received);
        };
        let (end, ()) = tokio::join!(actor.run_session(registration), script);
        assert!(matches!(end, SessionEnd::Stop));

        let counters = *counters.borrow();
        assert_eq!(counters.sdus_sent, 1);
        assert_eq!(counters.dropped_send_disconnected, 65);
        drop(producer.await.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn stop_writes_only_the_queued_sdus_built_for_this_session() {
        // A6 (S0-01): the same comparison in the stop path's write of the queued SDUs.
        let Harness {
            mut actor,
            sdus,
            counters,
            ..
        } = harness(LinkState::default());
        let (actor_side, mut service) = UnixStream::pair().unwrap();
        let (read_half, mut write_half) = actor_side.into_split();
        sdus.try_send(outbound(0, 1)).unwrap();
        sdus.try_send(outbound(1, 2)).unwrap();
        sdus.try_send(outbound(2, 3)).unwrap();

        let stopped = actor.stop_in_session(&mut write_half, 1, true).await;
        assert!(matches!(stopped, SessionEnd::Stop));
        drop((read_half, write_half));

        let mut written = Vec::new();
        service.read_to_end(&mut written).await.unwrap();
        assert_eq!(written.len(), FRAME_LEN);
        assert_eq!(written[FRAME_LEN - 1], 2);
        assert_eq!(counters.borrow().dropped_send_disconnected, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn end_of_file_and_a_dropped_role_actor_ready_together_take_the_stop_path() {
        // A6 (S0-02): the first poll of the session finds end of file, the dropped receiver
        // and a queued SDU all ready. `select!` picks among them at random, so the check
        // repeats until a wrong order could not stay hidden (a miss has odds of about 1 in
        // 2 per round). Whichever arm is picked, the SDU must be written and the linger run.
        for round in 0..40 {
            let Harness {
                mut actor,
                sdus,
                received,
                ..
            } = harness(LinkState::default());
            let (registration, mut service) = registration();
            sdus.try_send(outbound(1, 7)).unwrap();
            service.shutdown().await.unwrap();
            // Lets the runtime's I/O driver see the end of file before the session starts.
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            drop(received);
            let started = tokio::time::Instant::now();

            let end = actor.run_session(registration).await;
            assert!(matches!(end, SessionEnd::Stop), "round {round}");
            assert!(started.elapsed() >= LINGER, "round {round}: no linger");
            let mut written = Vec::new();
            service.read_to_end(&mut written).await.unwrap();
            assert_eq!(written.len(), FRAME_LEN, "round {round}: SDU not written");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn write_failure_with_the_role_actor_gone_takes_the_stop_path() {
        // A6 (S0-02), A7 (S0-A7-01): the service stopped reading, and the only sender is gone.
        // The failed write must not look like a plain disconnect: the actor lingers and ends.
        // It writes nothing after the failed write, so the second SDU is counted, not sent.
        // The read arm cannot win: the service never writes or closes its end, so the read
        // stays pending and the outcome is the same whichever arm `select!` polls first.
        let Harness {
            mut actor,
            sdus,
            received: _received,
            counters,
            ..
        } = harness(LinkState::default());
        let (registration, service) = registration();
        let service = service.into_std().unwrap();
        service.shutdown(std::net::Shutdown::Read).unwrap();
        sdus.try_send(outbound(1, 1)).unwrap();
        sdus.try_send(outbound(1, 2)).unwrap();
        drop(sdus);
        let started = tokio::time::Instant::now();

        let end = actor.run_session(registration).await;
        assert!(matches!(end, SessionEnd::Stop));
        assert!(started.elapsed() >= LINGER, "no linger");
        let counters = *counters.borrow();
        assert_eq!(counters.sdus_sent, 0);
        assert_eq!(counters.dropped_send_failed, 1, "the write that failed");
        assert_eq!(counters.dropped_send_disconnected, 1, "the SDU behind it");
    }

    #[tokio::test(start_paused = true)]
    async fn blocked_stop_write_refuses_sends_and_ends_when_the_peer_closes() {
        // A7 (F1): the service holds the socket open and does not read, so the first write of
        // the stop blocks. The channel refuses new sends meanwhile. When the peer closes, the
        // write fails and the SDU behind it is counted, not written.
        let Harness {
            mut actor,
            sdus,
            counters,
            ..
        } = harness(LinkState::default());
        let (actor_side, service) = UnixStream::pair().unwrap();
        while actor_side.try_write(&[0u8; 4096]).is_ok() {}
        let (_read_half, mut write_half) = actor_side.into_split();
        sdus.try_send(outbound(1, 1)).unwrap();
        sdus.try_send(outbound(1, 2)).unwrap();

        let script = async {
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            assert!(
                matches!(
                    sdus.try_send(outbound(1, 3)),
                    Err(mpsc::error::TrySendError::Closed(_))
                ),
                "the channel took an SDU while the stop was writing"
            );
            drop(service);
        };
        let stopped = tokio::time::timeout(Duration::from_secs(10), async {
            let (stopped, ()) =
                tokio::join!(actor.stop_in_session(&mut write_half, 1, true), script);
            stopped
        })
        .await
        .expect("the stop did not end");
        assert!(matches!(stopped, SessionEnd::Stop));

        let counters = *counters.borrow();
        assert_eq!(counters.sdus_sent, 0);
        assert_eq!(counters.dropped_send_failed, 1, "the write that failed");
        assert_eq!(counters.dropped_send_disconnected, 1, "the SDU behind it");
    }

    fn is_retry(failure: &RegisterFailure) -> bool {
        matches!(failure, RegisterFailure::Retry(_))
    }

    /// True for a fatal failure that keeps the operation and the kind of the error.
    fn is_fatal_io(
        failure: &RegisterFailure,
        expected_operation: &str,
        kind: io::ErrorKind,
    ) -> bool {
        matches!(
            failure,
            RegisterFailure::Fatal(TransportError::Io { operation, source })
                if *operation == expected_operation && source.kind() == kind
        )
    }

    #[test]
    fn connect_errors_retry_only_for_a_missing_or_refused_socket() {
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::ConnectionRefused] {
            assert!(
                is_retry(&classify_connect(kind.into(), "connect")),
                "{kind:?}"
            );
        }
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::InvalidInput,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::TimedOut,
            io::ErrorKind::Other,
        ] {
            let failure = classify_connect(kind.into(), "connect");
            assert!(is_fatal_io(&failure, "connect", kind), "{kind:?}");
        }
    }

    #[test]
    fn registration_io_errors_retry_when_the_service_closed_or_reset() {
        for kind in [
            io::ErrorKind::UnexpectedEof,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::BrokenPipe,
        ] {
            assert!(
                is_retry(&classify_registration_io(kind.into(), "read")),
                "{kind:?}"
            );
        }
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::NotFound,
            io::ErrorKind::InvalidInput,
            io::ErrorKind::TimedOut,
            io::ErrorKind::Other,
        ] {
            let failure = classify_registration_io(kind.into(), "read");
            assert!(is_fatal_io(&failure, "read", kind), "{kind:?}");
        }
    }
}
