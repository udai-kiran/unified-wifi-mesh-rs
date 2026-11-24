// SPDX-License-Identifier: Apache-2.0

//! Cuts a stream of octets into length-prefixed frames. No I/O, no clock (0016 D1).

use std::collections::VecDeque;

use crate::wire::{LENGTH_PREFIX_LEN, MAX_FRAME_LEN};

/// One thing cut from the stream.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Frame {
    /// A frame body, without its length prefix.
    Complete(Vec<u8>),
    /// A frame longer than [`MAX_FRAME_LEN`]. Its whole body was received and thrown away,
    /// so the stream is at the start of the next frame (0011 D3 step 6).
    Oversize { length: u32 },
}

/// Turns pushed octets into [`Frame`]s.
///
/// **State machine.** `push_bytes` cuts the stream as the octets arrive, in one of two states:
///
/// - *Collecting* (`skip_remaining == 0`): octets go to `partial` until the 4-octet prefix is
///   there, then until the declared body is there. A body of at most [`MAX_FRAME_LEN`] octets
///   becomes a `Complete` frame in `ready`, and `partial` is emptied. A prefix above
///   [`MAX_FRAME_LEN`] empties `partial` and starts *skipping*.
/// - *Skipping* (`skip_remaining > 0`): the declared body octets are thrown away as they arrive,
///   also those in the push that held the prefix. When the last one is gone, `Oversize` goes to
///   `ready` and the state is *collecting* again.
///
/// `next_frame` takes the front of `ready`, so frames come in stream order and `has_frame` is
/// true exactly when `ready` is not empty.
///
/// **Memory bound.** `partial` holds at most one frame that is not complete and not oversize
/// (4 + [`MAX_FRAME_LEN`] octets). `ready` holds the frames not yet taken, so a caller that
/// pushes without taking frames makes it grow, but never with the body of an oversize frame.
#[derive(Debug, Default)]
pub(crate) struct FrameSplitter {
    /// The prefix and body octets of the frame being collected.
    partial: Vec<u8>,
    /// Frames cut from the stream and not yet taken.
    ready: VecDeque<Frame>,
    /// Octets of an oversize body still to throw away.
    skip_remaining: u64,
    /// Length of the oversize frame being skipped.
    skipping_length: u32,
}

