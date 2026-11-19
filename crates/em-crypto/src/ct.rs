// SPDX-License-Identifier: Apache-2.0

//! Constant-time comparison for tags, MICs and authenticators.

use subtle::{Choice, ConstantTimeEq};

/// True when `a` and `b` have the same length and the same bytes.
///
/// The comparison time does not depend on where the inputs differ. Lengths are public, so a
/// length mismatch returns early; callers must still validate the expected length themselves
/// before they truncate or copy a received value.
#[must_use]
pub fn eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // `zip` stops at the shorter input, which is why the length check above is required for
    // correctness. Each byte comparison is a `Choice`, so the compiler cannot short-circuit it.
    let mut equal = Choice::from(1);
    for (x, y) in a.iter().zip(b) {
        equal &= x.ct_eq(y);
    }
    equal.into()
}
