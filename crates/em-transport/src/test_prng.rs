// SPDX-License-Identifier: Apache-2.0

//! The generator of the seeded model tests of the sans-I/O building blocks.

/// xorshift64: a tiny deterministic generator, so no dependency is needed.
pub(crate) struct Prng(pub(crate) u64);

impl Prng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..limit`; `limit` must be above 0.
    pub(crate) fn below(&mut self, limit: usize) -> usize {
        let limit = u64::try_from(limit).unwrap();
        usize::try_from(self.next() % limit).unwrap()
    }
}
