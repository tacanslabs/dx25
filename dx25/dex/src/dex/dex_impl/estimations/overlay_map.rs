use crate::dex::{KeyAt, Map, MapRemoveKey, OrderedMap, Result};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::Bound;

/// `OrderedOverlayMap` tracks modifications on top of an immutable `OrderedMap`.
///
/// It behaves as a mutable copy of an `OrderedMap`, but does not make a deep copy.
/// Instead, it keeps a reference to the origianl map and tracks modifications on top of it.
/// It is useful when making a copy is either not possible or too expensive.
///
/// Parameters:
///  - 'm: lifetime of the reference to the original (persistent) `OrderedMap`
///  - M: original (persistent) `OrderedMap` type
pub struct OrderedOverlayMap<'m, M: Map>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    // Reference to the original OrderedMap map.
    // The reference is reset to `None` when the OrderedOverlayMap is cleared.
    persistent: Option<&'m M>,

    // Modifications on top of the original (persistent) OrderedMap.
    // `None` indicates that the item at such key was deleted.
    // `Some` holds updated value.
    transient: BTreeMap<<M as Map>::Key, Option<<M as Map>::Value>>,

    // Length of the resulting OrderedOverlayMap (number of keys with a value behind).
    len: usize,
}

#[allow(unused)]
pub struct OverlayMapIter<'m, M: Map>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    map: OrderedOverlayMap<'m, M>,
}

impl<'m, M: Map> Iterator for OverlayMapIter<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    type Item = (
        <OrderedOverlayMap<'m, M> as Map>::KeyRef<'m>,
        <OrderedOverlayMap<'m, M> as Map>::ValueRef<'m>,
    );
    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

impl<'m, M: Map> OrderedOverlayMap<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    /// Construct a new `OrderedOverlayMap`
    pub fn new(persistent: &'m M) -> Self {
        Self {
            persistent: Some(persistent),
            transient: BTreeMap::new(),
            len: persistent.len(),
        }
    }
}

impl<'m, M: Map> Default for OrderedOverlayMap<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    /// Construct a new `OrderedOverlayMap`
    fn default() -> Self {
        Self {
            persistent: None,
            transient: BTreeMap::new(),
            len: 0,
        }
    }
}

impl<'m, M: Map> Map for OrderedOverlayMap<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    type Key = <M as Map>::Key;
    type Value = <M as Map>::Value;
    type KeyRef<'k> = &'k Self::Key where Self: 'k;
    type ValueRef<'v> = &'v Self::Value where Self: 'v;
    type Iter<'i> = OverlayMapIter<'i, Self> where Self: 'i;

    fn clear(&mut self) {
        self.persistent = None;
        self.transient.clear();
        self.len = 0;
    }

    fn contains_key(&self, key: &Self::Key) -> bool {
        match self.transient.get(key) {
            Some(Some(_)) => true,
            Some(None) => false,
            None => self
                .persistent
                .map_or(false, |persistent| persistent.contains_key(key)),
        }
    }

    fn insert(&mut self, key: Self::Key, value: Self::Value) {
        let old_transient_value = self.transient.insert(key.clone(), Some(value));
        // Check if the length increases:
        let is_inserted_new_item = match old_transient_value {
            // item at such was transiently inserted or modified
            Some(Some(_)) => false,
            // item at such was transiently deleted
            Some(None) => true,
            // there were no modifications on top of the persistent collection
            None => !self
                .persistent
                .map_or(false, |persistent| persistent.contains_key(&key)),
        };
        self.len += usize::from(is_inserted_new_item);
    }

    fn inspect<R, F: FnOnce(&Self::Value) -> R>(
        &self,
        key: &Self::Key,
        inspect_fn: F,
    ) -> Option<R> {
        match self.transient.get(key) {
            // transient collection contains newer value
            Some(Some(value_ref)) => Some(inspect_fn(value_ref)),
            // item at such was transiently deleted
            Some(None) => None,
            // there were no modifications on top of the persistent collection
            None => self
                .persistent
                .and_then(|persistent| persistent.inspect(key, inspect_fn)),
        }
    }

    fn is_empty(&self) -> bool {
        assert!(self.transient.is_empty());
        assert!(self.persistent.map_or(true, Map::is_empty));
        self.len == 0
    }

    fn iter(&self) -> Self::Iter<'_> {
        todo!()
    }

    fn len(&self) -> usize {
        self.len
    }

    fn update<R, F: FnOnce(&mut Self::Value) -> Result<R>>(
        &mut self,
        key: &Self::Key,
        update_fn: F,
    ) -> Option<Result<R>> {
        match self.transient.get_mut(key) {
            // transient collection contains newer value
            Some(Some(value_ref)) => {
                let mut value = value_ref.clone();
                let result = update_fn(&mut value);
                // update the value only if update_fn succeeded:
                if result.is_ok() {
                    self.transient.insert(key.clone(), Some(value));
                }
                Some(result)
            }
            // item at such key was transiently deleted
            Some(None) => None,
            // there were no modifications on top of the persistent collection, except possible 'clear'
            None => match self.persistent {
                // OrderedOverlayMap was cleared
                None => None,
                // no modifications
                Some(persistent) => {
                    if let Some(mut value) =
                        persistent.inspect(key, |value_ref| (*value_ref).clone())
                    {
                        let result = update_fn(&mut value);
                        // update the value only if update_fn succeeded
                        if result.is_ok() {
                            self.transient.insert(key.clone(), Some(value));
                        }
                        Some(result)
                    } else {
                        None
                    }
                }
            },
        }
    }

    fn update_or_insert<R, F, U>(
        &mut self,
        key: &Self::Key,
        factory_fn: F,
        update_fn: U,
    ) -> Result<R>
    where
        F: FnOnce() -> Result<Self::Value>,
        U: FnOnce(&mut Self::Value, bool) -> Result<R>,
    {
        match self.transient.get_mut(key) {
            // transient collection contains newer value
            Some(Some(value_ref)) => {
                let mut value = value_ref.clone();
                let result = update_fn(&mut value, true);
                if result.is_ok() {
                    self.transient.insert(key.clone(), Some(value));
                }
                result
            }
            // item at such key was transiently deleted
            Some(None) => self.create_update_insert(key, factory_fn, update_fn),
            // there were no modifications on top of the persistent collection, except possible 'clear'
            None => match self.persistent {
                // OrderedOverlayMap was cleared
                None => self.create_update_insert(key, factory_fn, update_fn),
                // no modifications
                Some(persistent) => {
                    if let Some(mut value) = persistent.inspect(key, |value| (*value).clone()) {
                        // item exists in persistent collection
                        let result = update_fn(&mut value, true);
                        // update the value only if update_fn succeeded
                        if result.is_ok() {
                            self.transient.insert(key.clone(), Some(value));
                        }
                        result
                    } else {
                        // no item in persistent collection
                        self.create_update_insert(key, factory_fn, update_fn)
                    }
                }
            },
        }
    }
}

