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

use super::{Error, U384, U576, U768};
use crate::chain::Float;
use crate::fp::try_float_to_ufp::try_float_to_ufp;
use crate::fp::ufp_to_float::ufp_to_float;
use crate::fp::{U128X128, U192X64, U256};
use num_traits::Zero;
use std::{iter::Sum, ops};

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
pub struct U192X192(pub U384);

#[allow(unused)]
impl U192X192 {
    pub const fn one() -> Self {
        U192X192(U384([0, 0, 0, 1, 0, 0]))
    }

    pub const fn fract(self) -> Self {
        // the fractional part is saved in the zeroth value
        // of the underlying array
        U192X192(U384([self.0 .0[0], self.0 .0[1], self.0 .0[2], 0, 0, 0]))
    }

    pub const fn floor(self) -> Self {
        // the integer part is saved in the first value
        // of the underlying array
        U192X192(U384([0, 0, 0, self.0 .0[3], self.0 .0[4], self.0 .0[5]]))
    }

    pub fn ceil(self) -> Self {
        let mut res = self.floor();
        if self.0 .0[0..3].iter().any(|word| *word > 0) {
            res += Self::from(1);
        }
        res
    }

    pub fn integer_sqrt(self) -> Self {
        // as we taking the sqaure root of a fraction
        // it's denominator, namely 2^192 also gets a square root
        // which is 2^96, therefore to compensate this
        // we need to multiply by 2^96, which is the same
        // as to make 32 left shifts
        U192X192(self.0.integer_sqrt() << 96)
    }

    #[allow(unused_assignments)]
    pub fn integer_cbrt(self) -> Self {
        let mut inner = self.0;

        let mut s = 255;
        let mut y = U384::zero();
        let mut b = U384::zero();
        let one = U384::one();
        while s >= 0 {
            y += y;
            b = U384::from(3) * y * (y + one) + one;
            if (inner >> s) >= b {
                inner -= b << s;
                y += one;
            }
            s -= 3;
        }
        U192X192(U384::from([0, 0, y.0[0], y.0[1], y.0[2], y.0[3]]))
    }
}

impl From<u128> for U192X192 {
    fn from(value: u128) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let lower_word = value as u64;
        let upper_word = (value >> 64) as u64;
        U192X192(U384([0, 0, 0, lower_word, upper_word, 0]))
    }
}

impl From<U128X128> for U192X192 {
    fn from(v: U128X128) -> Self {
        U192X192(U384([0, v.0 .0[0], v.0 .0[1], v.0 .0[2], v.0 .0[3], 0]))
    }
}

impl From<[u64; 6]> for U192X192 {
    fn from(array: [u64; 6]) -> Self {
        Self(U384(array))
    }
}

impl TryFrom<U192X192> for u128 {
    type Error = Error;

    fn try_from(v: U192X192) -> Result<Self, Self::Error> {
        if v.0 .0[5] > 0 {
            return Err(Error::Overflow);
        }
        Ok((u128::from(v.0 .0[4]) << 64) + u128::from(v.0 .0[3]))
    }
}

impl From<U192X192> for Float {
    fn from(v: U192X192) -> Self {
        ufp_to_float::<6, 3>(v.0 .0)
    }
}

impl From<U192X64> for U192X192 {
    fn from(v: U192X64) -> Self {
        U192X192(U384([0, 0, v.0 .0[0], v.0 .0[1], v.0 .0[2], v.0 .0[3]]))
    }
}

impl TryFrom<U192X192> for U192X64 {
    type Error = Error;
    fn try_from(v: U192X192) -> Result<Self, Self::Error> {
        if v.0 .0[0] > 0 || v.0 .0[1] > 0 {
            return Err(Error::PrecisionLoss);
        }
        Ok(U192X64(U256([v.0 .0[2], v.0 .0[3], v.0 .0[4], v.0 .0[5]])))
    }
}

impl TryFrom<Float> for U192X192 {
    type Error = Error;
    fn try_from(value: Float) -> Result<Self, Self::Error> {
        try_float_to_ufp::<U192X192, 6, 3>(value)
    }
}

