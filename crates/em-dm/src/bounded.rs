// SPDX-License-Identifier: Apache-2.0

//! Bounded collection for in-record arrays.
//!
//! A C record such as `em_network_ssid_info_t` keeps `num_akms` next to `akm[EM_MAX_AKMS]`.
//! [`BoundedVec`] folds both into one value: the count is the length, the array size is the
//! capacity, and adding an element at capacity is an error instead of a buffer overflow
//! (upstream defect 1). Storage is a `Vec`, so records holding one are never `Copy`.

use std::fmt;

/// Error returned when an element or a vector does not fit the capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    /// Capacity of the collection.
    pub capacity: usize,
    /// Number of elements the operation would have produced.
    pub requested: usize,
}

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} elements exceed the capacity of {}",
            self.requested, self.capacity
        )
    }
}

impl std::error::Error for CapacityError {}

/// A `Vec` that never holds more than `CAP` elements.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct BoundedVec<T, const CAP: usize>(Vec<T>);

impl<T, const CAP: usize> BoundedVec<T, CAP> {
    /// Maximum number of elements.
    pub const CAPACITY: usize = CAP;

    /// Creates an empty collection.
    #[must_use]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Takes ownership of `items`.
    ///
    /// # Errors
    ///
    /// [`CapacityError`] if `items` holds more than `CAP` elements; `items` is dropped.
    pub fn try_from_vec(items: Vec<T>) -> Result<Self, CapacityError> {
        if items.len() > CAP {
            return Err(CapacityError {
                capacity: CAP,
                requested: items.len(),
            });
        }
        Ok(Self(items))
    }

    /// Replaces all elements with `items`.
    ///
    /// # Errors
    ///
    /// [`CapacityError`] if `items` holds more than `CAP` elements; the current elements are kept
    /// and `items` is dropped.
    pub fn replace(&mut self, items: Vec<T>) -> Result<(), CapacityError> {
        *self = Self::try_from_vec(items)?;
        Ok(())
    }

    /// Appends `item`.
    ///
    /// # Errors
    ///
    /// [`CapacityError`] if the collection already holds `CAP` elements; it is left unchanged.
    pub fn push(&mut self, item: T) -> Result<(), CapacityError> {
        if self.0.len() >= CAP {
            return Err(CapacityError {
                capacity: CAP,
                requested: self.0.len() + 1,
            });
        }
        self.0.push(item);
        Ok(())
    }

    /// The elements.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Iterator over the elements.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    /// Number of elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True if there are no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Removes all elements.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl<T, const CAP: usize> Default for BoundedVec<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T, const CAP: usize> IntoIterator for &'a BoundedVec<T, CAP> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundedVec, CapacityError};

    #[test]
    fn push_stops_at_capacity_and_keeps_contents() {
        let mut list = BoundedVec::<u8, 2>::new();
        assert_eq!(list.push(1), Ok(()));
        assert_eq!(list.push(2), Ok(()));
        assert_eq!(
            list.push(3),
            Err(CapacityError {
                capacity: 2,
                requested: 3
            })
        );
        assert_eq!(list.as_slice(), &[1, 2]);
    }

    #[test]
    fn try_from_vec_accepts_exact_limit_and_rejects_one_more() {
        assert_eq!(
            BoundedVec::<u8, 3>::try_from_vec(vec![1, 2, 3]).map(|l| l.len()),
            Ok(3)
        );
        assert_eq!(
            BoundedVec::<u8, 3>::try_from_vec(vec![1, 2, 3, 4]),
            Err(CapacityError {
                capacity: 3,
                requested: 4
            })
        );
    }

    #[test]
    fn rejected_replace_keeps_the_old_elements() {
        let mut list = BoundedVec::<u8, 3>::try_from_vec(vec![1, 2]).unwrap();
        assert_eq!(list.replace(vec![9, 9, 9]), Ok(()));
        assert_eq!(list.as_slice(), &[9, 9, 9]);
        assert_eq!(
            list.replace(vec![0; 4]),
            Err(CapacityError {
                capacity: 3,
                requested: 4
            })
        );
        assert_eq!(list.as_slice(), &[9, 9, 9]);
    }

    #[test]
    fn zero_capacity_rejects_every_push() {
        let mut list = BoundedVec::<u8, 0>::new();
        assert_eq!(
            list.push(1),
            Err(CapacityError {
                capacity: 0,
                requested: 1
            })
        );
        assert!(list.is_empty());
    }

    #[test]
    fn clear_makes_room_again() {
        let mut list = BoundedVec::<u8, 1>::new();
        list.push(1).unwrap();
        list.clear();
        assert_eq!(list.push(2), Ok(()));
        assert_eq!((&list).into_iter().count(), 1);
        assert_eq!(list.iter().next(), Some(&2));
    }
}