impl<'m, M: Map> OrderedOverlayMap<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    fn create_update_insert<R, F, U>(
        &mut self,
        key: &<M as Map>::Key,
        factory_fn: F,
        update_fn: U,
    ) -> Result<R>
    where
        F: FnOnce() -> Result<<M as Map>::Value>,
        U: FnOnce(&mut <M as Map>::Value, bool) -> Result<R>,
    {
        let mut value = factory_fn()?;
        let result = update_fn(&mut value, false);
        // update the value only if update_fn succeeded
        if result.is_ok() {
            self.transient.insert(key.clone(), Some(value));
            self.len += 1;
        }
        result
    }
}

impl<'m, M: MapRemoveKey> MapRemoveKey for OrderedOverlayMap<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    fn remove(&mut self, key: &Self::Key) {
        if let Some(value) = self.transient.get_mut(key) {
            if value.is_some() {
                self.len -= 1;
                *value = None;
            }
        } else if self
            .persistent
            .map_or(false, |persistent| persistent.contains_key(key))
        {
            self.transient.insert(key.clone(), None);
            self.len -= 1;
        }
    }
}

enum OverlayMapKey<T: std::cmp::Eq + std::cmp::Ord + Clone> {
    Persistent(T),
    Transient(T),
    None,
}

impl<'m, M: OrderedMap> OrderedOverlayMap<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    fn find_key_at(&self, key_at: KeyAt<&<M as Map>::Key>) -> OverlayMapKey<<M as Map>::Key> {
        let mut next_persistent_key: Option<<M as Map>::Key> =
            if let Some(persistent) = self.persistent {
                persistent.inspect_at(key_at, |key, _value| key.clone())
            } else {
                None
            };

        let get_next_persistent_key =
            |prev_persistent_key: &<M as Map>::Key| -> Option<<M as Map>::Key> {
                if let Some(persistent) = self.persistent {
                    match key_at {
                        KeyAt::Min | KeyAt::Above(_) => {
                            persistent.inspect_above(prev_persistent_key, |key, _value| key.clone())
                        }
                        KeyAt::Max | KeyAt::Below(_) => {
                            persistent.inspect_below(prev_persistent_key, |key, _value| key.clone())
                        }
                    }
                } else {
                    None
                }
            };

        let mut transient_range = match key_at {
            KeyAt::Min | KeyAt::Max => self.transient.range((
                Bound::<<M as Map>::Key>::Unbounded,
                Bound::<<M as Map>::Key>::Unbounded,
            )),
            KeyAt::Above(key) => self
                .transient
                .range((Bound::Excluded(key), Bound::Unbounded)),
            KeyAt::Below(key) => self
                .transient
                .range((Bound::Unbounded, Bound::Excluded(key))),
        };

        let mut get_next_transient_key = || -> Option<(&<M as Map>::Key, bool)> {
            match key_at {
                KeyAt::Min | KeyAt::Above(_) => transient_range.next(),
                KeyAt::Max | KeyAt::Below(_) => transient_range.next_back(),
            }
            .map(|(key, value)| (key, value.is_none()))
        };

        // The second tuple element (bool) indicates whether the item marks a deletion
        let mut next_transient_key: Option<(&<M as Map>::Key, bool)> = get_next_transient_key();

        // Compare keys according to specified `key_at`:
        // returns Less if the left key is closer (fits better) to the specified `key_at`
        let compare_closer =
            |key_left: &<M as Map>::Key, key_right: &<M as Map>::Key| -> Ordering {
                match key_at {
                    KeyAt::Min | KeyAt::Above(_) => key_left.cmp(key_right),
                    KeyAt::Max | KeyAt::Below(_) => key_right.cmp(key_left),
                }
            };

        loop {
            match (next_persistent_key.as_ref(), next_transient_key) {
                (Some(persistent_key), Some((transient_key, false))) => {
                    match compare_closer(persistent_key, transient_key) {
                        // item in persistent collection is closer to the specified key
                        Ordering::Less => break OverlayMapKey::Persistent(persistent_key.clone()),
                        // item in the transient collection is closer, or equally close and newer.
                        Ordering::Equal | Ordering::Greater => {
                            break OverlayMapKey::Transient(transient_key.clone())
                        }
                    }
                }

                // transisent item was deleted
                (Some(persistent_key), Some((transient_key, true))) => {
                    match compare_closer(persistent_key, transient_key) {
                        // item in persistent collection is closer to the specified key
                        Ordering::Less => {
                            break OverlayMapKey::Persistent(persistent_key.clone());
                        }
                        // the item with this key was deleted
                        Ordering::Equal => {
                            next_persistent_key = get_next_persistent_key(persistent_key);
                            next_transient_key = get_next_transient_key();
                            continue;
                        }
                        // the item in transient collection marks a deletion => check the next one
                        Ordering::Greater => {
                            next_transient_key = get_next_transient_key();
                            continue;
                        }
                    }
                }

                // no items left in transient collection => take one from persistent
                (Some(persistent_key), None) => {
                    break OverlayMapKey::Persistent(persistent_key.clone());
                }

                // no items left in persistent collection => take one from transient
                (None, Some((transient_key, false))) => {
                    break OverlayMapKey::Transient(transient_key.clone())
                }

                // no items left in persistent collection, while the item in transient collection
                // marks a deletion => check the next one
                (None, Some((_transient_key, true))) => {
                    next_transient_key = get_next_transient_key();
                    continue;
                }

                // no items left at all
                (None, None) => {
                    break OverlayMapKey::None;
                }
            }
        }
    }
}

