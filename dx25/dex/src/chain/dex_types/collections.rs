use std::ops::{Deref, DerefMut};

use multiversx_sc::{
    api::StorageMapperApi,
    codec::{self, NestedDecode, NestedEncode, TopDecode, TopEncode},
    storage::{
        mappers::{MapMapper, SetMapper, StorageClearable, StorageMapper},
        StorageKey,
    },
    types::{ManagedBuffer, ManagedType},
};
use multiversx_sc_codec::derive::{NestedDecode, NestedEncode, TopDecode, TopEncode};

use crate::dex::{
    collection_helpers::{StorageRef, StorageRefIter, StorageRefPairIter},
    Map, MapRemoveKey, Result, Set,
};

/// Provides coding for any storage mapper
///
/// The only part actually coded is `storage_key`, while `mapper`
/// is ignored upon encoding and restored using decoded key upon decoding
struct CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    storage_key: ManagedBuffer<S>,
    mapper: M,
}

impl<S, M> CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    fn new(storage_key: ManagedBuffer<S>) -> Self {
        let key_handle = storage_key.get_handle();
        Self {
            storage_key,
            mapper: M::new(StorageKey::from_handle(key_handle)),
        }
    }
}

impl<S, M> Deref for CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    type Target = M;

    fn deref(&self) -> &Self::Target {
        &self.mapper
    }
}

impl<S, M> DerefMut for CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.mapper
    }
}

impl<S, M> NestedEncode for CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    fn dep_encode<O: multiversx_sc_codec::NestedEncodeOutput>(
        &self,
        dest: &mut O,
    ) -> std::result::Result<(), multiversx_sc_codec::EncodeError> {
        // We intentionally encode only `storage_key`, rest of structure is transient
        self.storage_key.dep_encode(dest)
    }
}

impl<S, M> NestedDecode for CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    fn dep_decode<I: multiversx_sc_codec::NestedDecodeInput>(
        input: &mut I,
    ) -> std::result::Result<Self, multiversx_sc_codec::DecodeError> {
        // The only persistent part is storage key, rest is transient
        Ok(Self::new(ManagedBuffer::dep_decode(input)?))
    }
}

impl<S, M> TopEncode for CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    fn top_encode<O>(&self, output: O) -> std::result::Result<(), multiversx_sc_codec::EncodeError>
    where
        O: multiversx_sc_codec::TopEncodeOutput,
    {
        self.storage_key.top_encode(output)
    }
}

impl<S, M> TopDecode for CodableMapper<S, M>
where
    S: StorageMapperApi,
    M: StorageMapper<S>,
{
    fn top_decode<I>(input: I) -> std::result::Result<Self, multiversx_sc_codec::DecodeError>
    where
        I: multiversx_sc_codec::TopDecodeInput,
    {
        Ok(Self::new(ManagedBuffer::top_decode(input)?))
    }
}
/// Set of items persisted in blockchain's key-value storage
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode)]
pub struct StorageSet<S, T>
where
    S: StorageMapperApi,
    T: TopEncode + TopDecode + NestedEncode + NestedDecode + 'static,
{
    mapper: CodableMapper<S, SetMapper<S, T>>,
}

impl<S, T> StorageSet<S, T>
where
    S: StorageMapperApi,
    T: TopEncode + TopDecode + NestedEncode + NestedDecode + 'static,
{
    pub fn new(storage_key: ManagedBuffer<S>) -> Self {
        Self {
            mapper: CodableMapper::new(storage_key),
        }
    }
}

impl<S, T> Set for StorageSet<S, T>
where
    S: StorageMapperApi,
    T: TopEncode + TopDecode + NestedEncode + NestedDecode + 'static,
{
    type Item = T;
    type Ref<'a> = StorageRef<'a, T> where Self: 'a;
    /// `SetMapper` iterator cannot be named, so we must use boxed value
    type Iter<'a> = StorageRefIter<'a, T, Box<dyn Iterator<Item = T> + 'a>> where Self: 'a;

    fn iter(&self) -> Self::Iter<'_> {
        StorageRefIter::new_boxed(self.mapper.iter())
    }

    fn clear(&mut self) {
        self.mapper.clear();
    }

    fn len(&self) -> usize {
        self.mapper.len()
    }

    fn is_empty(&self) -> bool {
        self.mapper.is_empty()
    }

    fn contains_item(&self, item: &T) -> bool {
        self.mapper.contains(item)
    }

    fn add_item(&mut self, item: T) {
        self.mapper.insert(item);
    }

    fn remove_item(&mut self, item: &T) {
        self.mapper.remove(item);
    }
}

