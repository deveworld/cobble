//! Maps and sets whose changes can be undone back to a mark, so a function
//! body or an isolated capture can change the symbol tables and have them
//! restored afterwards without copying them. Copying them for every
//! function made large programs quadratic.

use std::borrow::Borrow;
use std::collections::hash_map::{self, HashMap};
use std::hash::Hash;
use std::ops::Deref;

/// Where a scope opened by [`ScopedMap::open`] starts in the undo log.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mark(usize);

/// A `HashMap` that records the earlier value of each key it changes while
/// a scope is open. Reads go through `Deref`; every change goes through the
/// methods here, so it can be undone.
#[derive(Clone, Debug)]
pub(crate) struct ScopedMap<K, V> {
    map: HashMap<K, V>,
    /// Earlier values of changed keys, oldest first, while a scope is open.
    undo: Vec<(K, Option<V>)>,
    open_scopes: usize,
}

impl<K, V> Default for ScopedMap<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            undo: Vec::new(),
            open_scopes: 0,
        }
    }
}

impl<K, V> Deref for ScopedMap<K, V> {
    type Target = HashMap<K, V>;

    fn deref(&self) -> &HashMap<K, V> {
        &self.map
    }
}

impl<K: Hash + Eq + Clone, V: Clone> ScopedMap<K, V> {
    fn record(&mut self, key: &K) {
        if self.open_scopes > 0 {
            self.undo.push((key.clone(), self.map.get(key).cloned()));
        }
    }

    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.record(&key);
        self.map.insert(key, value)
    }

    pub(crate) fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let (key, value) = self.map.remove_entry(key)?;
        if self.open_scopes > 0 {
            self.undo.push((key, Some(value.clone())));
        }
        Some(value)
    }

    pub(crate) fn clear(&mut self) {
        if self.open_scopes > 0 {
            for (key, value) in self.map.drain() {
                self.undo.push((key, Some(value)));
            }
        } else {
            self.map.clear();
        }
    }

    /// Opens a scope: changes from here on can be undone with
    /// [`Self::undo_to`] or kept with [`Self::keep_from`].
    pub(crate) fn open(&mut self) -> Mark {
        self.open_scopes += 1;
        Mark(self.undo.len())
    }

    /// Undoes every change since `mark` and closes its scope.
    pub(crate) fn undo_to(&mut self, mark: Mark) {
        while self.undo.len() > mark.0 {
            let (key, value) = self.undo.pop().expect("undo log holds the change");
            match value {
                Some(value) => self.map.insert(key, value),
                None => self.map.remove(&key),
            };
        }
        self.close();
    }

    /// The map as it was when `mark` was opened, leaving this one as it is.
    pub(crate) fn snapshot_at(&self, mark: Mark) -> ScopedMap<K, V> {
        let mut map = self.map.clone();
        for (key, value) in self.undo[mark.0..].iter().rev() {
            match value {
                Some(value) => map.insert(key.clone(), value.clone()),
                None => map.remove(key),
            };
        }
        map.into_iter().collect()
    }

    /// Keeps the changes since `mark` and closes its scope. An outer scope
    /// can still undo them.
    pub(crate) fn keep_from(&mut self, _mark: Mark) {
        self.close();
    }

    fn close(&mut self) {
        self.open_scopes = self.open_scopes.saturating_sub(1);
        if self.open_scopes == 0 {
            self.undo.clear();
        }
    }
}

impl<K: Hash + Eq + Clone, V: Clone> Extend<(K, V)> for ScopedMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, items: I) {
        for (key, value) in items {
            self.insert(key, value);
        }
    }
}

impl<K: Hash + Eq, V> FromIterator<(K, V)> for ScopedMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(items: I) -> Self {
        Self {
            map: items.into_iter().collect(),
            undo: Vec::new(),
            open_scopes: 0,
        }
    }
}

/// A `HashSet` like [`ScopedMap`].
#[derive(Clone, Debug, Default)]
pub(crate) struct ScopedSet<K> {
    map: ScopedMap<K, ()>,
}

impl<K: Hash + Eq + Clone> ScopedSet<K> {
    pub(crate) fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.contains_key(key)
    }

    pub(crate) fn insert(&mut self, key: K) -> bool {
        if self.map.contains_key(&key) {
            return false;
        }
        self.map.insert(key, ());
        true
    }

    pub(crate) fn remove<Q>(&mut self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.remove(key).is_some()
    }

    pub(crate) fn iter(&self) -> hash_map::Keys<'_, K, ()> {
        self.map.keys()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
    }

    pub(crate) fn open(&mut self) -> Mark {
        self.map.open()
    }

    pub(crate) fn undo_to(&mut self, mark: Mark) {
        self.map.undo_to(mark);
    }

    pub(crate) fn keep_from(&mut self, mark: Mark) {
        self.map.keep_from(mark);
    }
}

impl<K: Hash + Eq + Clone> Extend<K> for ScopedSet<K> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, items: I) {
        for key in items {
            self.insert(key);
        }
    }
}

impl<'a, K: Hash + Eq + Clone> IntoIterator for &'a ScopedSet<K> {
    type Item = &'a K;
    type IntoIter = hash_map::Keys<'a, K, ()>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<K: Hash + Eq + Clone> FromIterator<K> for ScopedSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(items: I) -> Self {
        Self {
            map: items.into_iter().map(|key| (key, ())).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_inserted_removed_and_cleared_keys() {
        let mut map: ScopedMap<String, i32> = ScopedMap::default();
        map.insert("a".into(), 1);
        map.insert("b".into(), 2);
        let outer = map.open();
        map.insert("a".into(), 10);
        map.remove("b");
        let inner = map.open();
        map.insert("c".into(), 3);
        map.clear();
        map.undo_to(inner);
        assert_eq!(map.get("a"), Some(&10));
        assert_eq!(map.get("b"), None);
        assert_eq!(map.get("c"), None);
        let kept = map.open();
        map.insert("d".into(), 4);
        map.keep_from(kept);
        map.undo_to(outer);
        assert_eq!(map.get("a"), Some(&1));
        assert_eq!(map.get("b"), Some(&2));
        assert_eq!(map.get("d"), None);
    }

    #[test]
    fn a_set_undoes_like_a_map() {
        let mut set: ScopedSet<String> = ScopedSet::default();
        set.insert("a".into());
        let mark = set.open();
        set.remove("a");
        set.insert("b".into());
        set.undo_to(mark);
        assert!(set.contains("a") && !set.contains("b"));
    }
}
