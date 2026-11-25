// SPDX-License-Identifier: Apache-2.0

//! A map kept as a `Vec<(K, V)>` sorted by key.
//!
//! Role cores must behave identically on every run, so they avoid hash maps, whose iteration
//! order varies. [`SortedVecMap`] iterates in key order, whatever order the entries were
//! inserted in. Lookups are binary searches; inserts and removals shift elements, so this
//! suits the small maps a role core keeps, not large ones.

use core::ops::Index;

/// A map stored as a `Vec<(K, V)>` sorted by key.
#[derive(Debug)]
pub struct SortedVecMap<K: Ord, V> {
    entries: Vec<(K, V)>,
}

impl<K: Ord, V> Default for SortedVecMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord, V> SortedVecMap<K, V> {
    /// An empty map. O(1), does not allocate.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Number of entries. O(1).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map has no entries. O(1).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Position of `key`: `Ok(index)` when present, `Err(insert_index)` when absent.
    fn position(&self, key: &K) -> Result<usize, usize> {
        self.entries
            .binary_search_by(|(entry_key, _)| entry_key.cmp(key))
    }

    /// The value for `key`. O(log n).
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        let index = self.position(key).ok()?;
        Some(&self.entries[index].1)
    }

    /// The value for `key`, mutably. O(log n).
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let index = self.position(key).ok()?;
        Some(&mut self.entries[index].1)
    }

    /// Whether `key` is present. O(log n).
    #[must_use]
    pub fn contains_key(&self, key: &K) -> bool {
        self.position(key).is_ok()
    }

    /// Stores `value` under `key` and returns the value it replaced, if any, as
    /// `HashMap::insert` does. O(log n) to find the spot, O(n) when a new key shifts elements.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        match self.position(&key) {
            Ok(index) => Some(core::mem::replace(&mut self.entries[index].1, value)),
            Err(index) => {
                self.entries.insert(index, (key, value));
                None
            }
        }
    }

    /// Removes `key` and returns its value. O(log n) to find it, O(n) to shift elements.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let index = self.position(key).ok()?;
        Some(self.entries.remove(index).1)
    }

    /// The value for `key`; when absent, first inserts the result of `make_value`.
    /// O(log n) to find the spot, O(n) when a new key shifts elements.
    pub fn get_or_insert_with(&mut self, key: K, make_value: impl FnOnce() -> V) -> &mut V {
        let index = match self.position(&key) {
            Ok(index) => index,
            Err(index) => {
                self.entries.insert(index, (key, make_value()));
                index
            }
        };
        &mut self.entries[index].1
    }

    /// Entries in ascending key order. O(1) to create, O(n) to walk.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

/// Indexing panics when the key is missing, as `HashMap`'s `Index` does. O(log n).
impl<K: Ord, V> Index<&K> for SortedVecMap<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        self.get(key).expect("key not found in SortedVecMap")
    }
}

#[cfg(test)]
mod tests {
    use super::SortedVecMap;

    fn keys_in_order(map: &SortedVecMap<u32, &'static str>) -> Vec<u32> {
        map.iter().map(|(key, _)| *key).collect()
    }

    #[test]
    fn iteration_order_does_not_depend_on_insert_order() {
        let mut ascending = SortedVecMap::new();
        for key in [1, 2, 3, 4] {
            ascending.insert(key, "v");
        }
        let mut shuffled = SortedVecMap::new();
        for key in [3, 1, 4, 2] {
            shuffled.insert(key, "v");
        }
        assert_eq!(keys_in_order(&ascending), [1, 2, 3, 4]);
        assert_eq!(keys_in_order(&shuffled), [1, 2, 3, 4]);
    }

    #[test]
    fn insert_replaces_and_returns_the_old_value() {
        let mut map = SortedVecMap::new();
        assert_eq!(map.insert(5, "old"), None);
        assert_eq!(map.insert(5, "new"), Some("old"));
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&5), Some(&"new"));
    }

    #[test]
    fn remove_returns_the_value_once() {
        let mut map = SortedVecMap::new();
        map.insert(1, "a");
        map.insert(2, "b");
        assert_eq!(map.remove(&1), Some("a"));
        assert_eq!(map.remove(&1), None);
        assert_eq!(keys_in_order(&map), [2]);
    }

    #[test]
    fn get_or_insert_with_keeps_a_present_value() {
        let mut map = SortedVecMap::new();
        map.insert(1, "kept");
        let value = map.get_or_insert_with(1, || panic!("must not be called"));
        assert_eq!(*value, "kept");
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn get_or_insert_with_inserts_an_absent_value() {
        let mut map = SortedVecMap::new();
        map.insert(2, "two");
        *map.get_or_insert_with(1, || "one") = "uno";
        assert_eq!(map.get(&1), Some(&"uno"));
        assert_eq!(keys_in_order(&map), [1, 2]);
    }

    #[test]
    fn get_mut_changes_the_stored_value() {
        let mut map = SortedVecMap::new();
        map.insert(1, "a");
        *map.get_mut(&1).unwrap() = "b";
        assert_eq!(map[&1], "b");
        assert!(map.get_mut(&2).is_none());
    }

    #[test]
    fn contains_key_len_and_is_empty() {
        let mut map = SortedVecMap::default();
        assert!(map.is_empty());
        assert!(!map.contains_key(&1));
        map.insert(1, "a");
        assert!(!map.is_empty());
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&1));
        assert!(!map.contains_key(&2));
    }

    #[test]
    #[should_panic(expected = "key not found")]
    fn index_panics_on_a_missing_key() {
        let map: SortedVecMap<u32, &str> = SortedVecMap::new();
        let _ = map[&1];
    }
}
