// SPDX-License-Identifier: Apache-2.0

//! The queue of received SDUs that the role has not taken yet. No I/O, no clock (0016 D1).

use std::collections::VecDeque;

use crate::Sdu;

/// A queue of at most `max_sdus` SDUs and `max_bytes` octets of CMDU (the sum of `cmdu.len()`).
///
/// When a push would pass either limit, the oldest SDUs are dropped (0016 U1, difference
/// `rx-queue-1024-4mib`; the old actor queue keeps 256 SDUs and counts no octets). The limits
/// are arguments: the numbers live in `policy` (0016 U31). Built here, used from 4b-2b.
#[derive(Debug)]
pub(crate) struct ReceivedQueue {
    sdus: VecDeque<Sdu>,
    /// Sum of `cmdu.len()` over `sdus`.
    bytes: usize,
    max_sdus: usize,
    max_bytes: usize,
}

impl ReceivedQueue {
    pub(crate) fn new(max_sdus: usize, max_bytes: usize) -> Self {
        Self {
            sdus: VecDeque::new(),
            bytes: 0,
            max_sdus,
            max_bytes,
        }
    }

    /// Adds `sdu` and returns how many SDUs were dropped (0016 U1).
    ///
    /// An SDU larger than `max_bytes`, or any SDU when a limit is 0, is dropped alone and the
    /// queue stays as it was: it counts as 1 dropped (0016 E6, U34). Otherwise the oldest SDUs
    /// are dropped until both limits hold with `sdu` in the queue.
    pub(crate) fn push(&mut self, sdu: Sdu) -> u64 {
        let never_fits =
            self.max_sdus == 0 || self.max_bytes == 0 || sdu.cmdu.len() > self.max_bytes;
        if never_fits {
            return 1;
        }
        self.bytes += sdu.cmdu.len();
        self.sdus.push_back(sdu);
        let mut dropped = 0;
        while self.sdus.len() > self.max_sdus || self.bytes > self.max_bytes {
            // The new SDU fits alone, so the loop ends before it reaches the new SDU.
            if let Some(oldest) = self.sdus.pop_front() {
                self.bytes -= oldest.cmdu.len();
            }
            dropped += 1;
        }
        dropped
    }

    /// Takes the oldest SDU.
    pub(crate) fn pop(&mut self) -> Option<Sdu> {
        let oldest = self.sdus.pop_front()?;
        self.bytes -= oldest.cmdu.len();
        Some(oldest)
    }

    /// Only the tests read it.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.sdus.len()
    }

    /// The sum of `cmdu.len()` of the queued SDUs. Only the tests read it.
    #[cfg(test)]
    pub(crate) fn byte_count(&self) -> usize {
        self.bytes
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sdus.is_empty()
    }

    /// Empties the queue and returns how many SDUs it held: the `dropped_receive_stopped` count.
    pub(crate) fn clear(&mut self) -> u64 {
        let held = self.sdus.len() as u64;
        self.sdus.clear();
        self.bytes = 0;
        held
    }
}

#[cfg(test)]
mod tests {
    use em_types::MacAddr;

    use super::*;
    use crate::test_prng::Prng;

    /// An SDU that no other id equals: the id is in `local`, and `cmdu` is `length` copies of it.
    fn sdu(id: u8, length: usize) -> Sdu {
        Sdu {
            local: MacAddr::new([2, 0, 0, 0, 0, id]),
            remote: MacAddr::new([2, 0, 0, 0, 1, 0]),
            cmdu: vec![id; length],
        }
    }

    #[test]
    fn pops_oldest_first() {
        let mut queue = ReceivedQueue::new(4, 100);
        assert!(queue.is_empty());
        assert_eq!(queue.push(sdu(1, 3)), 0);
        assert_eq!(queue.push(sdu(2, 5)), 0);
        assert_eq!((queue.len(), queue.byte_count()), (2, 8));
        assert_eq!(queue.pop(), Some(sdu(1, 3)));
        assert_eq!((queue.len(), queue.byte_count()), (1, 5));
        assert_eq!(queue.pop(), Some(sdu(2, 5)));
        assert_eq!(queue.pop(), None);
        assert!(queue.is_empty());
        assert_eq!(queue.byte_count(), 0);
    }

    #[test]
    fn count_limit_drops_the_oldest() {
        let mut queue = ReceivedQueue::new(2, 100);
        assert_eq!(queue.push(sdu(1, 1)), 0);
        assert_eq!(queue.push(sdu(2, 1)), 0);
        assert_eq!(queue.push(sdu(3, 1)), 1);
        assert_eq!(queue.pop(), Some(sdu(2, 1)));
        assert_eq!(queue.pop(), Some(sdu(3, 1)));
    }

    #[test]
    fn byte_limit_drops_as_many_oldest_as_needed() {
        let mut queue = ReceivedQueue::new(10, 10);
        assert_eq!(queue.push(sdu(1, 3)), 0);
        assert_eq!(queue.push(sdu(2, 3)), 0);
        assert_eq!(queue.push(sdu(3, 3)), 0);
        // 9 + 8 = 17 octets: dropping SDU 1 leaves 14, dropping SDU 2 leaves 11, SDU 3 leaves 8.
        assert_eq!(queue.push(sdu(4, 8)), 3);
        assert_eq!(queue.byte_count(), 8);
        assert_eq!(queue.pop(), Some(sdu(4, 8)));
    }

