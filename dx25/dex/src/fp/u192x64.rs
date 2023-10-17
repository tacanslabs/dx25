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

use super::{Error, U128, U256, U320, U320X192};
use crate::chain::Float;
use crate::fp::try_float_to_ufp::try_float_to_ufp;
use crate::fp::ufp_to_float::ufp_to_float;

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
pub struct U192X64(pub U256);

impl Zero for U192X64 {
    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
    fn zero() -> Self {
        Self(U256::zero())
    }
    fn set_zero(&mut self) {
        self.0.set_zero();
    }
}

impl U192X64 {
    pub fn one() -> Self {
        U192X64(U256([0, 1, 0, 0]))
    }

    pub const fn fract(self) -> Self {
        // the fractional part is saved in the first part
        // of the underlying array therefore the underlying
        // array contains zeroth and first values of the
        // array, and the second part is zeroed, as the
        // integer part is zero
        U192X64(U256([self.0 .0[0], 0, 0, 0]))
    }

    pub const fn floor(self) -> Self {
        // the integer part is saved in the second part
        // of the underlying array therefore the underlying
        // array contains second and third values of the
        // array, and the first part is zeroed, as the
        // fractional part is zero
        U192X64(U256([0, self.0 .0[1], self.0 .0[2], self.0 .0[3]]))
    }

    pub fn integer_sqrt(self) -> Self {
        let integer_sqrt = self.0.integer_sqrt();
        // as we taking the sqaure root of a fraction
        // it's denominator, namely 2^64 also gets a square root
        // which is 2^64, therefore to compensate this
        // we need to multiply by 2^64, which is the same
        // as to move the underlying value by 1 to the right
        U192X64(U256([
            0,
            integer_sqrt.0[0],
            integer_sqrt.0[1],
            integer_sqrt.0[2],
        ]))
    }

    pub fn recip(self) -> Self {
        Self::one() / self
    }
}

impl ops::Add for U192X64 {
    type Output = Self;

    fn add(self, rhs: U192X64) -> Self {
        U192X64(self.0 + rhs.0)
    }
}

impl ops::AddAssign for U192X64 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl ops::Sub for U192X64 {
    type Output = Self;

    fn sub(self, rhs: U192X64) -> Self {
        U192X64(self.0 - rhs.0)
    }
}

impl ops::SubAssign for U192X64 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl ops::Mul for U192X64 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        // The underlying U256s are multiplied exactly, in sufficiently high precision,
        // and converted to U192X64 taking the scale into account and truncating excessive precision.
        // As the product must fit into U192X64, it is sufficient to perfrom
        // the multiplication in 384 (i.e. 3x128) bits:
        // U192X64 x U192X64 = U256/2**64 x U256/2**64 = U320/2**128 = U128x128  -->  U192X64

        let self_u320 = U320([self.0 .0[0], self.0 .0[1], self.0 .0[2], self.0 .0[3], 0]);
        let rhs_u320 = U320([rhs.0 .0[0], rhs.0 .0[1], rhs.0 .0[2], rhs.0 .0[3], 0]);

        // The product of two U192X64 may not necessarily fit into U192X64,
        // so we need to check for overflow:
        let (rhs_u320, is_overflow) = self_u320.overflowing_mul(rhs_u320);
        assert!(!is_overflow, "{}", Error::Overflow);

        // Scale the product back to U192X64:
        U192X64(U256([
            rhs_u320.0[1],
            rhs_u320.0[2],
            rhs_u320.0[3],
            rhs_u320.0[4],
        ]))
    }
}

impl ops::Div for U192X64 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        // as we divide 2 fractions with the same denominator (namely 2^128)
        // we are getting a value without a denominator
        // we need to multiply by this denominator to respect the definition
        // doing this is the same as moving the underlying array
        // by one u64 value to the right
        let self_u320_mul_2_64 = U320([0, self.0 .0[0], self.0 .0[1], self.0 .0[2], self.0 .0[3]]);
        let rhs_u320 = U320([rhs.0 .0[0], rhs.0 .0[1], rhs.0 .0[2], rhs.0 .0[3], 0]);

        let res_u320 = self_u320_mul_2_64 / rhs_u320;
        // assure no overflows happen
        assert!(res_u320.0[4] == 0, "{}", Error::Overflow);

        U192X64(U256([
            res_u320.0[0],
            res_u320.0[1],
            res_u320.0[2],
            res_u320.0[3],
        ]))
    }
}