impl FrameSplitter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds octets that arrived from the stream.
    pub(crate) fn push_bytes(&mut self, bytes: &[u8]) {
        let mut input = bytes;
        while !input.is_empty() {
            if self.skip_remaining > 0 {
                // A remaining count too big for usize is certainly above the input length.
                let discard = usize::try_from(self.skip_remaining)
                    .map_or(input.len(), |remaining| remaining.min(input.len()));
                input = &input[discard..];
                self.skip_remaining -= discard as u64;
                if self.skip_remaining == 0 {
                    self.ready.push_back(Frame::Oversize {
                        length: self.skipping_length,
                    });
                }
                continue;
            }

            // Take only the octets that the frame in progress still needs.
            let needed = match self.partial_length() {
                None => LENGTH_PREFIX_LEN - self.partial.len(),
                Some(length) => LENGTH_PREFIX_LEN + length as usize - self.partial.len(),
            };
            let taken = needed.min(input.len());
            self.partial.extend_from_slice(&input[..taken]);
            input = &input[taken..];

            let Some(length) = self.partial_length() else {
                continue;
            };
            if length > MAX_FRAME_LEN {
                self.partial.clear();
                self.skip_remaining = u64::from(length);
                self.skipping_length = length;
            } else if self.partial.len() == LENGTH_PREFIX_LEN + length as usize {
                let body = self.partial[LENGTH_PREFIX_LEN..].to_vec();
                self.partial.clear();
                self.ready.push_back(Frame::Complete(body));
            }
        }
    }

    /// True exactly when [`FrameSplitter::next_frame`] would return a frame. Used from 4b-2b
    /// (the loop keeps the read source ready while frames are left, 0016 D3b).
    #[allow(dead_code)]
    pub(crate) fn has_frame(&self) -> bool {
        !self.ready.is_empty()
    }

    /// Takes the oldest frame that is ready.
    pub(crate) fn next_frame(&mut self) -> Option<Frame> {
        self.ready.pop_front()
    }

    /// The length at the front of `partial`, if the whole prefix is there.
    fn partial_length(&self) -> Option<u32> {
        let prefix: [u8; LENGTH_PREFIX_LEN] =
            self.partial.get(..LENGTH_PREFIX_LEN)?.try_into().ok()?;
        Some(u32::from_be_bytes(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_prng::Prng;

    #[test]
    fn nothing_without_a_whole_prefix() {
        let mut splitter = FrameSplitter::new();
        assert!(!splitter.has_frame());
        assert_eq!(splitter.next_frame(), None);
        splitter.push_bytes(&[0, 0, 0]);
        assert!(!splitter.has_frame());
        assert_eq!(splitter.next_frame(), None);
    }

    #[test]
    fn frames_split_across_pushes() {
        let mut splitter = FrameSplitter::new();
        splitter.push_bytes(&[0, 0, 0, 3, 1, 2]);
        assert!(!splitter.has_frame());
        splitter.push_bytes(&[3, 0, 0, 0, 1, 9]);
        assert!(splitter.has_frame());
        assert_eq!(splitter.next_frame(), Some(Frame::Complete(vec![1, 2, 3])));
        assert!(splitter.has_frame());
        assert_eq!(splitter.next_frame(), Some(Frame::Complete(vec![9])));
        assert!(!splitter.has_frame());
        assert_eq!(splitter.next_frame(), None);
    }

    #[test]
    fn empty_frame_is_a_frame() {
        let mut splitter = FrameSplitter::new();
        splitter.push_bytes(&[0, 0, 0, 0]);
        assert!(splitter.has_frame());
        assert_eq!(splitter.next_frame(), Some(Frame::Complete(Vec::new())));
    }

    #[test]
    fn oversize_frame_is_reported_when_its_last_octet_arrives() {
        let mut splitter = FrameSplitter::new();
        let oversize = MAX_FRAME_LEN + 1;
        splitter.push_bytes(&oversize.to_be_bytes());
        splitter.push_bytes(&vec![0xAA; oversize as usize - 1]);
        assert!(!splitter.has_frame());
        assert_eq!(splitter.next_frame(), None);
        splitter.push_bytes(&[0xAA, 0, 0, 0, 1, 7]);
        assert!(splitter.has_frame());
        assert_eq!(
            splitter.next_frame(),
            Some(Frame::Oversize { length: oversize })
        );
        assert_eq!(splitter.next_frame(), Some(Frame::Complete(vec![7])));
    }

    #[test]
    fn limit_is_65536_octets_not_one_more_or_less() {
        // Literals, so that a change of `MAX_FRAME_LEN` itself is noticed.
        let mut splitter = FrameSplitter::new();
        for length in [65_536u32, 65_537] {
            splitter.push_bytes(&length.to_be_bytes());
            splitter.push_bytes(&vec![1; length as usize]);
        }
        let Some(Frame::Complete(body)) = splitter.next_frame() else {
            panic!("65 536 octets must be delivered");
        };
        assert_eq!(body.len(), 65_536);
        assert_eq!(
            splitter.next_frame(),
            Some(Frame::Oversize { length: 65_537 })
        );
    }

    #[test]
    fn huge_length_does_not_buffer_the_body() {
        let mut splitter = FrameSplitter::new();
        splitter.push_bytes(&u32::MAX.to_be_bytes());
        splitter.push_bytes(&[0; 1000]);
        assert!(!splitter.has_frame());
        assert_eq!(splitter.next_frame(), None);
        assert_eq!(splitter.partial.len(), 0);
        splitter.push_bytes(&[0; 1000]);
        assert_eq!(splitter.next_frame(), None);
        assert_eq!(splitter.partial.len(), 0);
    }

    /// The largest `partial` may be: one frame, prefix included, that is not oversize.
    const PARTIAL_LIMIT: usize = LENGTH_PREFIX_LEN + MAX_FRAME_LEN as usize;

    #[test]
    fn has_frame_gated_caller_does_not_retain_an_oversize_body() {
        let mut splitter = FrameSplitter::new();
        splitter.push_bytes(&u32::MAX.to_be_bytes());
        let chunk = vec![0xAA; 16 * 1024];
        let mut pushed: u64 = 0;
        while pushed + (chunk.len() as u64) < u64::from(u32::MAX) {
            splitter.push_bytes(&chunk);
            pushed += chunk.len() as u64;
            // The gated caller never calls `next_frame` here.
            assert!(!splitter.has_frame());
            assert_eq!(splitter.partial.len(), 0);
            assert!(splitter.ready.is_empty());
        }
        // The rest of the declared length, then the next frame.
        let rest = vec![0xAA; usize::try_from(u64::from(u32::MAX) - pushed).unwrap()];
        splitter.push_bytes(&rest);
        splitter.push_bytes(&[0, 0, 0, 1, 7]);
        assert!(splitter.has_frame());
        assert_eq!(
            splitter.next_frame(),
            Some(Frame::Oversize { length: u32::MAX })
        );
        assert!(splitter.has_frame());
        assert_eq!(splitter.next_frame(), Some(Frame::Complete(vec![7])));
        assert!(!splitter.has_frame());
    }

    #[test]
    fn oversize_between_frames_is_in_stream_order_with_one_push() {
        let oversize = MAX_FRAME_LEN + 3;
        let mut stream = vec![0, 0, 0, 2, 1, 2];
        stream.extend_from_slice(&oversize.to_be_bytes());
        stream.extend_from_slice(&vec![0xBB; oversize as usize]);
        stream.extend_from_slice(&[0, 0, 0, 1, 9]);

        let mut splitter = FrameSplitter::new();
        splitter.push_bytes(&stream);
        assert_eq!(splitter.partial.len(), 0);
        let mut frames = Vec::new();
        while splitter.has_frame() {
            frames.push(splitter.next_frame().unwrap());
        }
        assert_eq!(
            frames,
            [
                Frame::Complete(vec![1, 2]),
                Frame::Oversize { length: oversize },
                Frame::Complete(vec![9]),
            ]
        );
        assert_eq!(splitter.next_frame(), None);
    }

    #[test]
    fn oversize_prefix_split_across_pushes_discards_the_body() {
        let oversize = MAX_FRAME_LEN + 1;
        let prefix = oversize.to_be_bytes();
        let mut splitter = FrameSplitter::new();
        splitter.push_bytes(&prefix[..2]);
        splitter.push_bytes(&prefix[2..]);
        splitter.push_bytes(&vec![0; oversize as usize - 1]);
        assert_eq!(splitter.partial.len(), 0);
        assert!(!splitter.has_frame());
        splitter.push_bytes(&[0]);
        assert!(splitter.has_frame());
        assert_eq!(
            splitter.next_frame(),
            Some(Frame::Oversize { length: oversize })
        );
    }

    /// The model test builds a stream of known frames and cuts it at random places.
    struct Item {
        /// Octets of the stream up to the end of this item.
        end: usize,
        expected: Frame,
    }

    fn random_stream(rng: &mut Prng) -> (Vec<u8>, Vec<Item>) {
        let mut stream = Vec::new();
        let mut items = Vec::new();
        let mut next_byte = 0u8;
        for _ in 0..12 {
            let kind = rng.below(40);
            let length = if kind == 0 {
                MAX_FRAME_LEN as usize + 1 + rng.below(300)
            } else if kind == 1 {
                MAX_FRAME_LEN as usize
            } else {
                rng.below(30)
            };
            let body: Vec<u8> = (0..length)
                .map(|_| {
                    next_byte = next_byte.wrapping_add(1);
                    next_byte
                })
                .collect();
            let length = u32::try_from(length).unwrap();
            stream.extend_from_slice(&length.to_be_bytes());
            stream.extend_from_slice(&body);
            let expected = if length > MAX_FRAME_LEN {
                Frame::Oversize { length }
            } else {
                Frame::Complete(body)
            };
            items.push(Item {
                end: stream.len(),
                expected,
            });
        }
        (stream, items)
    }

    #[test]
    fn model_random_streams_cut_at_random_points() {
        for seed in 1..=150u64 {
            let mut rng = Prng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let (stream, items) = random_stream(&mut rng);
            let mut splitter = FrameSplitter::new();
            // Odd seeds take frames only while `has_frame` is true, even seeds call `next_frame`
            // after every push whatever `has_frame` says.
            let gated = seed % 2 == 1;
            let mut pushed = 0;
            let mut taken = 0;
            while pushed < stream.len() {
                let wanted = if rng.below(10) == 0 {
                    1 + rng.below(70_000)
                } else {
                    1 + rng.below(40)
                };
                let end = stream.len().min(pushed + wanted);
                splitter.push_bytes(&stream[pushed..end]);
                pushed = end;
                assert!(splitter.partial.len() <= PARTIAL_LIMIT, "seed {seed}");
                loop {
                    let expected_has_frame = taken < items.len() && items[taken].end <= pushed;
                    assert_eq!(splitter.has_frame(), expected_has_frame, "seed {seed}");
                    if gated && !splitter.has_frame() {
                        break;
                    }
                    let frame = splitter.next_frame();
                    if !expected_has_frame {
                        assert_eq!(frame, None, "seed {seed}");
                        break;
                    }
                    assert_eq!(frame.as_ref(), Some(&items[taken].expected), "seed {seed}");
                    taken += 1;
                }
            }
            assert_eq!(taken, items.len(), "seed {seed}");
        }
    }
}
