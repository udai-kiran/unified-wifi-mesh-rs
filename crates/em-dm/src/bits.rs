// SPDX-License-Identifier: Apache-2.0

//! Multi-bit C bit-field values.
//!
//! A 1-bit bit-field is a `bool` in the records. A wider one is a [`UBits`], whose constructor
//! and setter reject values that do not fit the declared width, so a record can never hold a
//! value the C bit-field could not.

use std::fmt;

/// Error returned when a value does not fit the bit-field width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitsError {
    /// Width of the bit-field in bits.
    pub width: u32,
    /// The rejected value.
    pub value: u32,
}

impl fmt::Display for BitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "value {} does not fit in {} bits",
            self.value, self.width
        )
    }
}

impl std::error::Error for BitsError {}

/// An unsigned value of `WIDTH` bits (2 to 24 in the records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UBits<const WIDTH: u32>(u32);

impl<const WIDTH: u32> UBits<WIDTH> {
    /// Width of the bit-field in bits.
    pub const WIDTH: u32 = WIDTH;

    /// Creates a value.
    ///
    /// # Errors
    ///
    /// [`BitsError`] if `value >= 2^WIDTH`.
    pub const fn new(value: u32) -> Result<Self, BitsError> {
        if value >> WIDTH != 0 {
            return Err(BitsError {
                width: WIDTH,
                value,
            });
        }
        Ok(Self(value))
    }

    /// Replaces the value; on error the old value is kept.
    ///
    /// # Errors
    ///
    /// [`BitsError`] if `value >= 2^WIDTH`.
    pub fn set(&mut self, value: u32) -> Result<(), BitsError> {
        *self = Self::new(value)?;
        Ok(())
    }

    /// The value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{BitsError, UBits};

    #[test]
    fn accepts_largest_value_and_rejects_next() {
        assert_eq!(UBits::<2>::new(3).map(UBits::get), Ok(3));
        assert_eq!(UBits::<2>::new(4), Err(BitsError { width: 2, value: 4 }));
        assert_eq!(
            UBits::<24>::new(0x00ff_ffff).map(UBits::get),
            Ok(0x00ff_ffff)
        );
        assert_eq!(
            UBits::<24>::new(0x0100_0000),
            Err(BitsError {
                width: 24,
                value: 0x0100_0000
            })
        );
    }

    #[test]
    fn rejected_set_keeps_old_value() {
        let mut bits = UBits::<3>::new(5).unwrap();
        assert!(bits.set(8).is_err());
        assert_eq!(bits.get(), 5);
        assert!(bits.set(7).is_ok());
        assert_eq!(bits.get(), 7);
    }
}