impl Sum for U192X64 {
    fn sum<I: Iterator<Item = U192X64>>(iter: I) -> Self {
        let mut s = U192X64::zero();
        for i in iter {
            s += i;
        }
        s
    }
}

impl<'a> Sum<&'a Self> for U192X64 {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        let mut s = U192X64::zero();
        for i in iter {
            s += *i;
        }
        s
    }
}

impl From<u128> for U192X64 {
    fn from(value: u128) -> Self {
        U192X64(U256([0, value as u64, (value >> 64) as u64, 0]))
    }
}

impl From<U128> for U192X64 {
    fn from(value: U128) -> Self {
        U192X64(U256([0, value.0[0], value.0[1], 0]))
    }
}

impl From<[u64; 4]> for U192X64 {
    fn from(array: [u64; 4]) -> Self {
        Self(U256(array))
    }
}

impl TryFrom<U192X64> for u128 {
    type Error = Error;

    fn try_from(v: U192X64) -> Result<Self, Self::Error> {
        if v.0 .0[3] > 0 {
            return Err(Error::Overflow);
        }
        Ok((u128::from(v.0 .0[2]) << 64) + u128::from(v.0 .0[1]))
    }
}

impl From<U192X64> for [u64; 4] {
    fn from(value: U192X64) -> Self {
        value.0 .0
    }
}

impl TryFrom<Float> for U192X64 {
    type Error = Error;
    fn try_from(value: Float) -> Result<Self, Self::Error> {
        try_float_to_ufp::<U192X64, 4, 1>(value)
    }
}

impl From<U192X64> for Float {
    fn from(v: U192X64) -> Self {
        ufp_to_float::<4, 1>(v.0 .0)
    }
}

impl TryFrom<U320X192> for U192X64 {
    type Error = Error;
    fn try_from(v: U320X192) -> Result<Self, Self::Error> {
        if v.0 .0[0] > 0 || v.0 .0[1] > 0 {
            return Err(Error::PrecisionLoss);
        }
        if v.0 .0[6] == 0 && v.0 .0[7] == 0 {
            return Err(Error::Overflow);
        }
        Ok(U192X64(U256([v.0 .0[2], v.0 .0[3], v.0 .0[4], v.0 .0[5]])))
    }
}

#[cfg(test)]
mod test {
    use super::super::ufp_to_float::{FLOAT_TWO_POW_128, FLOAT_TWO_POW_64};
    use super::*;
    use assert_matches::assert_matches;
    use float_extras::f64::ldexp;

    #[test]
    fn test_sum() {
        let one = U192X64(U256::one());
        let two = U192X64(U256::one() * 2);
        assert_eq!(one + one, two);
    }

    #[test]
    fn test_sub() {
        let one = U192X64(U256::one());
        let two = U192X64(U256::one() * 2);
        assert_eq!(two - one, one);
    }

    #[test]
    fn test_mul() {
        let real_one = U192X64(U256::one() << 64);
        let real_two = U192X64((U256::one() << 64) * 2);
        assert_eq!(real_two * real_one, real_two);
    }

    #[test]
    fn test_mul_large() {
        assert_eq!(
            U192X64::from(1u128 << 100) * U192X64::from(1u128 << 26),
            U192X64::from(1u128 << 126)
        );
    }

    #[test]
    fn test_div() {
        let real_one = U192X64(U256::one() << 64);
        let real_two = U192X64((U256::one() << 64) * 2);
        assert_eq!(real_two / real_one, real_two);
    }

    #[test]
    fn test_floor() {
        let unit = U256::one() << 64;
        let cases = [
            U192X64(unit * 2),
            U192X64(unit * 34141),
            U192X64(unit * 1435134134),
            U192X64(unit * 1),
            U192X64((unit >> 2) + 111),
            U192X64((unit >> 34) + 33),
            U192X64(unit << 32),
        ];
        for x in cases {
            println!("Case: {}", x.0);
            assert_eq!(x.floor(), U192X64((x.0 >> 64) << 64));
        }
    }

