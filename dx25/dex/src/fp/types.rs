#![allow(clippy::all, clippy::pedantic)]
#[cfg(feature = "concordium")]
use concordium_std::{SchemaType, Serialize};
#[cfg(feature = "multiversx")]
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode, TopDecode, TopEncode},
};
#[cfg(feature = "near")]
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    serde::{Deserialize, Serialize},
};

use uint::construct_uint;

use super::traits::{IntegerSqrt, OverflowMul};
use num_traits::Zero;

construct_uint! {
    /// 128-bit unsigned integer, constructed out of 2 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    #[cfg_attr(all(feature = "smartlib", feature = "multiversx"), derive(serde::Serialize, serde::Deserialize))]
    pub struct U128(2);
}

construct_uint! {
    /// 256-bit unsigned integer, constructed out of 4 words x 64 bits.
    #[cfg_attr(all(feature = "smartlib", feature = "concordium"), derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U256(4);
}

construct_uint! {
    /// 384-bit unsigned integer, constructed out of 6 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U320(5);
}

construct_uint! {
    /// 384-bit unsigned integer, constructed out of 6 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U384(6);
}

construct_uint! {
    /// 448-bit unsigned integer, constructed out of 7 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U448(7);
}

construct_uint! {
    /// 512-bit unsigned integer, constructed out of 8 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U512(8);
}

construct_uint! {
    /// 576-bit unsigned integer, constructed out of 9 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U576(9);
}

construct_uint! {
    /// 832-bit unsigned integer, constructed out of 11 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U704(11);
}

construct_uint! {
    /// 640-bit unsigned integer, constructed out of 10 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U640(10);
}

construct_uint! {
    /// 768-bit unsigned integer, constructed out of 12 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U768(12);
}

construct_uint! {
    /// 896-bit unsigned integer, constructed out of 14 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U896(14);
}

construct_uint! {
    /// 1024-bit unsigned integer, constructed out of 16 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U1024(16);
}

construct_uint! {
    /// 960-bit unsigned integer, constructed out of 15 words x 64 bits.
    #[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize))]
    #[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
    #[cfg_attr(feature = "multiversx", derive(TopDecode, TopEncode, NestedDecode, NestedEncode))]
    pub struct U960(15);
}

#[allow(unused)]
macro_rules! impl_uint {
    ($name:ident, $size_words:literal) => {
        impl Zero for $name {
            fn is_zero(&self) -> bool {
                self.0.iter().all(|word| *word == 0)
            }

            fn zero() -> Self {
                Self::zero()
            }

            fn set_zero(&mut self) {
                for word in self.0.iter_mut() {
                    *word = 0;
                }
            }
        }

        impl IntegerSqrt for $name {
            fn integer_sqrt(&self) -> Self {
                self.integer_sqrt()
            }
        }

        impl OverflowMul for $name {
            fn overflowing_mul(&self, rhs: Self) -> (Self, bool) {
                <Self>::overflowing_mul(*self, rhs)
            }
        }

        impl From<[u64; $size_words]> for $name {
            fn from(inner_value: [u64; $size_words]) -> Self {
                Self(inner_value)
            }
        }
    };
}

impl_uint!(U256, 4);
impl_uint!(U384, 6);
impl_uint!(U448, 7);
impl_uint!(U512, 8);
impl_uint!(U576, 9);
impl_uint!(U640, 10);
impl_uint!(U704, 11);
impl_uint!(U768, 12);
impl_uint!(U896, 14);
impl_uint!(U960, 15);
impl_uint!(U1024, 16);
