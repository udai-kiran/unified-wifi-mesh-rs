// SPDX-License-Identifier: Apache-2.0

//! `f32` member of a record, comparable and hashable by its bit pattern.

use std::hash::{Hash, Hasher};

/// C `float`. Equality and hashing use the bit pattern, so records holding one can still derive
/// `Eq` and `Hash`: `NaN` equals itself, and `0.0` differs from `-0.0`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Float32(f32);

impl Float32 {
    /// Wraps `value`.
    #[must_use]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// The value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl PartialEq for Float32 {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Float32 {}

impl Hash for Float32 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::Float32;

    #[test]
    fn compares_by_bit_pattern() {
        assert_eq!(Float32::new(1.5), Float32::new(1.5));
        assert_eq!(Float32::new(f32::NAN), Float32::new(f32::NAN));
        assert_ne!(Float32::new(0.0), Float32::new(-0.0));
        assert!((Float32::new(2.25).get() - 2.25).abs() < f32::EPSILON);
    }
}