    #[test]
    fn test_fract() {
        let unit = U256::one();
        let cases = [
            U192X64(unit * 2),
            U192X64(unit * 34141),
            U192X64(unit * 1435134134),
            U192X64(unit * 6),
            U192X64((unit << 2) + 111),
            U192X64((unit << 34) + 13),
            U192X64(unit << 32),
            U192X64((unit << 64) + 1231),
        ];
        for x in cases {
            println!("Case: {}", x.0);
            assert_eq!(x.fract(), U192X64((x.0 << 192) >> 192));
        }
    }

    #[test]
    fn test_try_into_u128_successful() {
        let expected_u128 = 32478829823894127273462167823_u128; // arbitrary value between 2^64 and 2^128
        let expected_u128_upper_64_bits = (expected_u128 >> 64) as u64;
        let expected_u128_lower_64_bits = (expected_u128 & ((1u128 << 64) - 1)) as u64;

        let u192x64 = U192X64(U256([
            7812734871234_u64, // arbitrary - will be truncated
            expected_u128_lower_64_bits,
            expected_u128_upper_64_bits,
            0,
        ]));

        assert_eq!(u128::try_from(u192x64).unwrap(), expected_u128);
    }

    #[test]
    fn test_try_into_u128_overflow() {
        let u192x64 = U192X64(U256([0, 0, 0, 1]));
        assert_matches!(u128::try_from(u192x64), Err(Error::Overflow));
    }

    #[test]
    fn test_try_f64_to_u192x64_large() {
        assert_eq!(
            U192X64::try_from(Float::from(ldexp(1_f64, 127))).unwrap(),
            U192X64::from(1u128 << 127)
        );

        assert_eq!(
            U192X64::try_from(Float::from(ldexp(f64::from(0b_1111_1111_1111), 128 - 12))).unwrap(),
            U192X64::from(0b_1111_1111_1111_u128 << (128 - 12))
        );
    }

    #[test]
    fn test_try_f64_to_u192x64_tiny() {
        assert_eq!(
            U192X64::try_from(Float::from(10f64)).unwrap(),
            U192X64::from(10)
        );

        assert_eq!(
            U192X64::try_from(Float::from(ldexp(287_f64, -64)))
                .unwrap()
                .0
                 .0,
            [287_u64, 0_u64, 0_u64, 0_u64]
        );

        assert_eq!(
            U192X64::try_from(Float::from(ldexp(113_f64, 0)))
                .unwrap()
                .0
                 .0,
            [0_u64, 113_u64, 0_u64, 0_u64]
        );
    }

    fn assert_eq_errors(e1: &Error, e2: &Error) {
        assert_eq!(format!("{:?}", e1), format!("{:?}", e2));
    }

    #[test]
    fn test_try_f64_to_u192x64_overflow() {
        assert_eq_errors(
            &U192X64::try_from(Float::from(ldexp(1_f64, 192))).unwrap_err(),
            &Error::Overflow,
        );
    }

    #[test]
    fn test_try_f64_to_u192x64_prec_loss() {
        assert_eq_errors(
            &U192X64::try_from(Float::from(ldexp(1_f64, -65))).unwrap_err(),
            &Error::PrecisionLoss,
        );
    }

    #[test]
    fn test_try_f64_to_u192x64_negative() {
        dbg!(FLOAT_TWO_POW_64.to_bits());
        dbg!(FLOAT_TWO_POW_128.to_bits());
        assert_eq_errors(
            &U192X64::try_from(Float::from(-0.15)).unwrap_err(),
            &Error::NegativeToUnsigned,
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_u192x64_to_f64() {
        assert_eq!(
            Float::from(U192X64::from(0) / U192X64::from(1)),
            Float::from(0.)
        );
        assert_eq!(
            Float::from(U192X64::from(217_387) / U192X64::from(1_000_000)),
            Float::from(0.217_387)
        );
        assert_eq!(
            Float::from(U192X64::from(71356) / U192X64::from(100)),
            Float::from(713.56)
        );
        assert_eq!(
            Float::from(U192X64::from(211_387_616) / U192X64::from(1000)),
            Float::from(211_387.616)
        );
        assert_eq!(
            Float::from(U192X64::from(372_792_773) / U192X64::from(1)),
            Float::from(372_792_773.)
        );
    }
}
