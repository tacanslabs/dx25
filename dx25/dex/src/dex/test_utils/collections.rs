use super::dex::{self, KeyAt, Map as _};
use super::storage::{Key, Snapshot, Storage, Value};
use super::{TestDe, TestSer};
use dex::collection_helpers::{StorageRef, StorageRefIter};
use scopeguard::defer;
use std::borrow::Borrow;
use std::cell::{Ref, RefCell, RefMut};
use std::marker::PhantomData;
use std::ops::Bound;
use std::rc::Rc;

#[cfg(feature = "near")]
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};

#[cfg(feature = "concordium")]
use concordium_std::Serialize;

#[cfg(feature = "multiversx")]
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode},
};

#[derive(Clone, Default)]
pub struct TypedStorage(Rc<RefCell<Storage>>);

thread_local! {
    /// Thread-local reference to storage context, used to deserialize map objects
    static STORAGE_CONTEXT: RefCell<Option<TypedStorage>> = RefCell::new(None);
}

impl TypedStorage {
    pub fn new() -> Self {
        // Special case - allows blockchain to perform some initialization code
        crate::chain::test_utils::init_test_env();
        Self(Rc::new(RefCell::new(Storage::new())))
    }

    pub fn read_root(&self) -> dex::Contract<super::Types> {
        self.read([]).expect("Root not initialized")
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn write_root(&self, contract: dex::Contract<super::Types>) {
        self.write([], &contract);
    }

    pub fn freeze(&self) -> TypedSnapshot {
        TypedSnapshot(self.0.as_ref().borrow_mut().freeze())
    }
    /// Produce function which properly deserializes value out of storage,
    /// without tying storage by lifetime
    fn de_value_fn<V: TestDe>(&self) -> impl FnMut(&Value) -> V {
        // Deserialization requires setting current storage as thread-local context
        // because there's no way to pass it directly to Map's, due to API limitations
        let mut storage = Some(self.clone());
        move |value| {
            let old = STORAGE_CONTEXT.with(|v| v.replace(storage.take()));
            defer! {
                storage = STORAGE_CONTEXT.with(|v| v.replace(old));
            };

            V::de(&mut value.as_slice())
        }
    }

    fn read<V: TestDe>(&self, key: impl Borrow<[u8]>) -> Option<V> {
        self.0
            .as_ref()
            .borrow()
            .get(key)
            .map(|v| self.de_value_fn()(v))
    }

    fn write<V: TestSer>(&self, key: impl Borrow<[u8]>, value: &V) {
        // Storage context isn't stored, so we don't need to touch it here
        let mut storage = self.0.as_ref().borrow_mut();
        let data = storage.get_or_insert(key).mutate();

        data.clear();
        value.ser(&mut *data);
    }

    /// Obtain current global state instance
    fn current() -> Option<TypedStorage> {
        STORAGE_CONTEXT.with(|v| v.borrow().clone())
    }

    fn next_prefix(&self) -> u64 {
        let mut storage = self.0.borrow_mut();
        let value = storage.get_or_insert(0u64.to_vec()).mutate();
        if value.is_empty() {
            2u64.ser(&mut *value);
            1u64
        } else {
            let next = u64::de(&mut &value[..]);
            value.clear();
            (next + 1u64).ser(&mut *value);
            next
        }
    }

    pub fn new_map<K, V>(&self) -> Map<K, V> {
        Map::new(self.next_prefix(), self.clone())
    }

    pub fn new_ord_map<K, V>(&self) -> OrderedMap<K, V> {
        OrderedMap::new(self.next_prefix(), self.clone())
    }
}

pub struct TypedSnapshot(Snapshot);

impl TypedSnapshot {
    pub fn thaw(&self) -> TypedStorage {
        TypedStorage(Rc::new(RefCell::new(self.0.thaw())))
    }
}

pub struct Map<K, V> {
    prefix: u64,
    storage: TypedStorage,
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> Map<K, V> {
    fn new(prefix: u64, storage: TypedStorage) -> Self {
        Self {
            prefix,
            storage,
            _phantom: PhantomData,
        }
    }

    fn storage(&self) -> Ref<'_, Storage> {
        self.storage.0.as_ref().borrow()
    }

    fn storage_mut(&self) -> RefMut<'_, Storage> {
        self.storage.0.as_ref().borrow_mut()
    }
}

impl<K: TestSer + TestDe, V: TestSer + TestDe> Map<K, V> {
    fn key(&self, key: &K) -> Vec<u8> {
        let mut buf = Vec::new();
        self.prefix.ser(&mut buf);
        key.ser(&mut buf);
        buf
    }