impl ops::Add for U192X192 {
    type Output = Self;

    fn add(self, rhs: U192X192) -> Self {
        U192X192(self.0 + rhs.0)
    }
}

impl ops::AddAssign for U192X192 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl ops::Sub for U192X192 {
    type Output = Self;

    fn sub(self, rhs: U192X192) -> Self {
        U192X192(self.0 - rhs.0)
    }
}

impl ops::SubAssign for U192X192 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl Sum for U192X192 {
    fn sum<I: Iterator<Item = U192X192>>(iter: I) -> Self {
        let mut s = U192X192::zero();
        for i in iter {
            s += i;
        }
        s
    }
}

impl<'a> Sum<&'a Self> for U192X192 {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        let mut s = U192X192::zero();
        for i in iter {
            s += *i;
        }
        s
    }
}

impl ops::Mul for U192X192 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        // The underlying U384s are multiplied exactly, in sufficiently high precision,
        // and converted to U192X192 taking the scale into account and truncating excessive precision.
        // As the product must fit into U192X192, it is sufficient to perfrom
        // the multiplication in 576 (i.e. 3x192) bits:
        // U192X192 x U192X192 = U384/2**192 x U384/2**192 = U576/2**384 = U192x384  -->  U192X192

        let self_u576 = U576([
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
            0,
            0,
            0,
        ]);
        let rhs_u576 = U576([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            0,
            0,
            0,
        ]);

        // The product of two U192X192 may not necessarily fit into U192X192,
        // so we need to check for overflow:
        let (res_u576, is_overflow) = self_u576.overflowing_mul(rhs_u576);
        assert!(!is_overflow, "{}", Error::Overflow);

        // Scale the product back to U192X192:
        U192X192(U384([
            res_u576.0[3],
            res_u576.0[4],
            res_u576.0[5],
            res_u576.0[6],
            res_u576.0[7],
            res_u576.0[8],
        ]))
    }
}

impl ops::Div for U192X192 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        // as we divide 2 fractions with the same denominator (namely 2^192)
        // we are getting a value without a denominator
        // we need to multiply by this denominator to respect the definition
        // doing this is the same as moving the underlying array
        // by three u64 value to the right
        let self_u768_mul_2_196 = U768([
            0,
            0,
            0,
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
            0,
            0,
            0,
        ]);
        let rhs_u768 = U768([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            0,
            0,
            0,
            0,
            0,
            0,
        ]);

        let res_u768 = self_u768_mul_2_196 / rhs_u768;
        // assure no overflows happen
        assert!(
            res_u768.0[6] == 0
                && res_u768.0[7] == 0
                && res_u768.0[8] == 0
                && res_u768.0[9] == 0
                && res_u768.0[10] == 0
                && res_u768.0[11] == 0,
            "{}",
            Error::Overflow
        );

        U192X192(U384([
            res_u768.0[0],
            res_u768.0[1],
            res_u768.0[2],
            res_u768.0[3],
            res_u768.0[4],
            res_u768.0[5],
        ]))
    }
}

impl Zero for U192X192 {
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

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;

    #[test]
    fn test_sum() {
        let one = U192X192(U384::one());
        let two = U192X192(U384::one() * 2);
        assert_eq!(one + one, two);
    }

    #[test]
    fn test_sub() {
        let one = U192X192(U384::one());
        let two = U192X192(U384::one() * 2);
        assert_eq!(two - one, one);
    }

    #[test]
    fn test_mul() {
        let real_one = U192X192(U384::one() << 192);
        let real_two = U192X192((U384::one() << 192) * 2);
        assert_eq!(real_two * real_one, real_two);
    }

    #[test]
    fn test_mul_moderate_integers() {
        let v1 = 7_362_374_734_662_634_773_247_u128;
        let v2 = 7_362_467_237_u128;
        assert_eq!(
            U192X192::from(v1) * U192X192::from(v2),
            U192X192::from(v1 * v2)
        );
    }

    #[test]
    fn test_div() {
        let real_one = U192X192(U384::one() << 192);
        let real_two = U192X192((U384::one() << 192) * 2);
        assert_eq!(real_two / real_one, real_two);
    }