    #[test]
    fn sdu_exactly_at_the_byte_limit_is_kept() {
        let mut queue = ReceivedQueue::new(10, 10);
        assert_eq!(queue.push(sdu(1, 2)), 0);
        assert_eq!(queue.push(sdu(2, 10)), 1);
        assert_eq!(queue.pop(), Some(sdu(2, 10)));
    }

    #[test]
    fn sdu_above_the_byte_limit_is_dropped_alone() {
        let mut queue = ReceivedQueue::new(10, 10);
        assert_eq!(queue.push(sdu(1, 4)), 0);
        assert_eq!(queue.push(sdu(2, 11)), 1);
        assert_eq!((queue.len(), queue.byte_count()), (1, 4));
        assert_eq!(queue.pop(), Some(sdu(1, 4)));
    }

    #[test]
    fn a_zero_limit_drops_every_sdu() {
        let mut no_sdus = ReceivedQueue::new(0, 10);
        assert_eq!(no_sdus.push(sdu(1, 0)), 1);
        assert!(no_sdus.is_empty());
        let mut no_bytes = ReceivedQueue::new(10, 0);
        assert_eq!(no_bytes.push(sdu(1, 0)), 1);
        assert!(no_bytes.is_empty());
    }

    #[test]
    fn clear_returns_the_count_and_empties() {
        let mut queue = ReceivedQueue::new(10, 100);
        queue.push(sdu(1, 3));
        queue.push(sdu(2, 3));
        assert_eq!(queue.clear(), 2);
        assert!(queue.is_empty());
        assert_eq!(queue.byte_count(), 0);
        assert_eq!(queue.clear(), 0);
    }

    #[test]
    fn rx_queue_1024_sdus_and_4_mib() {
        // intentional-difference:rx-queue-1024-4mib
        // The old actor queue held 256 SDUs and counted no octets.
        let mut queue = ReceivedQueue::new(1024, 4 * 1024 * 1024);
        for id in 0..=255 {
            assert_eq!(queue.push(sdu(id, 1)), 0);
        }
        for _ in 0..(1024 - 256) {
            assert_eq!(queue.push(sdu(0, 1)), 0);
        }
        assert_eq!(queue.len(), 1024);
        assert_eq!(queue.push(sdu(9, 1)), 1);
        assert_eq!(queue.len(), 1024);

        // intentional-difference:rx-queue-1024-4mib
        // 1 MiB each: the fifth SDU pushes out the first.
        let mut big = ReceivedQueue::new(1024, 4 * 1024 * 1024);
        for id in 1..=4 {
            assert_eq!(big.push(sdu(id, 1024 * 1024)), 0);
        }
        assert_eq!(big.push(sdu(5, 1024 * 1024)), 1);
        assert_eq!(big.byte_count(), 4 * 1024 * 1024);
        assert_eq!(big.pop().map(|oldest| oldest.local), Some(sdu(2, 0).local));
    }

    /// The reference: the queue as a plain list. A push keeps the longest suffix of
    /// "old SDUs and the new one" that obeys the limits; the dropped count is the rest.
    struct Model {
        sdus: Vec<Sdu>,
        max_sdus: usize,
        max_bytes: usize,
    }

    impl Model {
        fn fits(&self, sdus: &[Sdu]) -> bool {
            let bytes: usize = sdus.iter().map(|sdu| sdu.cmdu.len()).sum();
            sdus.len() <= self.max_sdus && bytes <= self.max_bytes
        }

        fn push(&mut self, sdu: Sdu) -> u64 {
            if self.max_sdus == 0 || self.max_bytes == 0 {
                return 1;
            }
            let mut all = std::mem::take(&mut self.sdus);
            all.push(sdu);
            let keep_from = (0..=all.len())
                .find(|&start| self.fits(&all[start..]))
                .unwrap();
            if keep_from == all.len() {
                // Not even the new SDU fits: the queue stays as it was, the new SDU is the drop.
                all.pop();
                self.sdus = all;
                return 1;
            }
            self.sdus = all.split_off(keep_from);
            keep_from as u64
        }
    }

    #[test]
    fn model_random_push_and_pop() {
        for seed in 1..=300u64 {
            let mut rng = Prng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let max_sdus = rng.below(6);
            let max_bytes = rng.below(14);
            let mut queue = ReceivedQueue::new(max_sdus, max_bytes);
            let mut model = Model {
                sdus: Vec::new(),
                max_sdus,
                max_bytes,
            };
            let mut next_id = 0u8;
            for _ in 0..200 {
                if rng.below(3) == 0 {
                    let expected = if model.sdus.is_empty() {
                        None
                    } else {
                        Some(model.sdus.remove(0))
                    };
                    assert_eq!(queue.pop(), expected, "seed {seed}");
                } else {
                    next_id = next_id.wrapping_add(1);
                    let length = rng.below(9);
                    let dropped = queue.push(sdu(next_id, length));
                    assert_eq!(dropped, model.push(sdu(next_id, length)), "seed {seed}");
                }
                let model_bytes: usize = model.sdus.iter().map(|sdu| sdu.cmdu.len()).sum();
                assert_eq!(queue.len(), model.sdus.len(), "seed {seed}");
                assert_eq!(queue.byte_count(), model_bytes, "seed {seed}");
                assert_eq!(queue.is_empty(), model.sdus.is_empty(), "seed {seed}");
            }
            assert_eq!(queue.clear(), model.sdus.len() as u64, "seed {seed}");
            assert_eq!(queue.pop(), None, "seed {seed}");
        }
    }
}
