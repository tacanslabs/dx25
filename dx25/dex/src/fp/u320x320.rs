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
use num_traits::{CheckedAdd, CheckedMul, CheckedSub, Zero};

use std::iter::Sum;
use std::ops;

use super::{
    try_float_to_ufp::try_float_to_ufp, ufp_to_float::ufp_to_float, Error, U192X192, U256,
    U256X320, U320X128, U320X64, U384, U576, U640, U960,
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
pub struct U320X320(pub U640);

impl U320X320 {
    pub const fn one() -> Self {
        U320X320(U640([0, 0, 0, 0, 0, 1, 0, 0, 0, 0]))
    }

    pub const fn fract(self) -> Self {
        U320X320(U640([
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            0,
            0,
            0,
            0,
            0,
        ]))
    }

    pub const fn floor(self) -> Self {
        U320X320(U640([
            0,
            0,
            0,
            0,
            0,
            self.0 .0[5],
            self.0 .0[6],
            self.0 .0[7],
            self.0 .0[8],
            self.0 .0[9],
        ]))
    }

    pub fn integer_sqrt(self) -> Self {
        U320X320(self.0.integer_sqrt() << (320 / 2))
    }
}

impl Zero for U320X320 {
    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
    fn zero() -> Self {
        Self(U640::zero())
    }
    fn set_zero(&mut self) {
        self.0.set_zero();
    }
}

impl From<u128> for U320X320 {
    fn from(value: u128) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let lower_word = value as u64;
        let upper_word = (value >> 64) as u64;

        U320X320(U640([0, 0, 0, 0, 0, lower_word, upper_word, 0, 0, 0]))
    }
}

impl From<U320X320> for [u64; 10] {
    fn from(value: U320X320) -> Self {
        value.0 .0
    }
}

impl From<[u64; 10]> for U320X320 {
    fn from(array: [u64; 10]) -> Self {
        Self(U640(array))
    }
}

impl ops::Add for U320X320 {
    type Output = Self;

    fn add(self, rhs: U320X320) -> Self {
        U320X320(self.0 + rhs.0)
    }
}

impl CheckedAdd for U320X320 {
    fn checked_add(&self, v: &Self) -> Option<Self> {
        self.0.checked_add(v.0).map(Self)
    }
}

impl CheckedSub for U320X320 {
    fn checked_sub(&self, v: &Self) -> Option<Self> {
        self.0.checked_sub(v.0).map(Self)
    }
}

impl ops::AddAssign for U320X320 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl ops::Sub for U320X320 {
    type Output = Self;

    fn sub(self, rhs: U320X320) -> Self {
        U320X320(self.0 - rhs.0)
    }
}

impl ops::SubAssign for U320X320 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl CheckedMul for U320X320 {
    fn checked_mul(&self, rhs: &Self) -> Option<Self> {
        // The underlying U640 are multiplied exactly, in sufficiently high precision,
        // and converted to U320X320 taking the scale into account and truncating excessive precision.
        // As the product must fit into U320X320, it is sufficient to perfrom
        // the multiplication in 960 (i.e. 320 + 320 + 320) bits:
        // U320X320 x U320X320 = U640/2**320 x U640/2**320 = U960/2**640  -->  U320X320

        let self_u960 = U960([
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
            self.0 .0[6],
            self.0 .0[7],
            self.0 .0[8],
            self.0 .0[9],
            0,
            0,
            0,
            0,
            0,
        ]);
        let rhs_u960 = U960([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            rhs.0 .0[6],
            rhs.0 .0[7],
            rhs.0 .0[8],
            rhs.0 .0[9],
            0,
            0,
            0,
            0,
            0,
        ]);

        // The product of two U320X320 may not necessarily fit into U320X320,
        // so we need to check for overflow:
        self_u960.checked_mul(rhs_u960).map(|result| {
            // Scale the product back to U320X320:
            U320X320(U640([
                result.0[5],
                result.0[6],
                result.0[7],
                result.0[8],
                result.0[9],
                result.0[10],
                result.0[11],
                result.0[12],
                result.0[13],
                result.0[14],
            ]))
        })
    }
}

impl ops::Mul for U320X320 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        self.checked_mul(&rhs)
            .unwrap_or_else(|| panic!("{}", Error::Overflow))
    }
}

impl ops::Div for U320X320 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        // as we divide 2 fractions with the same denominator (namely 2^320)
        // we are getting a value without a denominator
        // we need to multiply by this denominator to respect the definition
        // doing this is the same as moving the underlying array
        // by 192 bits to the right
        let self_u960_mul_2_320 = U960([
            0,
            0,
            0,
            0,
            0,
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
            self.0 .0[6],
            self.0 .0[7],
            self.0 .0[8],
            self.0 .0[9],
        ]);

        let rhs_u960 = U960([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            rhs.0 .0[6],
            rhs.0 .0[7],
            rhs.0 .0[8],
            rhs.0 .0[9],
            0,
            0,
            0,
            0,
            0,
        ]);

        let result = self_u960_mul_2_320 / rhs_u960;
        // ensure no overflows happen
        assert!(
            result.0[10..15].iter().all(|word| *word == 0),
            "{}",
            Error::Overflow
        );

        U320X320(U640([
            result.0[0],
            result.0[1],
            result.0[2],
            result.0[3],
            result.0[4],
            result.0[5],
            result.0[6],
            result.0[7],
            result.0[8],
            result.0[9],
        ]))
    }
}