    #[test]
    fn test_floor() {
        let unit = U384::one() << 64;
        let cases = [
            U192X192(unit * 2),
            U192X192(unit * 34141),
            U192X192(unit * 1_435_134_134),
            U192X192(unit * 1),
            U192X192((unit >> 2) + 111),
            U192X192((unit >> 34) + 33),
            U192X192(unit << 32),
        ];
        for x in cases {
            println!("Case: {}", x.0);
            assert_eq!(x.floor(), U192X192((x.0 >> 192) << 192));
        }
    }

    #[test]
    fn test_ceil() {
        assert_eq!((U192X192::from(0)).ceil(), U192X192::from(0));
        assert_eq!(U192X192::from(237).ceil(), U192X192::from(237));
        assert_eq!(
            (U192X192::from(115) / U192X192::from(10)).ceil(),
            U192X192::from(12)
        );
        assert_eq!(
            (U192X192::from(723_000_000_001) / U192X192::from(1_000_000_000)).ceil(),
            U192X192::from(724)
        );
    }

    #[test]
    fn test_fract() {
        let unit = U384::one();
        let cases = [
            U192X192(unit * 2),
            U192X192(unit * 34141),
            U192X192(unit * 1_435_134_134),
            U192X192(unit * 6),
            U192X192((unit << 2) + 111),
            U192X192((unit << 34) + 13),
            U192X192(unit << 32),
            U192X192((unit << 64) + 1231),
        ];
        for x in cases {
            println!("Case: {}", x.0);
            assert_eq!(x.fract(), U192X192((x.0 << 64) >> 64));
        }
    }

    #[test]
    fn test_integer_sqrt() {
        let four = U192X192::from(4);
        let two = U192X192::from(2);
        assert_eq!(four.integer_sqrt(), two);
    }

    #[test]
    fn test_try_into_u128_successful() {
        let expected_u128 = 32_478_829_823_894_127_273_462_167_823_u128; // arbitrary value between 2^64 and 2^128
        let expected_u128_upper_64_bits = (expected_u128 >> 64) as u64;
        let expected_u128_lower_64_bits = (expected_u128 & ((1u128 << 64) - 1)) as u64;

        let u192x192 = U192X192(U384([
            3_972_737_429_871_234_u64, // arbitrary - will be truncated
            98_927_324_u64,            // arbitrary - will be truncated
            7_812_734_871_234_u64,     // arbitrary - will be truncated
            expected_u128_lower_64_bits,
            expected_u128_upper_64_bits,
            0,
        ]));

        assert_eq!(u128::try_from(u192x192).unwrap(), expected_u128);
    }

    #[test]
    fn test_try_into_u128_zero() {
        let u192x192 = U192X192(U384([0, 0, 0, 0, 0, 0]));
        assert_eq!(u128::try_from(u192x192).unwrap(), 0);
    }

    #[test]
    fn test_try_into_u128_overflow() {
        let u192x192 = U192X192(U384([0, 0, 0, 0, 0, 1]));
        assert_matches!(u128::try_from(u192x192), Err(Error::Overflow));
    }

    #[test]
    fn test_integer_cbrt() {
        let u192x192_27 = U192X192::from(27);
        let three = U192X192::from(3);
        assert_eq!(u192x192_27.integer_cbrt(), three);
    }

    #[test]
    // #[allow(clippy::float_cmp)]
    fn test_u192x192_to_f64() {
        assert_eq!(Float::from(U192X192::from(0)), Float::from(0.));
        assert_eq!(
            Float::from(U192X192::from(217_387) / U192X192::from(1_000_000)),
            Float::from(0.217_387)
        );
        assert_eq!(
            Float::from(U192X192::from(71356) / U192X192::from(100)),
            Float::from(713.56)
        );
        assert_eq!(
            Float::from(U192X192::from(211_387_616) / U192X192::from(1000)),
            Float::from(211_387.616)
        );
        assert_eq!(
            Float::from(U192X192::from(372_792_773)),
            Float::from(372_792_773.)
        );
    }
}
