// SPDX-License-Identifier: Apache-2.0

//! Frames waiting for a free slot in the transport's send channel (0012 D9).

use std::collections::VecDeque;

use em_transport::OutboundSdu;

/// Frames waiting for `reserve()` are kept at most this many.
pub(crate) const MAX_PENDING_FRAMES: usize = 64;

/// A bounded queue that drops its oldest frame when full and counts the drops.
#[derive(Debug, Default)]
pub(crate) struct PendingFrames {
    queue: VecDeque<OutboundSdu>,
    dropped: u64,
}

impl PendingFrames {
    pub(crate) fn push(&mut self, sdu: OutboundSdu) {
        if self.queue.len() >= MAX_PENDING_FRAMES {
            self.queue.pop_front();
            self.dropped += 1;
            tracing::warn!(
                dropped_total = self.dropped,
                "pending frames full, dropped the oldest"
            );
        }
        self.queue.push_back(sdu);
    }

    pub(crate) fn pop(&mut self) -> Option<OutboundSdu> {
        self.queue.pop_front()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }

    #[cfg(test)]
    pub(crate) const fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use em_transport::Sdu;
    use em_types::MacAddr;

    use super::{MAX_PENDING_FRAMES, OutboundSdu, PendingFrames};

    fn sdu(session: u64) -> OutboundSdu {
        OutboundSdu {
            session,
            sdu: Sdu {
                local: MacAddr::new([2, 0, 0, 0, 0, 1]),
                remote: MacAddr::IEEE1905_MULTICAST,
                cmdu: Vec::new(),
            },
        }
    }

    #[test]
    fn a_full_queue_drops_the_oldest_and_counts() {
        let mut pending = PendingFrames::default();
        let total = MAX_PENDING_FRAMES as u64 + 6;
        for session in 0..total {
            pending.push(sdu(session));
        }
        assert_eq!(pending.len(), MAX_PENDING_FRAMES);
        assert_eq!(pending.dropped(), 6);
        assert_eq!(pending.pop().unwrap().session, 6);
    }
}