impl<'m, M: OrderedMap> OrderedMap for OrderedOverlayMap<'m, M>
where
    <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
    <M as Map>::Value: Clone,
{
    fn inspect_at<R, F: FnOnce(&Self::Key, &Self::Value) -> R>(
        &self,
        at: KeyAt<&Self::Key>,
        inspect_fn: F,
    ) -> Option<R> {
        match self.find_key_at(at) {
            OverlayMapKey::Transient(key_found) => {
                let value = self.transient
                    .get(&key_found)
                    .expect("The key was already found in transient collection.")
                    .as_ref()
                    .expect("The key was already found in transient collection, and the item was a Some.");
                Some(inspect_fn(&key_found, value))
            }
            OverlayMapKey::Persistent(key_found) => Some(
                self.persistent
                    .expect("The key was already found in persistent collection.")
                    .inspect(&key_found, |value| inspect_fn(&key_found, value))
                    .expect("The key was already found in persistent collection."),
            ),
            OverlayMapKey::None => None,
        }
    }

    fn update_at<R, F: FnOnce(&Self::Key, &mut Self::Value) -> Result<R>>(
        &mut self,
        at: KeyAt<&Self::Key>,
        update_fn: F,
    ) -> Option<Result<R>> {
        match self.find_key_at(at) {
            OverlayMapKey::Transient(key_found) => {
                let value_ref: &mut Self::Value = self.transient
                    .get_mut(&key_found)
                    .expect("The key was already found in transient collection.")
                    .as_mut()
                    .expect("The key was already found in transient collection, and the item was a Some.");
                let mut value_updated = value_ref.clone();
                let result = update_fn(&key_found, &mut value_updated);
                if result.is_ok() {
                    *value_ref = value_updated;
                }
                Some(result)
            }
            OverlayMapKey::Persistent(key_found) => {
                let mut value = self
                    .persistent
                    .expect("The key was already found in persistent collection.")
                    .inspect(&key_found, |value| value.clone())
                    .expect("The key was already found in persistent collection.");
                let result = update_fn(&key_found, &mut value);
                if result.is_ok() {
                    debug_assert!(!self.transient.contains_key(&key_found));
                    self.transient.insert(key_found, Some(value));
                }
                Some(result)
            }

            OverlayMapKey::None => None,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::dex::test_utils::collections::{OrderedMap, TypedStorage};
    use crate::dex::test_utils::{TestDe, TestSer};
    use crate::dex::traits::{Map, MapRemoveKey as _, OrderedMap as _};
    use crate::dex::{Error, KeyAt, KeyAt::Above, KeyAt::Below, KeyAt::Max, KeyAt::Min, Result};
    use crate::error_here;
    use assert_matches::assert_matches;

    use super::OrderedOverlayMap;

    /// Helper to construct a persistent `OrderedMap`
    fn make_persistent_map<K, V, I>(items: I) -> OrderedMap<K, V>
    where
        K: TestSer + TestDe + std::cmp::Eq + std::cmp::Ord + Clone,
        V: TestSer + TestDe + Clone,
        I: IntoIterator<Item = (K, V)>,
    {
        let storage = TypedStorage::new();
        let mut map: OrderedMap<K, V> = storage.new_ord_map();
        for (k, v) in items {
            map.insert(k, v);
        }
        map
    }

    impl<'m, M: Map> OrderedOverlayMap<'m, M>
    where
        <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone,
        <M as Map>::Value: Clone,
    {
        /// Shortcut to get items from `overlay_map` in tests
        fn get(&self, key: &<M as Map>::Key) -> Option<<M as Map>::Value>
        where
            <M as Map>::Value: Clone,
        {
            self.inspect(key, <M as Map>::Value::clone)
        }
    }

    /// A trivial `factory_fn`
    #[allow(clippy::unnecessary_wraps)]
    fn make_zero() -> Result<i32> {
        Ok(0)
    }

    /// Shortcut to construct a dex-native error
    fn make_error() -> Error {
        error_here!(crate::dex::ErrorKind::InternalLogicError)
    }

    /// Helper to test `inspect_at` functions
    #[allow(clippy::needless_pass_by_value)]
    fn assert_inspect_at_yields<M>(
        overlay: &OrderedOverlayMap<M>,
        at: KeyAt<<M as Map>::Key>,
        expected: Option<(<M as Map>::Key, <M as Map>::Value)>,
    ) where
        M: crate::dex::traits::OrderedMap,
        <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone + std::fmt::Debug,
        <M as Map>::Value: Clone + std::fmt::Debug + std::cmp::Eq,
    {
        let at: KeyAt<&<M as Map>::Key> = match at {
            Above(ref key) => Above(key),
            Below(ref key) => Below(key),
            Min => Min,
            Max => Max,
        };

        if let Some((key_expected, val_expected)) = expected {
            let res = overlay.inspect_at(at, |key, val| {
                assert_eq!(key, &key_expected);
                assert_eq!(val, &val_expected);
            });
            assert!(res.is_some());
        } else {
            let res = overlay.inspect_at(at, |_key, _val| {
                panic!("inspect_fn is expeted not to be called")
            });
            assert!(res.is_none());
        }
    }

    fn test_update_at<M, F, R>(
        overlay: &mut OrderedOverlayMap<M>,
        at: KeyAt<<M as Map>::Key>,
        expected_initial: Option<(<M as Map>::Key, <M as Map>::Value)>,
        update_fn: F,
    ) where
        M: crate::dex::traits::OrderedMap,
        <M as Map>::Key: std::cmp::Eq + std::cmp::Ord + Clone + std::fmt::Debug,
        <M as Map>::Value: Clone + std::fmt::Debug + std::cmp::Eq,
        F: Fn(&mut <M as Map>::Value) -> Result<R> + Clone,
        R: std::cmp::Eq + Clone + std::fmt::Debug,
    {
        let len = overlay.len();

        assert_inspect_at_yields(overlay, at.clone(), expected_initial.clone());

        let (expected_final, expected_result) =
            match expected_initial.clone().map(|(key, initial_value)| {
                let mut updated_value = initial_value.clone();
                let expected_result = update_fn(&mut updated_value);
                if expected_result.is_ok() {
                    (key, updated_value, expected_result)
                } else {
                    (key, initial_value, expected_result)
                }
            }) {
                Some((key, value, result)) => (Some((key, value)), Some(result)),
                None => (None, None),
            };

        let at_ref: KeyAt<&<M as Map>::Key> = match at {
            Above(ref key) => Above(key),
            Below(ref key) => Below(key),
            Min => Min,
            Max => Max,
        };

        let actual_result = overlay.update_at(at_ref, |key, value| {
            let (expected_key, expected_value) = expected_initial.unwrap();
            assert_eq!(*key, expected_key);
            assert_eq!(*value, expected_value);
            update_fn(value)
        });
        // Can't derive(PartialEq) for the ErrorKind::Custom, so compare results without comparing error kind:
        assert!(
            (actual_result.is_none() && expected_result.is_none())
                || (actual_result.as_ref().unwrap().is_err()
                    && expected_result.as_ref().unwrap().is_err())
                || (actual_result.unwrap().unwrap() == expected_result.unwrap().unwrap())
        );

        assert_inspect_at_yields(overlay, at, expected_final);

        assert_eq!(len, overlay.len());
    }

    #[allow(clippy::unnecessary_wraps)]
    fn mul_3_ok(v: &mut i32) -> Result<String> {
        *v *= 3;
        Ok(String::from("success"))
    }

    fn mul_3_fail(v: &mut i32) -> Result<String> {
        *v *= 3;
        Err(make_error())
    }

    #[test]
    fn insert_non_existing() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(2, "two".into());
        assert_eq!(overlay.get(&2).unwrap(), "two");
    }

    #[test]
    fn insert_overwrite_persistent() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(1, "three".into());
        assert_eq!(overlay.get(&1).unwrap(), "three");
    }

    #[test]
    fn insert_overwrite_transient() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(1, "three".into());
        assert_eq!(overlay.get(&1).unwrap(), "three");
        overlay.insert(1, "four".into());
        assert_eq!(overlay.get(&1).unwrap(), "four");
    }

    #[test]
    fn insert_after_clear() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        assert_eq!(overlay.get(&1), None);
        overlay.insert(1, "three".into());
        assert_eq!(overlay.get(&1).unwrap(), "three");
    }

    #[test]
    fn insert_after_remove() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.remove(&1);
        assert_eq!(overlay.get(&1), None);
        overlay.insert(1, "three".into());
        assert_eq!(overlay.get(&1).unwrap(), "three");
    }

    #[test]
    fn clear_persistent() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        assert_eq!(overlay.get(&1), None);
    }

    #[test]
    fn clear_overwritten_persistent() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(1, "fourty two".into());
        overlay.clear();
        assert_eq!(overlay.get(&1), None);
    }

    #[test]
    fn clear_transient() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(2, "two".into());
        overlay.clear();
        assert_eq!(overlay.get(&2), None);
    }

    #[test]
    fn contains_key_non_existing() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let overlay = OrderedOverlayMap::new(&persistent);
        assert!(!overlay.contains_key(&2));
    }

    #[test]
    fn contains_key_persistent() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let overlay = OrderedOverlayMap::new(&persistent);
        assert!(overlay.contains_key(&1));
    }

    #[test]
    fn contains_key_persitent_after_clear() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        assert!(!overlay.contains_key(&1));
    }

    #[test]
    fn contains_key_transient_after_insert() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(2, "seventeen".into());
        assert!(overlay.contains_key(&2));
    }

    #[test]
    fn contains_key_transient_after_insert_after_clear() {
        let persistent = make_persistent_map([(1, "one".to_string())]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        overlay.insert(1, "fourty two".into());
        assert!(overlay.contains_key(&1));
    }

    #[test]
    fn initial_len_equals_len_of_persistent() {
        let persistent = make_persistent_map([
            (1, "one".to_string()),
            (2, "two".to_string()),
            (3, "three".to_string()),
            (4, "four".to_string()),
        ]);
        let overlay = OrderedOverlayMap::new(&persistent);
        assert_eq!(overlay.len(), 4);
    }

    #[test]
    fn len_after_clear_is_zero() {
        let persistent = make_persistent_map([
            (1, "one".to_string()),
            (2, "two".to_string()),
            (3, "three".to_string()),
            (4, "four".to_string()),
        ]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        assert_eq!(overlay.len(), 0);
    }

    #[test]
    fn len_after_overwrite_is_same() {
        let persistent = make_persistent_map([
            (1, "one".to_string()),
            (2, "two".to_string()),
            (3, "three".to_string()),
            (4, "four".to_string()),
        ]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(2, "twenty".into());
        assert_eq!(overlay.len(), 4);
    }

    #[test]
    fn len_after_insert_increases() {
        let persistent = make_persistent_map([
            (1, "one".to_string()),
            (2, "two".to_string()),
            (3, "three".to_string()),
            (4, "four".to_string()),
        ]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.insert(7, "seven".into());
        assert_eq!(overlay.len(), 5);
    }

    #[test]
    fn update_existing() {
        let persistent = make_persistent_map([(2, 222)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        let update_result = overlay.update(&2, |v| {
            *v += 333;
            Ok(777)
        });
        assert_matches!(update_result, Some(Ok(777)));
        assert_eq!(overlay.get(&2), Some(555));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_non_existing() {
        let persistent = make_persistent_map([(1, 111)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        let update_result = overlay.update(&2, |v| {
            *v += 222;
            Ok(())
        });
        assert_matches!(update_result, None);
        assert_eq!(overlay.get(&2), None);
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_removed() {
        let persistent = make_persistent_map([(3, 333)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.remove(&3);
        let update_result = overlay.update(&3, |v| {
            *v += 111;
            Ok(())
        });
        assert_matches!(update_result, None);
        assert_eq!(overlay.get(&3), None);
        assert_eq!(overlay.len(), 0);
    }

    #[test]
    fn update_after_clear() {
        let persistent = make_persistent_map([(5, 555)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        let update_result = overlay.update(&5, |v| {
            *v += 555;
            Ok(())
        });
        assert_matches!(update_result, None);
        assert_eq!(overlay.get(&5), None);
        assert_eq!(overlay.len(), 0);
    }

    #[test]
    fn update_updated() {
        let persistent = make_persistent_map([(3, 33)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.update(&3, |v| {
            *v += 11;
            Ok(())
        });
        overlay.update(&3, |v| {
            assert_eq!(*v, 44);
            *v += 44;
            Ok(())
        });
        assert_eq!(overlay.get(&3), Some(88));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_persistent_fails_value_remains_unchanged() {
        let persistent = make_persistent_map([(3, 33)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.update(&3, |v| -> Result<()> {
            *v += 11;
            Err(make_error())
        });
        assert_eq!(overlay.get(&3), Some(33));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_transient_fails_value_remains_unchanged() {
        let persistent = make_persistent_map([(3, 33)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.update(&3, |v| {
            *v += 11;
            Ok(())
        });
        overlay.update(&3, |v| -> Result<()> {
            assert_eq!(*v, 44);
            *v += 22;
            Err(make_error())
        });
        assert_eq!(overlay.get(&3), Some(44));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_or_insert_item_exists_in_persistent_update_successful() {
        let persistent = make_persistent_map([(1, 111)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        let result = overlay.update_or_insert(
            &1,
            || panic!("not expected to be called"),
            |v, exists| {
                assert!(exists);
                *v += 222;
                Ok("success")
            },
        );
        assert_matches!(result, Ok("success"));
        assert_eq!(overlay.get(&1), Some(333));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_or_insert_item_exists_in_persistent_update_fails() {
        let persistent = make_persistent_map([(1, 111)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        let result: Result<()> = overlay.update_or_insert(
            &1,
            || panic!("not expected to be called"),
            |v, exists| {
                assert!(exists);
                *v += 222;
                Err(make_error())
            },
        );
        assert_matches!(result, Err(_));
        assert_eq!(overlay.get(&1), Some(111));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_or_insert_item_exists_in_transient_update_successful() {
        let persistent = make_persistent_map([(3, 33)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay
            .update(&3, |v| {
                *v += 22;
                Ok(())
            })
            .unwrap()
            .unwrap();

        let result = overlay.update_or_insert(
            &3,
            || panic!("not expected to be called"),
            |v, exists| {
                assert!(exists);
                *v += 11;
                Ok("success")
            },
        );
        assert_matches!(result, Ok("success"));
        assert_eq!(overlay.get(&3), Some(66));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_or_insert_item_exists_in_transient_update_fails() {
        let persistent = make_persistent_map([(3, 33)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay
            .update(&3, |v| {
                *v += 22;
                Ok(())
            })
            .unwrap()
            .unwrap();

        let result: Result<()> = overlay.update_or_insert(
            &3,
            || panic!("not expected to be called"),
            |v, exists| {
                assert!(exists);
                *v += 11;
                Err(make_error())
            },
        );
        assert_matches!(result, Err(_));
        assert_eq!(overlay.get(&3), Some(55));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_or_insert_item_did_not_exist_update_successful() {
        let persistent = make_persistent_map([(1, 1)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        let result = overlay.update_or_insert(&2, make_zero, |v, exists| {
            assert!(!exists);
            assert_eq!(*v, 0);
            *v = 2;
            Ok(())
        });
        assert_matches!(result, Ok(()));
        assert_eq!(overlay.get(&2), Some(2));
        assert_eq!(overlay.len(), 2);
    }

    #[test]
    fn update_or_insert_item_did_not_exist_update_fails() {
        let persistent = make_persistent_map([(1, 1)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        let result: Result<i32> = overlay.update_or_insert(&2, make_zero, |v, exists| {
            assert!(!exists);
            assert_eq!(*v, 0);
            *v = 2;
            Err(make_error())
        });
        assert_matches!(result, Err(_));
        assert_eq!(overlay.get(&2), None);
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_or_insert_after_clear_update_successful() {
        let persistent = make_persistent_map([(1, 1), (2, 2), (3, 3)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        let result = overlay.update_or_insert(&2, make_zero, |v, exists| {
            assert!(!exists);
            *v = 2;
            Ok(())
        });
        assert_matches!(result, Ok(()));
        assert_eq!(overlay.get(&2), Some(2));
        assert_eq!(overlay.len(), 1);
    }

    #[test]
    fn update_or_insert_after_clear_update_fails() {
        let persistent = make_persistent_map([(1, 1), (2, 2), (3, 3)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.clear();
        let result: Result<i32> = overlay.update_or_insert(&2, make_zero, |v, exists| {
            assert!(!exists);
            *v = 2;
            Err(make_error())
        });
        assert_matches!(result, Err(_));
        assert_eq!(overlay.get(&2), None);
        assert_eq!(overlay.len(), 0);
    }

    #[test]
    fn update_or_insert_item_was_removed_update_successful() {
        let persistent = make_persistent_map([(1, 1), (2, 2), (3, 3)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.remove(&3);
        let result = overlay.update_or_insert(&3, make_zero, |v, exists| {
            assert!(!exists);
            *v = 33;
            Ok(())
        });
        assert_matches!(result, Ok(()));
        assert_eq!(overlay.get(&3), Some(33));
        assert_eq!(overlay.len(), 3);
    }

    #[test]
    fn update_or_insert_item_was_removed_update_fails() {
        let persistent = make_persistent_map([(1, 1), (2, 2), (3, 3)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        overlay.remove(&3);
        let result: Result<i32> = overlay.update_or_insert(&3, make_zero, |v, exists| {
            assert!(!exists);
            *v = 33;
            Err(make_error())
        });
        assert_matches!(result, Err(_));
        assert_eq!(overlay.get(&3), None);
        assert_eq!(overlay.len(), 2);
    }

    #[test]
    fn inspect_above_empty() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let overlay = OrderedOverlayMap::new(&persistent);
        assert_inspect_at_yields(&overlay, Above(9), None);
    }

    #[test]
    fn inspect_above_persistent_only() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let overlay = OrderedOverlayMap::new(&persistent);

        assert_inspect_at_yields(&overlay, Above(-4), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Above(1), Some((2, 22)));
        assert_inspect_at_yields(&overlay, Above(2), Some((3, 33)));
        assert_inspect_at_yields(&overlay, Above(3), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Above(5), Some((9, 99)));
        assert_inspect_at_yields(&overlay, Above(7), Some((9, 99)));
        assert_inspect_at_yields(&overlay, Above(9), None);
        assert_inspect_at_yields(&overlay, Above(19), None);
    }

    #[test]
    fn inspect_above_transient_only() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.insert(1, 11);
        overlay.insert(2, 22);
        overlay.insert(3, 33);
        overlay.insert(5, 55);
        overlay.insert(9, 99);

        assert_inspect_at_yields(&overlay, Above(-4), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Above(1), Some((2, 22)));
        assert_inspect_at_yields(&overlay, Above(2), Some((3, 33)));
        assert_inspect_at_yields(&overlay, Above(3), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Above(4), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Above(5), Some((9, 99)));
        assert_inspect_at_yields(&overlay, Above(7), Some((9, 99)));
        assert_inspect_at_yields(&overlay, Above(9), None);
        assert_inspect_at_yields(&overlay, Above(19), None);
    }

    #[test]
    fn inspect_above_removed_first() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);

        assert_inspect_at_yields(&overlay, Above(-4), Some((2, 22)));
        assert_inspect_at_yields(&overlay, Above(1), Some((2, 22)));
    }

    #[test]
    fn inspect_above_removed_last() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&9);

        assert_inspect_at_yields(&overlay, Above(4), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Above(5), None);
        assert_inspect_at_yields(&overlay, Above(8), None);
        assert_inspect_at_yields(&overlay, Above(9), None);
        assert_inspect_at_yields(&overlay, Above(10), None);
    }

    #[test]
    fn inspect_above_removed_intermediate() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        assert_inspect_at_yields(&overlay, Above(-4), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Above(1), Some((3, 33)));
        assert_inspect_at_yields(&overlay, Above(2), Some((3, 33)));
    }

    #[test]
    fn inspect_above_removed_all() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);
        overlay.remove(&9);

        assert_inspect_at_yields(&overlay, Above(-73), None);
        assert_inspect_at_yields(&overlay, Above(0), None);
        assert_inspect_at_yields(&overlay, Above(9), None);
    }

    #[test]
    fn inspect_above_removed_several() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);

        assert_inspect_at_yields(&overlay, Above(-17), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Above(1), Some((9, 99)));
        assert_inspect_at_yields(&overlay, Above(9), None);
    }

    #[test]
    fn inspect_above_removed_inserted() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&3);
        overlay.insert(4, 44);

        assert_inspect_at_yields(&overlay, Above(2), Some((4, 44)));
        assert_inspect_at_yields(&overlay, Above(3), Some((4, 44)));
        assert_inspect_at_yields(&overlay, Above(4), Some((5, 55)));
    }

    #[test]
    fn inspect_above_with_removed_and_updated() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.insert(3, 333);

        assert_inspect_at_yields(&overlay, Above(1), Some((3, 333)));
        assert_inspect_at_yields(&overlay, Above(2), Some((3, 333)));
        assert_inspect_at_yields(&overlay, Above(3), Some((5, 55)));
    }

    #[test]
    fn inspect_below_empty() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let overlay = OrderedOverlayMap::new(&persistent);

        assert_inspect_at_yields(&overlay, Below(-73), None);
    }

    #[test]
    fn inspect_below_persistent_only() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let overlay = OrderedOverlayMap::new(&persistent);

        assert_inspect_at_yields(&overlay, Below(-4), None);
        assert_inspect_at_yields(&overlay, Below(1), None);
        assert_inspect_at_yields(&overlay, Below(2), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Below(3), Some((2, 22)));
        assert_inspect_at_yields(&overlay, Below(4), Some((3, 33)));
        assert_inspect_at_yields(&overlay, Below(5), Some((3, 33)));
        assert_inspect_at_yields(&overlay, Below(7), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Below(9), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Below(77), Some((9, 99)));
    }

    #[test]
    fn inspect_below_transient_only() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.insert(1, 11);
        overlay.insert(2, 22);
        overlay.insert(3, 33);
        overlay.insert(5, 55);
        overlay.insert(9, 99);

        assert_inspect_at_yields(&overlay, Below(-4), None);
        assert_inspect_at_yields(&overlay, Below(1), None);
        assert_inspect_at_yields(&overlay, Below(2), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Below(3), Some((2, 22)));
        assert_inspect_at_yields(&overlay, Below(4), Some((3, 33)));
        assert_inspect_at_yields(&overlay, Below(5), Some((3, 33)));
        assert_inspect_at_yields(&overlay, Below(7), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Below(9), Some((5, 55)));
        assert_inspect_at_yields(&overlay, Below(77), Some((9, 99)));
    }

    #[test]
    fn inspect_below_removed_first() {
        let persistent = make_persistent_map([(1, 11), (3, 33), (5, 55), (8, 88), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);

        assert_inspect_at_yields(&overlay, Below(-4), None);
        assert_inspect_at_yields(&overlay, Below(1), None);
        assert_inspect_at_yields(&overlay, Below(2), None);
        assert_inspect_at_yields(&overlay, Below(3), None);
        assert_inspect_at_yields(&overlay, Below(4), Some((3, 33)));
    }

    #[test]
    fn inspect_below_removed_last() {
        let persistent = make_persistent_map([(1, 11), (3, 33), (5, 55), (8, 88), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&9);
        assert_inspect_at_yields(&overlay, Below(14), Some((8, 88)));
        assert_inspect_at_yields(&overlay, Below(9), Some((8, 88)));
    }

    #[test]
    fn inspect_below_removed_intermediate() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);

        assert_inspect_at_yields(&overlay, Below(-4), None);
        assert_inspect_at_yields(&overlay, Below(1), None);
        assert_inspect_at_yields(&overlay, Below(2), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Below(3), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Below(4), Some((3, 33)));
    }

    #[test]
    fn inspect_below_removed_all() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);
        overlay.remove(&9);

        assert_inspect_at_yields(&overlay, Below(-73), None);
        assert_inspect_at_yields(&overlay, Below(4), None);
        assert_inspect_at_yields(&overlay, Below(19), None);
    }

    #[test]
    fn inspect_below_removed_several() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);

        assert_inspect_at_yields(&overlay, Below(-17), None);
        assert_inspect_at_yields(&overlay, Below(1), None);
        assert_inspect_at_yields(&overlay, Below(9), Some((1, 11)));
    }

    #[test]
    fn inspect_below_removed_inserted() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&3);
        overlay.insert(4, 44);

        assert_inspect_at_yields(&overlay, Below(1), None);
        assert_inspect_at_yields(&overlay, Below(2), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Below(3), Some((2, 22)));
        assert_inspect_at_yields(&overlay, Below(4), Some((2, 22)));
        assert_inspect_at_yields(&overlay, Below(5), Some((4, 44)));
        assert_inspect_at_yields(&overlay, Below(6), Some((5, 55)));
    }

    #[test]
    fn inspect_below_with_removed_and_updated() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.insert(3, 333);

        assert_inspect_at_yields(&overlay, Below(1), None);
        assert_inspect_at_yields(&overlay, Below(2), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Below(3), Some((1, 11)));
        assert_inspect_at_yields(&overlay, Below(4), Some((3, 333)));
        assert_inspect_at_yields(&overlay, Below(5), Some((3, 333)));
        assert_inspect_at_yields(&overlay, Below(6), Some((5, 55)));
    }

    #[test]
    fn inspect_min_max_persistent_only() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (9, 99)]);
        let overlay = OrderedOverlayMap::new(&persistent);

        assert_inspect_at_yields(&overlay, Min, Some((1, 11)));
        assert_inspect_at_yields(&overlay, Max, Some((9, 99)));
    }

    #[test]
    fn inspect_min_max_transient_only() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.insert(1, 11);
        overlay.insert(3, 33);
        overlay.insert(9, 99);

        assert_inspect_at_yields(&overlay, Min, Some((1, 11)));
        assert_inspect_at_yields(&overlay, Max, Some((9, 99)));
    }

    #[test]
    fn inspect_min_max_removed_first_and_last() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&9);

        assert_inspect_at_yields(&overlay, Min, Some((2, 22)));
        assert_inspect_at_yields(&overlay, Max, Some((5, 55)));
    }

    #[test]
    fn inspect_min_max_removed_several() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&5);
        overlay.remove(&9);

        assert_inspect_at_yields(&overlay, Min, Some((3, 33)));
        assert_inspect_at_yields(&overlay, Max, Some((3, 33)));
    }

    #[test]
    fn inspect_min_max_removed_all() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);
        overlay.remove(&9);

        assert_inspect_at_yields(&overlay, Min, None);
        assert_inspect_at_yields(&overlay, Max, None);
    }

    #[test]
    fn inspect_min_max_after_clear() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.clear();

        assert_inspect_at_yields(&overlay, Min, None);
        assert_inspect_at_yields(&overlay, Max, None);
    }

    #[test]
    fn inspect_min_max_inserted_after_clear() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.clear();
        overlay.insert(7, 77);

        assert_inspect_at_yields(&overlay, Min, Some((7, 77)));
        assert_inspect_at_yields(&overlay, Max, Some((7, 77)));
    }

    #[test]
    fn inspect_min_max_removed_inserted() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.insert(4, 44);
        overlay.insert(7, 77);
        overlay.remove(&9);

        assert_inspect_at_yields(&overlay, Min, Some((4, 44)));
        assert_inspect_at_yields(&overlay, Max, Some((7, 77)));
    }

    #[test]
    fn inspect_min_max_with_removed_and_updated() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.insert(3, 333);
        overlay.remove(&5);

        assert_inspect_at_yields(&overlay, Min, Some((3, 333)));
        assert_inspect_at_yields(&overlay, Max, Some((3, 333)));
    }

    #[test]
    fn update_above_empty() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);
        test_update_at(&mut overlay, Above(9), None, mul_3_ok);
    }

    #[test]
    fn update_above_persistent_only() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        test_update_at(&mut overlay, Above(-4), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Above(-4), Some((1, 11 * 3)), mul_3_ok);

        test_update_at(&mut overlay, Above(1), Some((2, 22)), mul_3_ok);
        test_update_at(&mut overlay, Above(1), Some((2, 22 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Above(1), Some((2, 22 * 3 * 3)), mul_3_fail);
        test_update_at(&mut overlay, Above(1), Some((2, 22 * 3 * 3)), mul_3_fail);

        test_update_at(&mut overlay, Above(2), Some((3, 33)), mul_3_fail);
        test_update_at(&mut overlay, Above(2), Some((3, 33)), mul_3_ok);
        test_update_at(&mut overlay, Above(2), Some((3, 33 * 3)), mul_3_ok);

        test_update_at(&mut overlay, Above(3), Some((5, 55)), mul_3_fail);
        test_update_at(&mut overlay, Above(3), Some((5, 55)), mul_3_fail);
        test_update_at(&mut overlay, Above(3), Some((5, 55)), mul_3_fail);

        test_update_at(&mut overlay, Above(5), Some((9, 99)), mul_3_ok);
        test_update_at(&mut overlay, Above(7), Some((9, 99 * 3)), mul_3_ok);

        test_update_at(&mut overlay, Above(9), None, mul_3_ok);
        test_update_at(&mut overlay, Above(9), None, mul_3_ok);

        test_update_at(&mut overlay, Above(19), None, mul_3_fail);
        test_update_at(&mut overlay, Above(19), None, mul_3_fail);
    }

    #[test]
    fn update_above_transient_only() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.insert(1, 11);
        overlay.insert(2, 22);
        overlay.insert(3, 33);
        overlay.insert(5, 55);
        overlay.insert(9, 99);

        test_update_at(&mut overlay, Above(-4), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Above(-4), Some((1, 11 * 3)), mul_3_ok);

        test_update_at(&mut overlay, Above(1), Some((2, 22)), mul_3_fail);
        test_update_at(&mut overlay, Above(1), Some((2, 22)), mul_3_fail);
        test_update_at(&mut overlay, Above(1), Some((2, 22)), mul_3_ok);
        test_update_at(&mut overlay, Above(1), Some((2, 22 * 3)), mul_3_ok);

        test_update_at(&mut overlay, Above(2), Some((3, 33)), mul_3_ok);
        test_update_at(&mut overlay, Above(2), Some((3, 33 * 3)), mul_3_fail);

        test_update_at(&mut overlay, Above(3), Some((5, 55)), mul_3_ok);
        test_update_at(&mut overlay, Above(4), Some((5, 55 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Above(3), Some((5, 55 * 9)), mul_3_ok);
        test_update_at(&mut overlay, Above(4), Some((5, 55 * 27)), mul_3_fail);

        test_update_at(&mut overlay, Above(5), Some((9, 99)), mul_3_fail);
        test_update_at(&mut overlay, Above(7), Some((9, 99)), mul_3_fail);
        test_update_at(&mut overlay, Above(5), Some((9, 99)), mul_3_ok);
        test_update_at(&mut overlay, Above(7), Some((9, 99 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Above(5), Some((9, 99 * 9)), mul_3_ok);
        test_update_at(&mut overlay, Above(7), Some((9, 99 * 27)), mul_3_ok);

        test_update_at(&mut overlay, Above(9), None, mul_3_fail);
        test_update_at(&mut overlay, Above(19), None, mul_3_ok);
    }

    #[test]
    fn update_above_removed_first() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);

        test_update_at(&mut overlay, Above(-4), Some((2, 22)), mul_3_fail);
        test_update_at(&mut overlay, Above(-4), Some((2, 22)), mul_3_ok);
        test_update_at(&mut overlay, Above(-4), Some((2, 22 * 3)), mul_3_fail);
        test_update_at(&mut overlay, Above(1), Some((2, 22 * 3)), mul_3_fail);
    }

    #[test]
    fn update_above_removed_last() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&9);

        test_update_at(&mut overlay, Above(4), Some((5, 55)), mul_3_fail);
        test_update_at(&mut overlay, Above(4), Some((5, 55)), mul_3_ok);
        test_update_at(&mut overlay, Above(4), Some((5, 55 * 3)), mul_3_fail);
        test_update_at(&mut overlay, Above(5), None, mul_3_fail);
        test_update_at(&mut overlay, Above(8), None, mul_3_ok);
        test_update_at(&mut overlay, Above(9), None, mul_3_ok);
        test_update_at(&mut overlay, Above(10), None, mul_3_fail);
    }

    #[test]
    fn update_above_removed_intermediate() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        test_update_at(&mut overlay, Above(-4), Some((1, 11)), mul_3_fail);
        test_update_at(&mut overlay, Above(-4), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Above(1), Some((3, 33)), mul_3_ok);
        test_update_at(&mut overlay, Above(2), Some((3, 33 * 3)), mul_3_fail);
    }

    #[test]
    fn update_above_removed_all() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);
        overlay.remove(&9);

        test_update_at(&mut overlay, Above(-73), None, mul_3_ok);
        test_update_at(&mut overlay, Above(-73), None, mul_3_fail);
        test_update_at(&mut overlay, Above(0), None, mul_3_fail);
        test_update_at(&mut overlay, Above(0), None, mul_3_ok);
        test_update_at(&mut overlay, Above(9), None, mul_3_ok);
        test_update_at(&mut overlay, Above(9), None, mul_3_fail);
    }

    #[test]
    fn update_above_removed_several() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);

        test_update_at(&mut overlay, Above(-17), Some((1, 11)), mul_3_fail);
        test_update_at(&mut overlay, Above(1), Some((9, 99)), mul_3_fail);
        test_update_at(&mut overlay, Above(1), Some((9, 99)), mul_3_ok);
        test_update_at(&mut overlay, Above(1), Some((9, 99 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Above(3), Some((9, 99 * 3 * 3)), mul_3_fail);
        test_update_at(&mut overlay, Above(9), None, mul_3_fail);
        test_update_at(&mut overlay, Above(8), Some((9, 99 * 3 * 3)), mul_3_ok);
    }

    #[test]
    fn update_above_removed_inserted() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&3);
        overlay.insert(4, 44);

        test_update_at(&mut overlay, Above(3), Some((4, 44)), mul_3_ok);
        test_update_at(&mut overlay, Above(2), Some((4, 44 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Above(4), Some((5, 55)), mul_3_ok);
    }

    #[test]
    fn update_above_with_removed_and_updated() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.insert(3, 333);

        test_update_at(&mut overlay, Above(1), Some((3, 333)), mul_3_fail);
        test_update_at(&mut overlay, Above(2), Some((3, 333)), mul_3_ok);
        test_update_at(&mut overlay, Above(2), Some((3, 333 * 3)), mul_3_fail);
    }

    #[test]
    fn update_below_empty() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        test_update_at(&mut overlay, Below(-73), None, mul_3_fail);
    }

    #[test]
    fn update_below_persistent_only() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        test_update_at(&mut overlay, Below(-4), None, mul_3_ok);
        test_update_at(&mut overlay, Below(1), None, mul_3_ok);
        test_update_at(&mut overlay, Below(2), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Below(3), Some((2, 22)), mul_3_ok);
        test_update_at(&mut overlay, Below(4), Some((3, 33)), mul_3_ok);
        test_update_at(&mut overlay, Below(5), Some((3, 33 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(7), Some((5, 55)), mul_3_ok);
        test_update_at(&mut overlay, Below(9), Some((5, 55 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(77), Some((9, 99)), mul_3_ok);
    }

    #[test]
    fn update_below_transient_only() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.insert(1, 11);
        overlay.insert(2, 22);
        overlay.insert(3, 33);
        overlay.insert(5, 55);
        overlay.insert(9, 99);

        test_update_at(&mut overlay, Below(-4), None, mul_3_ok);
        test_update_at(&mut overlay, Below(1), None, mul_3_ok);
        test_update_at(&mut overlay, Below(2), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Below(3), Some((2, 22)), mul_3_ok);
        test_update_at(&mut overlay, Below(4), Some((3, 33)), mul_3_ok);
        test_update_at(&mut overlay, Below(5), Some((3, 33 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(7), Some((5, 55)), mul_3_ok);
        test_update_at(&mut overlay, Below(9), Some((5, 55 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(77), Some((9, 99)), mul_3_ok);
    }

    #[test]
    fn update_below_removed_first() {
        let persistent = make_persistent_map([(1, 11), (3, 33), (5, 55), (8, 88), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);

        test_update_at(&mut overlay, Below(-4), None, mul_3_ok);
        test_update_at(&mut overlay, Below(1), None, mul_3_ok);
        test_update_at(&mut overlay, Below(2), None, mul_3_ok);
        test_update_at(&mut overlay, Below(3), None, mul_3_ok);
        test_update_at(&mut overlay, Below(4), Some((3, 33)), mul_3_ok);
    }

    #[test]
    fn update_below_removed_last() {
        let persistent = make_persistent_map([(1, 11), (3, 33), (5, 55), (8, 88), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&9);

        test_update_at(&mut overlay, Below(14), Some((8, 88)), mul_3_ok);
        test_update_at(&mut overlay, Below(9), Some((8, 88 * 3)), mul_3_ok);
    }

    #[test]
    fn update_below_removed_intermediate() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);

        test_update_at(&mut overlay, Below(-4), None, mul_3_ok);
        test_update_at(&mut overlay, Below(1), None, mul_3_ok);
        test_update_at(&mut overlay, Below(2), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Below(3), Some((1, 11 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(4), Some((3, 33)), mul_3_ok);
    }

    #[test]
    fn update_below_removed_all() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);
        overlay.remove(&9);

        test_update_at(&mut overlay, Below(-73), None, mul_3_ok);
        test_update_at(&mut overlay, Below(4), None, mul_3_ok);
        test_update_at(&mut overlay, Below(19), None, mul_3_ok);
    }

    #[test]
    fn update_below_removed_several() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);

        test_update_at(&mut overlay, Below(-17), None, mul_3_ok);
        test_update_at(&mut overlay, Below(1), None, mul_3_ok);
        test_update_at(&mut overlay, Below(9), Some((1, 11)), mul_3_ok);
    }

    #[test]
    fn update_below_removed_inserted() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&3);
        overlay.insert(4, 44);

        test_update_at(&mut overlay, Below(1), None, mul_3_ok);
        test_update_at(&mut overlay, Below(2), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Below(3), Some((2, 22)), mul_3_ok);
        test_update_at(&mut overlay, Below(4), Some((2, 22 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(5), Some((4, 44)), mul_3_ok);
        test_update_at(&mut overlay, Below(6), Some((5, 55)), mul_3_ok);
    }

    #[test]
    fn update_below_with_removed_updated() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&2);
        overlay.insert(3, 333);

        test_update_at(&mut overlay, Below(1), None, mul_3_ok);
        test_update_at(&mut overlay, Below(2), Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Below(3), Some((1, 11 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(4), Some((3, 333)), mul_3_ok);
        test_update_at(&mut overlay, Below(5), Some((3, 333 * 3)), mul_3_ok);
        test_update_at(&mut overlay, Below(6), Some((5, 55)), mul_3_ok);
    }

    #[test]
    fn update_min_max_persistent_only() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        test_update_at(&mut overlay, Min, Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Max, Some((9, 99)), mul_3_ok);
    }

    #[test]
    fn update_min_max_transient_only() {
        let empty: Vec<(i32, i32)> = Vec::new();
        let persistent = make_persistent_map(empty);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.insert(1, 11);
        overlay.insert(3, 33);
        overlay.insert(9, 99);

        test_update_at(&mut overlay, Min, Some((1, 11)), mul_3_ok);
        test_update_at(&mut overlay, Max, Some((9, 99)), mul_3_ok);
    }

    #[test]
    fn update_min_max_removed_first_and_last() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&9);

        test_update_at(&mut overlay, Min, Some((2, 22)), mul_3_ok);
        test_update_at(&mut overlay, Max, Some((5, 55)), mul_3_ok);
    }

    #[test]
    fn update_min_max_removed_several() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&5);
        overlay.remove(&9);

        test_update_at(&mut overlay, Min, Some((3, 33)), mul_3_ok);
        test_update_at(&mut overlay, Max, Some((3, 33 * 3)), mul_3_ok);
    }

    #[test]
    fn update_min_max_removed_all() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.remove(&5);
        overlay.remove(&9);

        test_update_at(&mut overlay, Min, None, mul_3_ok);
        test_update_at(&mut overlay, Max, None, mul_3_ok);
    }

    #[test]
    fn update_min_max_after_clear() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.clear();

        test_update_at(&mut overlay, Min, None, mul_3_ok);
        test_update_at(&mut overlay, Max, None, mul_3_ok);
    }

    #[test]
    fn update_min_max_inserted_after_clear() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.clear();
        overlay.insert(7, 77);

        test_update_at(&mut overlay, Min, Some((7, 77)), mul_3_ok);
        test_update_at(&mut overlay, Max, Some((7, 77 * 3)), mul_3_ok);
    }

    #[test]
    fn update_min_max_removed_inserted() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55), (9, 99)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.remove(&3);
        overlay.insert(4, 44);
        overlay.insert(7, 77);
        overlay.remove(&9);

        test_update_at(&mut overlay, Min, Some((4, 44)), mul_3_ok);
        test_update_at(&mut overlay, Max, Some((7, 77)), mul_3_ok);
    }

    #[test]
    fn update_min_max_with_removed_and_updated() {
        let persistent = make_persistent_map([(1, 11), (2, 22), (3, 33), (5, 55)]);
        let mut overlay = OrderedOverlayMap::new(&persistent);

        overlay.remove(&1);
        overlay.remove(&2);
        overlay.insert(3, 333);
        overlay.remove(&5);

        test_update_at(&mut overlay, Min, Some((3, 333)), mul_3_ok);
        test_update_at(&mut overlay, Max, Some((3, 333 * 3)), mul_3_ok);
    }
}
