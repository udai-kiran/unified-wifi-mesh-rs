// SPDX-License-Identifier: Apache-2.0

//! The SDUs of one session that wait to be written to the data socket, with the progress of
//! the partial write. No I/O, no clock (0016 D1).

use std::collections::VecDeque;

use crate::Sdu;
use crate::wire::encode_sdu_frames;

/// An SDU in the buffer: its frames, one after the other, and how much was written.
#[derive(Debug)]
struct Queued {
    /// All the frames of the SDU, concatenated.
    frames: Vec<u8>,
    /// Octets of `frames` already written.
    written: usize,
}

impl Queued {
    fn is_started(&self) -> bool {
        self.written > 0
    }
}

/// What [`OutboundBuffer::push`] dropped. The counts are for `TransportCore` to apply to the
/// counters (4b-2b).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PushOutcome {
    /// SDUs that need more than 256 fragments: `dropped_send_failed` (as `send_sdu` counts them).
    pub(crate) send_failed: u64,
    /// SDUs dropped for room, or refused: `dropped_send_overflow` (0016 U12).
    pub(crate) overflow: u64,
}

/// What [`OutboundBuffer::advance`] finished.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AdvanceOutcome {
    /// SDUs whose last octet was written: `sdus_sent`. At most 1 per call.
    pub(crate) sdus_sent: u64,
}

/// What [`OutboundBuffer::end_session`] dropped, by the one counting rule for every session end
/// (0016 D3b).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SessionEndOutcome {
    /// A partly written SDU (0 or 1): `dropped_send_failed`.
    pub(crate) send_failed: u64,
    /// SDUs not yet started: `dropped_send_disconnected`.
    pub(crate) send_disconnected: u64,
}

/// The write queue of one session (0016 U12, D3b; difference `tx-buffer`). Built here, used
/// from 4b-2b.
///
/// The buffer holds at most `max_bytes` octets, measured as the octets it stores: the encoded
/// frames of each queued SDU (the CMDU plus 19 octets of header per fragment), the one being
/// written included. 0016 U12 measures the sum of `cmdu.len()`; this counts the stored frames (at
/// most 19 octets more per fragment) so that SDUs with an empty CMDU cannot grow the buffer
/// without bound (coordinator decision). When a push would pass the limit, the oldest SDUs
/// that are not started are dropped until it fits. An SDU that cannot fit even then (it is alone
/// over the limit, or the started SDU and it together are) is refused, and nothing else is
/// dropped: 0016 U12, coordinator decision for an SDU above the limit (mirrors E6).
///
/// **Memory bound.** The buffer stores at most `max_bytes` octets of frames. While `push`
/// encodes, the SDU and its frames exist twice for a moment (up to 16 MiB for the largest SDU).
#[derive(Debug)]
pub(crate) struct OutboundBuffer {
    queue: VecDeque<Queued>,
    /// Sum of `frames.len()` over `queue`.
    bytes: usize,
    max_bytes: usize,
}