impl Sum for U320X320 {
    fn sum<I: Iterator<Item = U320X320>>(iter: I) -> Self {
        let mut s = U320X320::zero();
        for i in iter {
            s += i;
        }
        s
    }
}

impl<'a> Sum<&'a Self> for U320X320 {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        let mut s = U320X320::zero();
        for i in iter {
            s += *i;
        }
        s
    }
}

impl From<U320X320> for Float {
    fn from(value: U320X320) -> Self {
        ufp_to_float::<10, 5>(value.0 .0)
    }
}

impl TryFrom<Float> for U320X320 {
    type Error = Error;

    fn try_from(value: Float) -> Result<Self, Self::Error> {
        try_float_to_ufp::<_, 10, 5>(value)
    }
}

impl From<U256> for U320X320 {
    fn from(value: U256) -> Self {
        U320X320(U640([
            0, 0, 0, 0, 0, value.0[0], value.0[1], value.0[2], value.0[3], 0,
        ]))
    }
}
impl From<U320X64> for U320X320 {
    fn from(value: U320X64) -> Self {
        U320X320(U640([
            0,
            0,
            0,
            0,
            value.0 .0[0],
            value.0 .0[1],
            value.0 .0[2],
            value.0 .0[3],
            value.0 .0[4],
            value.0 .0[5],
        ]))
    }
}

impl TryFrom<U320X320> for U256X320 {
    type Error = Error;

    fn try_from(value: U320X320) -> Result<Self, Self::Error> {
        if value.0 .0[9] != 0 {
            return Err(Error::Overflow);
        };

        Ok(U256X320(U576([
            value.0 .0[0],
            value.0 .0[1],
            value.0 .0[2],
            value.0 .0[3],
            value.0 .0[4],
            value.0 .0[5],
            value.0 .0[6],
            value.0 .0[7],
            value.0 .0[8],
        ])))
    }
}

impl TryFrom<U320X320> for U192X192 {
    type Error = Error;

    fn try_from(value: U320X320) -> Result<Self, Self::Error> {
        if value.0 .0[9] != 0 || value.0 .0[8] != 0 {
            return Err(Error::Overflow);
        };
        if value.0 .0[0] != 0 || value.0 .0[1] != 0 {
            return Err(Error::PrecisionLoss);
        };

        Ok(U192X192(U384([
            value.0 .0[2],
            value.0 .0[3],
            value.0 .0[4],
            value.0 .0[5],
            value.0 .0[6],
            value.0 .0[7],
        ])))
    }
}

impl From<U320X128> for U320X320 {
    fn from(value: U320X128) -> Self {
        U320X320(U640([
            0,
            0,
            0,
            value.0 .0[0],
            value.0 .0[1],
            value.0 .0[2],
            value.0 .0[3],
            value.0 .0[4],
            value.0 .0[5],
            value.0 .0[6],
        ]))
    }
}

impl From<U192X192> for U320X320 {
    fn from(value: U192X192) -> Self {
        U320X320(U640([
            0,
            0,
            value.0 .0[0],
            value.0 .0[1],
            value.0 .0[2],
            value.0 .0[3],
            value.0 .0[4],
            value.0 .0[5],
            0,
            0,
        ]))
    }
}

impl From<U256X320> for U320X320 {
    fn from(value: U256X320) -> Self {
        U320X320(U640([
            value.0 .0[0],
            value.0 .0[1],
            value.0 .0[2],
            value.0 .0[3],
            value.0 .0[4],
            value.0 .0[5],
            value.0 .0[6],
            value.0 .0[7],
            value.0 .0[8],
            0,
        ]))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_320_mul() {
        let real_one = U320X320(U640::one() << 320);
        let real_two = U320X320((U640::one() << 320) * 2);
        assert_eq!(real_two * real_one, real_two);
    }

    #[test]
    fn test_320_mul_large() {
        let left = U320X320::from(1u128 << 100);
        let right = U320X320::from(1u128 << 26);

        assert_eq!(left * right, U320X320::from(1u128 << 126));
    }

    #[test]
    fn test_320_div() {
        let real_one = U320X320(U640::one() << 320);
        let real_two = U320X320((U640::one() << 320) * 2);
        assert_eq!(real_two / real_one, real_two);
    }

    #[test]
    fn test_320_debug() {
        assert_eq!(U320X320::one().to_string(), "1.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        assert_eq!(U320X320::from(42u128).to_string(), "42.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
    }
}