    fn read(&self, key: &K) -> Option<V> {
        let key = self.key(key);
        self.storage.read(key)
    }

    fn write(&mut self, key: &K, value: &V) {
        let key = self.key(key);
        self.storage.write(key, value);
    }

    fn key_prefix(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.prefix.ser(&mut buf);
        buf
    }

    fn prefix_bounds(&self) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
        Storage::prefix_bounds(self.key_prefix())
    }
    /// Returns function which deserializes key out of raw bytes,
    /// with respect to container's key prefix length
    fn de_key_fn() -> impl Fn(&Key) -> K {
        move |key| {
            let mut buf: &[u8] = key;
            u64::de(&mut buf);
            K::de(&mut buf)
        }
    }
    /// Returns function which deserializes value out of raw bytes,
    /// with respect to setting storage as thread-local context value
    #[allow(unused)] // Don't wanna delete it yet
    fn de_value_fn(&self) -> impl FnMut(&Value) -> V {
        self.storage.de_value_fn()
    }
    /// Retrieves list of all keys in map, in unspecified order
    fn keys(&self) -> Vec<K> {
        let de_key_fn = Self::de_key_fn();
        let bounds = self.prefix_bounds();
        let storage = self.storage();
        let range = storage.range(bounds);

        range.map(|(k, _)| de_key_fn(k)).collect()
    }
}

pub struct MapIter<'a, K, V> {
    keys: std::vec::IntoIter<K>,
    map: &'a Map<K, V>,
}

impl<'a, K: TestSer + TestDe, V: TestSer + TestDe> MapIter<'a, K, V> {
    fn new(map: &'a Map<K, V>) -> Self {
        let keys = map.keys();
        Self {
            keys: keys.into_iter(),
            map,
        }
    }
}

impl<'a, K: TestSer + TestDe + 'a, V: TestSer + TestDe + 'a> Iterator for MapIter<'a, K, V> {
    type Item = (StorageRef<'a, K>, StorageRef<'a, V>);

    fn next(&mut self) -> Option<Self::Item> {
        self.keys.next().map(|key| {
            let value = self.map.read(&key).unwrap();
            (StorageRef::new(key), StorageRef::new(value))
        })
    }
}

impl<K: TestSer + TestDe, V: TestSer + TestDe> super::dex::Map for Map<K, V> {
    type Key = K;
    type Value = V;
    type KeyRef<'a> = StorageRef<'a, K> where Self: 'a;
    type ValueRef<'a> = StorageRef<'a, V> where Self: 'a;
    type Iter<'a> = MapIter<'a, K, V> where Self: 'a;

    fn iter(&self) -> Self::Iter<'_> {
        MapIter::new(self)
    }

    fn clear(&mut self) {
        let key_prefix = self.key_prefix();
        self.storage_mut().remove_prefix(key_prefix);
    }

    fn len(&self) -> usize {
        let bounds = self.prefix_bounds();
        let storage = self.storage();
        storage.range(bounds).count()
    }

    fn is_empty(&self) -> bool {
        let bounds = self.prefix_bounds();
        let storage = self.storage();
        storage.range(bounds).next().is_none()
    }

    fn contains_key(&self, key: &K) -> bool {
        self.storage().contains(self.key(key))
    }

    fn inspect<R, F: FnOnce(&V) -> R>(&self, key: &K, inspect_fn: F) -> Option<R> {
        self.read(key).map(|v| inspect_fn(&v))
    }

    fn update<R, F: FnOnce(&mut V) -> crate::dex::Result<R>>(
        &mut self,
        key: &K,
        update_fn: F,
    ) -> Option<crate::dex::Result<R>> {
        self.read(key).map(|mut v| {
            update_fn(&mut v).map(|r| {
                self.write(key, &v);
                r
            })
        })
    }

    fn update_or_insert<R, F, U>(
        &mut self,
        key: &K,
        factory_fn: F,
        update_fn: U,
    ) -> crate::dex::Result<R>
    where
        F: FnOnce() -> crate::dex::Result<V>,
        U: FnOnce(&mut V, /* exists */ bool) -> crate::dex::Result<R>,
    {
        let (mut v, exists) = match self.read(key) {
            Some(v) => (v, true),
            None => (factory_fn()?, false),
        };
        let r = update_fn(&mut v, exists)?;
        self.write(key, &v);
        Ok(r)
    }

    fn insert(&mut self, key: K, value: V) {
        self.write(&key, &value);
    }
}

