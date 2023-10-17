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
use num_traits::Zero;

use std::iter::Sum;
use std::ops;

use super::{
    try_float_to_ufp::try_float_to_ufp, ufp_to_float::ufp_to_float, Error, U192X192, U256,
    U256X256, U320X192, U384, U448, U512,
};
use crate::chain::Float;

#[cfg_attr(
    feature = "near",
    derive(BorshDeserialize, BorshSerialize, Deserialize, Serialize)
)]
#[cfg_attr(feature = "concordium", derive(Serialize, SchemaType))]
#[cfg_attr(
    feature = "multiversx",
    derive(TopDecode, TopEncode, NestedDecode, NestedEncode)
)]
#[derive(Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct U320X64(pub U384);

impl U320X64 {
    pub const fn one() -> Self {
        U320X64(U384([0, 1, 0, 0, 0, 0]))
    }

    pub const fn fract(self) -> Self {
        U320X64(U384([self.0 .0[0], 0, 0, 0, 0, 0]))
    }

    pub const fn floor(self) -> Self {
        U320X64(U384([
            0,
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
        ]))
    }

    pub fn integer_sqrt(self) -> Self {
        // as we taking the sqaure root of a fraction
        // it's denominator, namely 2^64 also gets a square root
        // which is 2^32, therefore to compensate this
        // we need to multiply by 2^32, which is the same
        // as to make 32 left shifts
        U320X64(self.0.integer_sqrt() << 32)
    }

    pub fn recip(self) -> Self {
        Self::one() / self
    }
}

impl Zero for U320X64 {
    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
    fn zero() -> Self {
        Self(U384::zero())
    }
    fn set_zero(&mut self) {
        self.0.set_zero();
    }
}

impl From<u128> for U320X64 {
    fn from(value: u128) -> Self {
        U320X64(U384([0, value as u64, (value >> 64) as u64, 0, 0, 0]))
    }
}

impl From<U320X64> for [u64; 6] {
    fn from(value: U320X64) -> Self {
        value.0 .0
    }
}

impl From<[u64; 6]> for U320X64 {
    fn from(array: [u64; 6]) -> Self {
        Self(U384(array))
    }
}

impl ops::Add for U320X64 {
    type Output = Self;

    fn add(self, rhs: U320X64) -> Self {
        U320X64(self.0 + rhs.0)
    }
}

impl ops::AddAssign for U320X64 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl ops::Sub for U320X64 {
    type Output = Self;

    fn sub(self, rhs: U320X64) -> Self {
        U320X64(self.0 - rhs.0)
    }
}

impl ops::SubAssign for U320X64 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl ops::Mul for U320X64 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        // The underlying U384 are multiplied exactly, in sufficiently high precision,
        // and converted to U320X64 taking the scale into account and truncating excessive precision.
        // As the product must fit into U320X64, it is sufficient to perfrom
        // the multiplication in 384 (i.e. 3x128) bits:
        // U320X64 x U320X64 = U384/2**64 x U384/2**64 = U448/2**128  -->  U320X64

        let self_u448 = U448([
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
            0,
        ]);
        let rhs_u448 = U448([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            0,
        ]);

        // The product of two U320X64 may not necessarily fit into U320X64,
        // so we need to check for overflow:
        let (result, is_overflow) = self_u448.overflowing_mul(rhs_u448);
        assert!(!is_overflow, "{}", Error::Overflow);

        // Scale the product back to U320X64:
        U320X64(U384([
            result.0[1],
            result.0[2],
            result.0[3],
            result.0[4],
            result.0[5],
            result.0[6],
        ]))
    }
}

impl ops::Div for U320X64 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        // as we divide 2 fractions with the same denominator (namely 2^128)
        // we are getting a value without a denominator
        // we need to multiply by this denominator to respect the definition
        // doing this is the same as moving the underlying array
        // by one u64 value to the right
        let self_u448_mul_2_64 = U448([
            0,
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
        ]);

        let rhs_u448 = U448([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            0,
        ]);

        let result = self_u448_mul_2_64 / rhs_u448;
        // assure no overflows happen
        assert!(result.0[6] == 0, "{}", Error::Overflow);

        U320X64(U384([
            result.0[0],
            result.0[1],
            result.0[2],
            result.0[3],
            result.0[4],
            result.0[5],
        ]))
    }
}