/// Storage ID to be used in nested sets.
/// Contains `storage_key`, which can be used to get the set handle
#[derive(TopEncode, TopDecode, NestedDecode, NestedEncode)]
pub struct StorageMap<S, K, V>
where
    S: StorageMapperApi,
    K: Clone + TopEncode + TopDecode + NestedEncode + NestedDecode + 'static,
    V: TopEncode + TopDecode + 'static,
{
    mapper: CodableMapper<S, MapMapper<S, K, V>>,
}

impl<S, K, V> StorageMap<S, K, V>
where
    S: StorageMapperApi,
    K: Clone + TopEncode + TopDecode + NestedEncode + NestedDecode + 'static,
    V: TopEncode + TopDecode + 'static,
{
    pub fn new(storage_key: ManagedBuffer<S>) -> Self {
        Self {
            mapper: CodableMapper::new(storage_key),
        }
    }
}

impl<S, K, V> Map for StorageMap<S, K, V>
where
    S: StorageMapperApi,
    K: Clone + TopEncode + TopDecode + NestedEncode + NestedDecode + 'static,
    V: TopEncode + TopDecode + 'static,
{
    type Key = K;
    type Value = V;
    type KeyRef<'a> = StorageRef<'a, K> where Self: 'a;
    type ValueRef<'a> = StorageRef<'a, V> where Self: 'a;
    /// `MapMapper` iterator cannot be named, so we must use boxed value
    type Iter<'a> = StorageRefPairIter<'a, K, V, Box<dyn Iterator<Item = (K, V)> + 'a>> where Self: 'a;

    fn iter(&self) -> Self::Iter<'_> {
        StorageRefPairIter::new_boxed(self.mapper.iter())
    }

    fn clear(&mut self) {
        self.mapper.clear();
    }

    fn len(&self) -> usize {
        self.mapper.len()
    }

    fn is_empty(&self) -> bool {
        self.mapper.is_empty()
    }

    fn contains_key(&self, key: &K) -> bool {
        self.mapper.contains_key(key)
    }

    fn inspect<R, F: FnOnce(&V) -> R>(&self, key: &K, inspect_fn: F) -> Option<R> {
        self.mapper.get(key).map(|value| inspect_fn(&value))
    }

    // Can't match over the value, because Elrond lib doen's export the Entry type
    fn update<R, F: FnOnce(&mut V) -> Result<R>>(
        &mut self,
        key: &K,
        update_fn: F,
    ) -> Option<Result<R>> {
        self.mapper.get(key).map(|mut value| {
            let result = update_fn(&mut value);
            self.insert(key.clone(), value);
            result
        })
    }

    fn update_or_insert<R, F, U>(&mut self, key: &K, factory_fn: F, update_fn: U) -> Result<R>
    where
        F: FnOnce() -> Result<V>,
        U: FnOnce(&mut V, /* exists */ bool) -> Result<R>,
    {
        let (mut value, exists) = match self.mapper.get(key) {
            None => (factory_fn()?, false),
            Some(value) => (value, true),
        };

        let result = update_fn(&mut value, exists);
        self.mapper.insert(key.clone(), value);
        result
    }

    fn insert(&mut self, key: K, value: V) {
        self.mapper.insert(key, value);
    }
}

impl<S, K, V> MapRemoveKey for StorageMap<S, K, V>
where
    S: StorageMapperApi,
    K: Clone + TopEncode + TopDecode + NestedEncode + NestedDecode + 'static,
    V: TopEncode + TopDecode + 'static,
{
    fn remove(&mut self, key: &K) {
        self.mapper.remove(key);
    }
}