impl<K: TestSer + TestDe, V: TestSer + TestDe> super::dex::MapRemoveKey for Map<K, V> {
    fn remove(&mut self, key: &K) {
        let buf = self.key(key);
        self.storage_mut().remove(buf);
    }
}

impl<I: TestSer + TestDe> dex::Set for Map<I, ()> {
    type Item = I;
    type Ref<'a> = StorageRef<'a, I> where Self: 'a;
    type Iter<'a> = StorageRefIter<'a, I, std::vec::IntoIter<I>> where Self: 'a;

    fn clear(&mut self) {
        <Self as dex::Map>::clear(self);
    }

    fn len(&self) -> usize {
        <Self as dex::Map>::len(self)
    }

    fn is_empty(&self) -> bool {
        <Self as dex::Map>::is_empty(self)
    }

    fn iter(&self) -> Self::Iter<'_> {
        StorageRefIter::new(self.keys())
    }

    fn contains_item(&self, item: &I) -> bool {
        self.storage().contains(self.key(item))
    }

    fn add_item(&mut self, item: I) {
        self.write(&item, &());
    }

    fn remove_item(&mut self, item: &I) {
        let buf = self.key(item);
        self.storage_mut().remove(buf);
    }
}

#[cfg_attr(feature = "near", derive(BorshSerialize, BorshDeserialize))]
#[cfg_attr(feature = "concordium", derive(Serialize))]
#[cfg_attr(feature = "multiversx", derive(NestedEncode, NestedDecode))]
pub struct OrderedMap<K, V>(Map<K, V>);

impl<K, V> OrderedMap<K, V> {
    fn new(prefix: u64, storage: TypedStorage) -> Self {
        Self(Map::new(prefix, storage))
    }
}

impl<K: Ord + TestSer + TestDe, V: TestSer + TestDe> OrderedMap<K, V> {
    fn keys(&self) -> Vec<K> {
        let mut keys = self.0.keys();
        keys.sort();
        keys
    }

    fn try_swap_remove(mut keys: Vec<K>, idx: usize) -> Option<K> {
        if idx < keys.len() {
            Some(keys.swap_remove(idx))
        } else {
            None
        }
    }

    fn key_at(&self, at: dex::KeyAt<&K>) -> Option<K> {
        let keys = self.keys();
        match at {
            dex::KeyAt::Min => keys.into_iter().next(),
            dex::KeyAt::Max => keys.into_iter().next_back(),
            dex::KeyAt::Above(key) => match keys.binary_search(key) {
                // Key was found, so we try pick key right next to it
                Ok(idx) => Self::try_swap_remove(keys, idx + 1),
                // Key not found, so index points at insertion spot i.e. at element right above key
                Err(idx) => Self::try_swap_remove(keys, idx),
            },
            dex::KeyAt::Below(key) => match keys.binary_search(key) {
                // If key was found, index points to it;
                // if it wasn't found, index points to element right above it;
                // in any case, we need element prior to index
                //
                // 0 - 1 will produce `usize::MAX`, so we're fine here
                Ok(idx) | Err(idx) => Self::try_swap_remove(keys, idx.wrapping_sub(1)),
            },
        }
    }
}

pub struct OrderedMapIter<'a, K, V> {
    keys: std::vec::IntoIter<K>,
    map: &'a OrderedMap<K, V>,
}

impl<'a, K: Ord + TestSer + TestDe, V: TestSer + TestDe> OrderedMapIter<'a, K, V> {
    fn new(map: &'a OrderedMap<K, V>) -> Self {
        let keys = map.keys();
        Self {
            keys: keys.into_iter(),
            map,
        }
    }
}

impl<'a, K: Ord + TestSer + TestDe + 'a, V: TestSer + TestDe + 'a> Iterator
    for OrderedMapIter<'a, K, V>
{
    type Item = (StorageRef<'a, K>, StorageRef<'a, V>);

    fn next(&mut self) -> Option<Self::Item> {
        self.keys.next().map(|key| {
            let value = self.map.0.read(&key).unwrap();
            (StorageRef::new(key), StorageRef::new(value))
        })
    }
}

impl<K: Ord + TestSer + TestDe, V: TestSer + TestDe> dex::Map for OrderedMap<K, V> {
    type Key = K;
    type Value = V;
    type KeyRef<'a> = StorageRef<'a, K> where Self: 'a;
    type ValueRef<'a> = StorageRef<'a, V> where Self: 'a;
    type Iter<'a> = OrderedMapIter<'a, K, V> where Self: 'a;

