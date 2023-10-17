use super::{ErrorKind, Result};
use crate::error_here;
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

#[cfg(feature = "concordium")]
use concordium_std::{Deletable, DeserialWithState, HasStateApi, Serial};
#[cfg(feature = "near")]
use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
// Unfortunately can't use derive macros here, they don't add proper reqs to type parameters
#[cfg(feature = "multiversx")]
use multiversx_sc::codec::{NestedDecode, NestedEncode, TopDecode, TopEncode};

pub trait MapContext {
    /// Produces error which is returned when specified key wasn't found
    fn not_found_error() -> ErrorKind;
}
/// Wrapper type for map-like collections which provides some additional capabilities
/// * methods which produce predefined "not found" error
pub struct MapWithContext<T, E: MapContext>(T, PhantomData<E>);

#[cfg(feature = "near")]
impl<T: BorshSerialize, E: MapContext> BorshSerialize for MapWithContext<T, E> {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

#[cfg(feature = "near")]
impl<T: BorshDeserialize, E: MapContext> BorshDeserialize for MapWithContext<T, E> {
    fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
        BorshDeserialize::deserialize(buf).map(Self::new)
    }
}

#[cfg(feature = "concordium")]
impl<T: Serial, E: MapContext> Serial for MapWithContext<T, E> {
    fn serial<W: concordium_std::Write>(&self, out: &mut W) -> std::result::Result<(), W::Err> {
        self.0.serial(out)
    }
}

#[cfg(feature = "concordium")]
impl<S: HasStateApi, T: DeserialWithState<S>, E: MapContext> DeserialWithState<S>
    for MapWithContext<T, E>
{
    fn deserial_with_state<R: concordium_std::Read>(
        state: &S,
        source: &mut R,
    ) -> concordium_std::ParseResult<Self> {
        DeserialWithState::deserial_with_state(state, source).map(Self::new)
    }
}

#[cfg(feature = "concordium")]
impl<T: Deletable, E: MapContext> Deletable for MapWithContext<T, E> {
    fn delete(self) {
        self.0.delete();
    }
}

#[cfg(feature = "multiversx")]
impl<T: TopEncode, E: MapContext> TopEncode for MapWithContext<T, E> {
    fn top_encode<O>(&self, output: O) -> std::result::Result<(), multiversx_sc_codec::EncodeError>
    where
        O: multiversx_sc_codec::TopEncodeOutput,
    {
        self.0.top_encode(output)
    }
}

#[cfg(feature = "multiversx")]
impl<T: TopDecode, E: MapContext> TopDecode for MapWithContext<T, E> {
    fn top_decode<I>(input: I) -> std::result::Result<Self, multiversx_sc_codec::DecodeError>
    where
        I: multiversx_sc_codec::TopDecodeInput,
    {
        TopDecode::top_decode(input).map(Self::new)
    }
}

#[cfg(feature = "multiversx")]
impl<T: NestedEncode, E: MapContext> NestedEncode for MapWithContext<T, E> {
    fn dep_encode<O: multiversx_sc_codec::NestedEncodeOutput>(
        &self,
        dest: &mut O,
    ) -> std::result::Result<(), multiversx_sc_codec::EncodeError> {
        self.0.dep_encode(dest)
    }
}

#[cfg(feature = "multiversx")]
impl<T: NestedDecode, E: MapContext> NestedDecode for MapWithContext<T, E> {
    fn dep_decode<I: multiversx_sc_codec::NestedDecodeInput>(
        input: &mut I,
    ) -> std::result::Result<Self, multiversx_sc_codec::DecodeError> {
        NestedDecode::dep_decode(input).map(Self::new)
    }
}

impl<T, E: MapContext> From<T> for MapWithContext<T, E> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T, E: MapContext> Deref for MapWithContext<T, E> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, E: MapContext> DerefMut for MapWithContext<T, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T, E: MapContext> MapWithContext<T, E> {
    pub fn new(inner: T) -> Self {
        Self(inner, PhantomData)
    }
}

impl<T: super::Map, E: MapContext> MapWithContext<T, E> {
    #[track_caller]
    #[inline]
    pub fn try_inspect_or<R>(
        &self,
        key: &T::Key,
        error: ErrorKind,
        inspect_fn: impl FnOnce(&T::Value) -> R,
    ) -> Result<R> {
        match self.inspect(key, inspect_fn) {
            Some(r) => Ok(r),
            None => Err(error_here!(error)),
        }
    }

    #[track_caller]
    #[inline]
    pub fn try_update_or<R>(
        &mut self,
        key: &T::Key,
        error: ErrorKind,
        update_fn: impl FnOnce(&mut T::Value) -> Result<R>,
    ) -> Result<R> {
        match self.update(key, update_fn) {
            Some(r) => r,
            None => Err(error_here!(error)),
        }
    }
    /// Tries to find specified key and pass reference to found value to `inspect_fn`
    /// Unlike `dex::Map::inspect`, returns error defined by `E::not_found_error` if entry wasn't found
    #[track_caller]
    #[inline]
    pub fn try_inspect<R>(
        &self,
        key: &T::Key,
        inspect_fn: impl FnOnce(&T::Value) -> R,
    ) -> Result<R> {
        self.try_inspect_or(key, E::not_found_error(), inspect_fn)
    }
    /// Tries to find specified key and pass mutable reference to found value to `update_fn`
    /// Unlike `dex::Map::update`, returns error defined by `E::not_found_error` if entry wasn't found
    #[track_caller]
    #[inline]
    pub fn try_update<R>(
        &mut self,
        key: &T::Key,
        update_fn: impl FnOnce(&mut T::Value) -> Result<R>,
    ) -> Result<R> {
        self.try_update_or(key, E::not_found_error(), update_fn)
    }
}