impl OutboundBuffer {
    pub(crate) fn new(max_bytes: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            bytes: 0,
            max_bytes,
        }
    }

    /// Encodes `sdu` and queues it, dropping or refusing as the type's docs describe.
    pub(crate) fn push(&mut self, sdu: &Sdu) -> PushOutcome {
        let mut outcome = PushOutcome::default();
        let Ok(frames) = encode_sdu_frames(sdu) else {
            outcome.send_failed = 1;
            return outcome;
        };
        let frames = frames.concat();
        let stored_len = frames.len();

        // Only the oldest SDU can be started: SDUs are written one after the other.
        let oldest_started = self.is_oldest_started();
        let started_bytes = match self.queue.front() {
            Some(oldest) if oldest_started => oldest.frames.len(),
            _ => 0,
        };
        if started_bytes + stored_len > self.max_bytes {
            outcome.overflow = 1;
            return outcome;
        }

        let first_unstarted = usize::from(oldest_started);
        while self.bytes + stored_len > self.max_bytes {
            // The check above shows that dropping the unstarted SDUs is enough.
            if let Some(dropped) = self.queue.remove(first_unstarted) {
                self.bytes -= dropped.frames.len();
            }
            outcome.overflow += 1;
        }

        self.bytes += stored_len;
        self.queue.push_back(Queued { frames, written: 0 });
        outcome
    }

    /// The octets of the oldest SDU that are not written yet: all its frames, concatenated.
    pub(crate) fn next_chunk(&self) -> Option<&[u8]> {
        let oldest = self.queue.front()?;
        Some(&oldest.frames[oldest.written..])
    }

    /// Records that `written` octets of [`OutboundBuffer::next_chunk`] were written.
    ///
    /// # Panics
    ///
    /// If the buffer is empty (there is no chunk), or `written` is more than the length of the
    /// chunk.
    pub(crate) fn advance(&mut self, written: usize) -> AdvanceOutcome {
        let oldest = self
            .queue
            .front_mut()
            .expect("advance needs a chunk to have been written");
        assert!(
            written <= oldest.frames.len() - oldest.written,
            "advance past the end of the chunk"
        );
        oldest.written += written;
        if oldest.written < oldest.frames.len() {
            return AdvanceOutcome::default();
        }
        self.bytes -= oldest.frames.len();
        self.queue.pop_front();
        AdvanceOutcome { sdus_sent: 1 }
    }

    /// True if the oldest SDU is partly written. Only the tests read it.
    #[cfg(test)]
    pub(crate) fn has_started(&self) -> bool {
        self.is_oldest_started()
    }

    /// Empties the buffer at the end of a session and returns what it counts (0016 D3b).
    pub(crate) fn end_session(&mut self) -> SessionEndOutcome {
        let mut outcome = SessionEndOutcome::default();
        for queued in self.queue.drain(..) {
            if queued.is_started() {
                outcome.send_failed += 1;
            } else {
                outcome.send_disconnected += 1;
            }
        }
        self.bytes = 0;
        outcome
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// The octets stored: the sum of the encoded frame lengths of the queued SDUs. Only the
    /// tests read it.
    #[cfg(test)]
    pub(crate) fn byte_count(&self) -> usize {
        self.bytes
    }

    fn is_oldest_started(&self) -> bool {
        self.queue.front().is_some_and(Queued::is_started)
    }
}

#[cfg(test)]
mod tests {
    use em_types::MacAddr;

    use super::*;
    use crate::test_prng::Prng;
    use crate::wire::{MAX_FRAGMENT_PAYLOAD, MAX_SDU_PAYLOAD};

    /// An SDU that no other id equals: the id is in `local`, and `cmdu` is `length` copies of it.
    fn sdu(id: u8, length: usize) -> Sdu {
        Sdu {
            local: MacAddr::new([2, 0, 0, 0, 0, id]),
            remote: MacAddr::new([2, 0, 0, 0, 1, 0]),
            cmdu: vec![id; length],
        }
    }

    /// What the buffer must give for `sdu`: its frames, concatenated.
    fn encoded(sdu: &Sdu) -> Vec<u8> {
        encode_sdu_frames(sdu).unwrap().concat()
    }

    /// Octets the buffer stores for `sdu`: its frames.
    fn stored(sdu: &Sdu) -> usize {
        encoded(sdu).len()
    }

    fn overflow(count: u64) -> PushOutcome {
        PushOutcome {
            send_failed: 0,
            overflow: count,
        }
    }

    fn sent(count: u64) -> AdvanceOutcome {
        AdvanceOutcome { sdus_sent: count }
    }

    #[test]
    fn empty_buffer_has_no_chunk() {
        let buffer = OutboundBuffer::new(100);
        assert!(buffer.is_empty());
        assert_eq!(buffer.next_chunk(), None);
        assert!(!buffer.has_started());
        assert_eq!(buffer.byte_count(), 0);
    }