    fn iter(&self) -> Self::Iter<'_> {
        OrderedMapIter::new(self)
    }

    fn clear(&mut self) {
        self.0.clear();
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    fn inspect<R, F: FnOnce(&V) -> R>(&self, key: &K, inspect_fn: F) -> Option<R> {
        self.0.inspect(key, inspect_fn)
    }

    fn update<R, F: FnOnce(&mut V) -> dex::Result<R>>(
        &mut self,
        key: &K,
        update_fn: F,
    ) -> Option<dex::Result<R>> {
        self.0.update(key, update_fn)
    }

    fn update_or_insert<R, F, U>(&mut self, key: &K, factory_fn: F, update_fn: U) -> dex::Result<R>
    where
        F: FnOnce() -> dex::Result<V>,
        U: FnOnce(&mut V, /* exists */ bool) -> dex::Result<R>,
    {
        self.0.update_or_insert(key, factory_fn, update_fn)
    }

    fn insert(&mut self, key: K, value: V) {
        self.0.insert(key, value);
    }
}

impl<K: Ord + TestSer + TestDe, V: TestSer + TestDe> dex::MapRemoveKey for OrderedMap<K, V> {
    fn remove(&mut self, key: &K) {
        self.0.remove(key);
    }
}

impl<K: Ord + TestSer + TestDe, V: TestSer + TestDe> dex::OrderedMap for OrderedMap<K, V> {
    fn inspect_at<R, F: FnOnce(&K, &V) -> R>(&self, at: KeyAt<&K>, inspect_fn: F) -> Option<R> {
        self.key_at(at)
            .and_then(|key| self.inspect(&key, |value| inspect_fn(&key, value)))
    }

    fn update_at<R, F: FnOnce(&K, &mut V) -> dex::Result<R>>(
        &mut self,
        at: KeyAt<&K>,
        update_fn: F,
    ) -> Option<dex::Result<R>> {
        self.key_at(at)
            .and_then(|key| self.update(&key, |value| update_fn(&key, value)))
    }
}

/// NEAR-specific bridges and proxies
#[cfg(feature = "near")]
mod near {
    use super::{Map, TypedStorage};
    use std::{io, marker::PhantomData};

    impl<K, V> near_sdk::borsh::BorshSerialize for Map<K, V> {
        fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
            self.prefix.serialize(writer)
        }
    }

    impl<K, V> near_sdk::borsh::BorshDeserialize for Map<K, V> {
        fn deserialize(buf: &mut &[u8]) -> io::Result<Self> {
            Ok(Self {
                prefix: near_sdk::borsh::BorshDeserialize::deserialize(buf)?,
                storage: TypedStorage::current().expect("No storage was set as context"),
                _phantom: PhantomData,
            })
        }
    }
}

/// Concordium-specific bridges and proxies
#[cfg(feature = "concordium")]
mod concordium {
    use super::{Map, TypedStorage};
    use concordium_std::{Deserial, ParseResult, Read};
    use std::marker::PhantomData;

    impl<K, V> concordium_std::Serial for Map<K, V> {
        fn serial<W: concordium_std::Write>(&self, out: &mut W) -> std::result::Result<(), W::Err> {
            self.prefix.serial(out)
        }
    }

    impl<K, V> Deserial for Map<K, V> {
        fn deserial<R: Read>(source: &mut R) -> ParseResult<Self> {
            Ok(Self {
                prefix: Deserial::deserial(source)?,
                storage: TypedStorage::current().expect("No storage was set as context"),
                _phantom: PhantomData,
            })
        }
    }
}

/// MultiversX-specific bridges and proxies
#[cfg(feature = "multiversx")]
mod multiversx {
    use super::{Map, TypedStorage};
    use multiversx_sc::codec::{
        DecodeError, EncodeError, NestedDecode, NestedDecodeInput, NestedEncode, NestedEncodeOutput,
    };
    use std::marker::PhantomData;

    impl<K, V> NestedEncode for Map<K, V> {
        fn dep_encode<O: NestedEncodeOutput>(&self, dest: &mut O) -> Result<(), EncodeError> {
            self.prefix.dep_encode(dest)
        }
    }

    impl<K, V> NestedDecode for Map<K, V> {
        fn dep_decode<I: NestedDecodeInput>(input: &mut I) -> Result<Self, DecodeError> {
            Ok(Self {
                prefix: NestedDecode::dep_decode(input)?,
                storage: TypedStorage::current().expect("No storage was set as context"),
                _phantom: PhantomData,
            })
        }
    }
}