impl Sum for U320X64 {
    fn sum<I: Iterator<Item = U320X64>>(iter: I) -> Self {
        let mut s = U320X64::zero();
        for i in iter {
            s += i;
        }
        s
    }
}

impl<'a> Sum<&'a Self> for U320X64 {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        let mut s = U320X64::zero();
        for i in iter {
            s += *i;
        }
        s
    }
}

impl From<U320X64> for U256X256 {
    fn from(value: U320X64) -> Self {
        // This should never be an overflow
        assert!(value.0 .0[5] == 0, "{}", Error::Overflow);

        let mut bytes: [u8; 72] = [0; 72];
        value.0.to_big_endian(&mut bytes[..48]);
        // Skip first byte
        Self(U512::from_big_endian(&bytes[8..]))
    }
}

impl TryFrom<U320X64> for U192X192 {
    type Error = Error;

    fn try_from(v: U320X64) -> Result<Self, Self::Error> {
        // This should never be an overflow
        if !(v.0 .0[5] == 0) && (v.0 .0[4] == 0) {
            return Err(Error::Overflow);
        }

        Ok(U192X192(U384([
            0, 0, v.0 .0[0], v.0 .0[1], v.0 .0[2], v.0 .0[3],
        ])))
    }
}

impl From<U320X192> for U320X64 {
    fn from(v: U320X192) -> Self {
        U320X64(U384([
            v.0 .0[2], v.0 .0[3], v.0 .0[4], v.0 .0[5], v.0 .0[6], v.0 .0[7],
        ]))
    }
}

impl From<U320X64> for Float {
    fn from(value: U320X64) -> Self {
        ufp_to_float::<6, 1>(value.0 .0)
    }
}

impl TryFrom<Float> for U320X64 {
    type Error = Error;

    fn try_from(value: Float) -> Result<Self, Self::Error> {
        try_float_to_ufp::<_, 6, 1>(value)
    }
}

impl From<U256> for U320X64 {
    fn from(value: U256) -> Self {
        U320X64(U384([0, value.0[0], value.0[1], value.0[2], value.0[3], 0]))
    }
}

impl From<U192X192> for U320X64 {
    fn from(value: U192X192) -> Self {
        // This should never be an overflow
        assert!(
            (value.0 .0[0] == 0) && (value.0 .0[1] == 0),
            "{}",
            Error::Overflow
        );
        U320X64(U384([
            value.0 .0[2],
            value.0 .0[3],
            value.0 .0[4],
            value.0 .0[5],
            0,
            0,
        ]))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_256x256_conversion() {
        let x320_one = U320X64(U384::one() << 319);
        let x256_one = U256X256(U512::one() << 511);

        assert_eq!(x256_one, U256X256::from(x320_one));
    }

    #[test]
    #[should_panic]
    fn test_invalid_256x256_conversion() {
        let x320_one = U320X64(U384::one() << 257);
        let x256_one = U256X256(U512::one() << 256);

        assert_eq!(x256_one, U256X256::from(x320_one));
    }

    #[test]
    fn test_256_conversion() {
        let x320_one = U320X64::from(U256::from(42u128));
        let x256_one = U256X256::from(U256::from(42u128));

        assert_eq!(x256_one, U256X256::from(x320_one));
    }

    #[test]
    fn test_320_mul() {
        let real_one = U320X64(U384::one() << 64);
        let real_two = U320X64((U384::one() << 64) * 2);
        assert_eq!(real_two * real_one, real_two);
    }

    #[test]
    fn test_320_mul_large() {
        let left = U320X64::from(1u128 << 100);
        let right = U320X64::from(1u128 << 26);

        dbg!(left.to_string(), right.to_string());

        assert_eq!(left * right, U320X64::from(1u128 << 126));
    }

    #[test]
    fn test_320_div() {
        let real_one = U320X64(U384::one() << 64);
        let real_two = U320X64((U384::one() << 64) * 2);
        assert_eq!(real_two / real_one, real_two);
    }

    #[test]
    fn test_320_debug() {
        assert_eq!(U320X64::one().to_string(), "1.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        assert_eq!(U320X64::from(42u128).to_string(), "42.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
    }
}