    #[test]
    fn writes_one_sdu_in_pieces() {
        let mut buffer = OutboundBuffer::new(100);
        assert_eq!(buffer.push(&sdu(1, 6)), PushOutcome::default());
        let all = encoded(&sdu(1, 6));
        assert_eq!(buffer.next_chunk(), Some(&all[..]));
        assert!(!buffer.has_started());

        assert_eq!(buffer.advance(5), sent(0));
        assert!(buffer.has_started());
        assert_eq!(buffer.next_chunk(), Some(&all[5..]));
        assert_eq!(buffer.byte_count(), all.len());

        assert_eq!(buffer.advance(all.len() - 5), sent(1));
        assert!(buffer.is_empty());
        assert!(!buffer.has_started());
        assert_eq!(buffer.byte_count(), 0);
    }

    #[test]
    fn chunk_is_the_oldest_sdu_and_the_next_one_is_not_started() {
        let mut buffer = OutboundBuffer::new(100);
        buffer.push(&sdu(1, 2));
        buffer.push(&sdu(2, 3));
        let first = encoded(&sdu(1, 2));
        assert_eq!(buffer.advance(first.len()), sent(1));
        assert!(!buffer.has_started());
        let second = encoded(&sdu(2, 3));
        assert_eq!(buffer.next_chunk(), Some(&second[..]));
    }

    #[test]
    fn fragmented_sdu_chunk_holds_all_its_frames() {
        let mut buffer = OutboundBuffer::new(1_000_000);
        let big = || sdu(7, MAX_FRAGMENT_PAYLOAD + 1);
        assert_eq!(buffer.push(&big()), PushOutcome::default());
        let frames = encode_sdu_frames(&big()).unwrap();
        assert_eq!(frames.len(), 2);
        let chunk = buffer.next_chunk().unwrap();
        assert_eq!(chunk.len(), frames[0].len() + frames[1].len());
        assert_eq!(chunk, frames.concat());
        // The first frame written does not finish the SDU.
        assert_eq!(buffer.advance(frames[0].len()), sent(0));
        assert_eq!(buffer.next_chunk(), Some(&frames[1][..]));
    }

    #[test]
    fn sdu_needing_more_than_256_fragments_is_send_failed() {
        let mut buffer = OutboundBuffer::new(usize::MAX);
        let outcome = buffer.push(&sdu(1, MAX_SDU_PAYLOAD + 1));
        assert_eq!(
            outcome,
            PushOutcome {
                send_failed: 1,
                overflow: 0
            }
        );
        assert!(buffer.is_empty());
        assert_eq!(
            buffer.push(&sdu(2, MAX_SDU_PAYLOAD)),
            PushOutcome::default()
        );
    }

    #[test]
    fn overflow_drops_the_oldest_unstarted() {
        // intentional-difference:tx-buffer
        let mut buffer = OutboundBuffer::new(50);
        buffer.push(&sdu(1, 4));
        buffer.push(&sdu(2, 4));
        // 23 + 23 + 24 = 70: dropping SDU 1 leaves 47.
        assert_eq!(buffer.push(&sdu(3, 5)), overflow(1));
        assert_eq!(buffer.byte_count(), stored(&sdu(2, 4)) + stored(&sdu(3, 5)));
        assert_eq!(buffer.byte_count(), 47);
        let second = encoded(&sdu(2, 4));
        assert_eq!(buffer.next_chunk(), Some(&second[..]));
    }

    #[test]
    fn overflow_keeps_the_started_sdu() {
        // intentional-difference:tx-buffer
        let mut buffer = OutboundBuffer::new(50);
        buffer.push(&sdu(1, 4));
        buffer.push(&sdu(2, 4));
        buffer.advance(1);
        assert_eq!(buffer.push(&sdu(3, 5)), overflow(1));
        // SDU 1 (started, 23 octets) and SDU 3 (24 octets) remain.
        assert_eq!(buffer.byte_count(), 47);
        let first = encoded(&sdu(1, 4));
        assert_eq!(buffer.next_chunk(), Some(&first[1..]));
        buffer.advance(first.len() - 1);
        let third = encoded(&sdu(3, 5));
        assert_eq!(buffer.next_chunk(), Some(&third[..]));
    }

    #[test]
    fn sdu_exactly_at_the_limit_is_kept() {
        let mut buffer = OutboundBuffer::new(stored(&sdu(2, 10)));
        buffer.push(&sdu(1, 3));
        assert_eq!(buffer.push(&sdu(2, 10)), overflow(1));
        assert_eq!(buffer.byte_count(), 29);
    }

    #[test]
    fn sdu_above_the_limit_is_refused_and_nothing_is_dropped() {
        // 0016 U12, coordinator decision for an SDU above the limit.
        let mut buffer = OutboundBuffer::new(50);
        buffer.push(&sdu(1, 4));
        // 19 + 32 = 51 octets stored, one above the limit.
        assert_eq!(buffer.push(&sdu(2, 32)), overflow(1));
        assert_eq!(buffer.byte_count(), 23);
        let first = encoded(&sdu(1, 4));
        assert_eq!(buffer.next_chunk(), Some(&first[..]));
    }

    #[test]
    fn sdu_that_fits_only_without_the_started_one_is_refused_and_nothing_is_dropped() {
        // 0016 U12, coordinator decision for an SDU above the limit.
        let mut buffer = OutboundBuffer::new(48);
        buffer.push(&sdu(1, 6));
        buffer.push(&sdu(2, 2));
        buffer.advance(1);
        // The started SDU 1 (25) and SDU 3 (24) make 49, one above the limit.
        assert_eq!(buffer.push(&sdu(3, 5)), overflow(1));
        assert_eq!(buffer.byte_count(), 46);
        // SDU 2 is still queued behind the started SDU 1.
        let first = encoded(&sdu(1, 6));
        buffer.advance(first.len() - 1);
        let second = encoded(&sdu(2, 2));
        assert_eq!(buffer.next_chunk(), Some(&second[..]));
    }

    #[test]
    fn tx_buffer_holds_4_mib_and_more_than_64_sdus() {
        // intentional-difference:tx-buffer
        // The old PendingFrames held 64 frames, drop oldest, plus 64 in the channel.
        let mut buffer = OutboundBuffer::new(4 * 1024 * 1024);
        for id in 0..200 {
            assert_eq!(buffer.push(&sdu(id, 10)), PushOutcome::default());
        }
        assert_eq!(buffer.byte_count(), 200 * 29);

        // 16 fragments: the frames of one SDU are 304 octets more than its CMDU, so 4 of them
        // fit in 4 MiB.
        let cmdu_len = 1024 * 1024 - 512;
        let one = stored(&sdu(1, cmdu_len));
        let mut full = OutboundBuffer::new(4 * 1024 * 1024);
        for id in 1..=4 {
            assert_eq!(full.push(&sdu(id, cmdu_len)), PushOutcome::default());
        }
        assert_eq!(full.push(&sdu(5, cmdu_len)), overflow(1));
        assert_eq!(full.byte_count(), 4 * one);
    }

    #[test]
    fn empty_cmdus_are_bounded_by_their_frames() {
        // Each empty CMDU is stored as one 19-octet frame: 5 fit in 100 octets.
        let mut buffer = OutboundBuffer::new(100);
        let mut overflowed = 0;
        for id in 0..=255 {
            overflowed += buffer.push(&sdu(id, 0)).overflow;
        }
        assert_eq!(buffer.byte_count(), 5 * 19);
        assert_eq!(overflowed, 256 - 5);

        // With no room at all, none is kept.
        let mut none = OutboundBuffer::new(0);
        assert_eq!(none.push(&sdu(1, 0)), overflow(1));
        assert!(none.is_empty());
        assert_eq!(none.byte_count(), 0);
    }

    #[test]
    fn end_session_counts_started_and_unstarted() {
        let mut buffer = OutboundBuffer::new(100);
        buffer.push(&sdu(1, 3));
        buffer.push(&sdu(2, 3));
        buffer.push(&sdu(3, 3));
        buffer.advance(2);
        assert_eq!(
            buffer.end_session(),
            SessionEndOutcome {
                send_failed: 1,
                send_disconnected: 2
            }
        );
        assert!(buffer.is_empty());
        assert_eq!(buffer.byte_count(), 0);
        assert_eq!(buffer.next_chunk(), None);
    }

    #[test]
    fn end_session_with_nothing_started() {
        let mut buffer = OutboundBuffer::new(100);
        buffer.push(&sdu(1, 3));
        buffer.push(&sdu(2, 3));
        assert_eq!(
            buffer.end_session(),
            SessionEndOutcome {
                send_failed: 0,
                send_disconnected: 2
            }
        );
        assert_eq!(buffer.end_session(), SessionEndOutcome::default());
    }

    /// One SDU of the reference: its encoded octets, and how many were written.
    struct ModelEntry {
        bytes: Vec<u8>,
        written: usize,
    }

    /// The reference: a list. A push keeps the started SDU and the newest unstarted SDUs that fit.
    struct Model {
        entries: Vec<ModelEntry>,
        max_bytes: usize,
    }

    impl Model {
        fn byte_count(&self) -> usize {
            self.entries.iter().map(|entry| entry.bytes.len()).sum()
        }

        fn push(&mut self, sdu: &Sdu) -> u64 {
            let started = self.entries.first().is_some_and(|entry| entry.written > 0);
            let first_unstarted = usize::from(started);
            let started_bytes = if started {
                self.entries[0].bytes.len()
            } else {
                0
            };
            let new_bytes = encoded(sdu);
            let new_len = new_bytes.len();
            if started_bytes + new_len > self.max_bytes {
                return 1;
            }
            // Keep the newest unstarted SDUs while they fit with the started SDU and the new one.
            let mut budget = self.max_bytes - started_bytes - new_len;
            let mut keep_from = self.entries.len();
            while keep_from > first_unstarted && self.entries[keep_from - 1].bytes.len() <= budget {
                keep_from -= 1;
                budget -= self.entries[keep_from].bytes.len();
            }
            let dropped = keep_from - first_unstarted;
            self.entries.drain(first_unstarted..keep_from);
            self.entries.push(ModelEntry {
                bytes: new_bytes,
                written: 0,
            });
            dropped as u64
        }
    }

    #[test]
    fn model_random_push_advance_and_end_session() {
        for seed in 1..=300u64 {
            let mut rng = Prng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let max_bytes = rng.below(100);
            let mut buffer = OutboundBuffer::new(max_bytes);
            let mut model = Model {
                entries: Vec::new(),
                max_bytes,
            };
            let mut next_id = 0u8;
            for step in 0..200 {
                let context = format!("seed {seed} step {step}");
                match rng.below(8) {
                    0..=3 => {
                        next_id = next_id.wrapping_add(1);
                        let length = rng.below(12);
                        let outcome = buffer.push(&sdu(next_id, length));
                        let dropped = model.push(&sdu(next_id, length));
                        assert_eq!(outcome, overflow(dropped), "{context}");
                    }
                    4..=6 => {
                        let Some(entry) = model.entries.first_mut() else {
                            assert_eq!(buffer.next_chunk(), None, "{context}");
                            continue;
                        };
                        let remaining = entry.bytes.len() - entry.written;
                        let written = 1 + rng.below(remaining.min(14));
                        let expected_sent = u64::from(written == remaining);
                        assert_eq!(buffer.advance(written), sent(expected_sent), "{context}");
                        entry.written += written;
                        if expected_sent == 1 {
                            model.entries.remove(0);
                        }
                    }
                    _ => {
                        let started = model.entries.first().is_some_and(|e| e.written > 0);
                        let expected = SessionEndOutcome {
                            send_failed: u64::from(started),
                            send_disconnected: (model.entries.len() - usize::from(started)) as u64,
                        };
                        assert_eq!(buffer.end_session(), expected, "{context}");
                        model.entries.clear();
                    }
                }
                let expected_chunk = model
                    .entries
                    .first()
                    .map(|entry| &entry.bytes[entry.written..]);
                assert_eq!(buffer.next_chunk(), expected_chunk, "{context}");
                assert_eq!(buffer.byte_count(), model.byte_count(), "{context}");
                assert_eq!(buffer.is_empty(), model.entries.is_empty(), "{context}");
                let started = model.entries.first().is_some_and(|e| e.written > 0);
                assert_eq!(buffer.has_started(), started, "{context}");
            }
        }
    }
}
