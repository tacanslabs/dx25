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

use super::{Error, U1024, U512, U704};
use crate::chain::Float;
use crate::fp::try_float_to_ufp::try_float_to_ufp;
use crate::fp::{U128X128, U192X64, U320X64};
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
pub struct U320X192(pub U512);

impl U320X192 {
    pub const fn one() -> Self {
        U320X192(U512([0, 0, 0, 1, 0, 0, 0, 0]))
    }

    pub const fn fract(self) -> Self {
        // the fractional part is saved in the zeroth value
        // of the underlying array
        U320X192(U512([
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            0,
            0,
            0,
            0,
            0,
        ]))
    }

    pub const fn floor(self) -> Self {
        // the integer part is saved in the first value
        // of the underlying array
        U320X192(U512([
            0,
            0,
            0,
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
            self.0 .0[6],
            self.0 .0[7],
        ]))
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
        U320X192(self.0.integer_sqrt() << 96)
    }

    pub fn integer_cbrt(self) -> Self {
        let mut inner = self.0;

        let mut s = 255;
        let mut y = U512::zero();
        let mut b;
        let one = U512::one();
        while s >= 0 {
            y += y;
            b = U512::from(3) * y * (y + one) + one;
            if (inner >> s) >= b {
                inner -= b << s;
                y += one;
            }
            s -= 3;
        }
        U320X192(U512::from([
            0, 0, y.0[0], y.0[1], y.0[2], y.0[3], y.0[4], y.0[5],
        ]))
    }
}

impl From<u128> for U320X192 {
    fn from(value: u128) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let lower_word = value as u64;
        let upper_word = (value >> 64) as u64;
        U320X192(U512([0, 0, 0, lower_word, upper_word, 0, 0, 0]))
    }
}

impl From<U128X128> for U320X192 {
    fn from(v: U128X128) -> Self {
        U320X192(U512([
            0, v.0 .0[0], v.0 .0[1], v.0 .0[2], v.0 .0[3], 0, 0, 0,
        ]))
    }
}

impl From<[u64; 8]> for U320X192 {
    fn from(array: [u64; 8]) -> Self {
        Self(U512(array))
    }
}

impl From<U192X64> for U320X192 {
    fn from(v: U192X64) -> Self {
        U320X192(U512([
            0, 0, v.0 .0[0], v.0 .0[1], v.0 .0[2], v.0 .0[3], 0, 0,
        ]))
    }
}

impl TryFrom<Float> for U320X192 {
    type Error = Error;
    fn try_from(value: Float) -> Result<Self, Self::Error> {
        try_float_to_ufp::<U320X192, 8, 3>(value)
    }
}

impl ops::Add for U320X192 {
    type Output = Self;

    fn add(self, rhs: U320X192) -> Self {
        U320X192(self.0 + rhs.0)
    }
}

impl ops::AddAssign for U320X192 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl ops::Sub for U320X192 {
    type Output = Self;

    fn sub(self, rhs: U320X192) -> Self {
        U320X192(self.0 - rhs.0)
    }
}

impl ops::SubAssign for U320X192 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl Sum for U320X192 {
    fn sum<I: Iterator<Item = U320X192>>(iter: I) -> Self {
        let mut s = U320X192::zero();
        for i in iter {
            s += i;
        }
        s
    }
}

impl<'a> Sum<&'a Self> for U320X192 {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        let mut s = U320X192::zero();
        for i in iter {
            s += *i;
        }
        s
    }
}

impl ops::Mul for U320X192 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        // The underlying U512s are multiplied exactly, in sufficiently high precision,
        // and converted to U320X192 taking the scale into account and truncating excessive precision.
        // As the product must fit into U320X192, it is sufficient to perfrom
        // the multiplication in 576 (i.e. 3x192) bits:
        // U320X192 x U320X192 = U512/2**192 x U512/2**192 = U576/2**384 = U192x384  -->  U320X192

        let self_u704 = U704([
            self.0 .0[0],
            self.0 .0[1],
            self.0 .0[2],
            self.0 .0[3],
            self.0 .0[4],
            self.0 .0[5],
            self.0 .0[6],
            self.0 .0[7],
            0,
            0,
            0,
        ]);
        let rhs_u704 = U704([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            rhs.0 .0[6],
            rhs.0 .0[7],
            0,
            0,
            0,
        ]);

        // The product of two U320X192 may not necessarily fit into U320X192,
        // so we need to check for overflow:
        let (res_u704, is_overflow) = self_u704.overflowing_mul(rhs_u704);
        assert!(!is_overflow, "{}", Error::Overflow);

        // Scale the product back to U320X192:
        U320X192(U512([
            res_u704.0[3],
            res_u704.0[4],
            res_u704.0[5],
            res_u704.0[6],
            res_u704.0[7],
            res_u704.0[8],
            res_u704.0[9],
            res_u704.0[10],
        ]))
    }
}

impl ops::Div for U320X192 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        // as we divide 2 fractions with the same denominator (namely 2^192)
        // we are getting a value without a denominator
        // we need to multiply by this denominator to respect the definition
        // doing this is the same as moving the underlying array
        // by three u64 value to the right
        let self_u1024_mul_2_196 = U1024([
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
            0,
            0,
            0,
            0,
            0,
        ]);
        let rhs_u1024 = U1024([
            rhs.0 .0[0],
            rhs.0 .0[1],
            rhs.0 .0[2],
            rhs.0 .0[3],
            rhs.0 .0[4],
            rhs.0 .0[5],
            rhs.0 .0[6],
            rhs.0 .0[7],
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]);

        let res_u1024 = self_u1024_mul_2_196 / rhs_u1024;
        // assure no overflows happen
        assert!(
            res_u1024.0[8] == 0
                && res_u1024.0[9] == 0
                && res_u1024.0[10] == 0
                && res_u1024.0[11] == 0
                && res_u1024.0[12] == 0
                && res_u1024.0[13] == 0
                && res_u1024.0[14] == 0
                && res_u1024.0[15] == 0,
            "{}",
            Error::Overflow
        );

        U320X192(U512([
            res_u1024.0[0],
            res_u1024.0[1],
            res_u1024.0[2],
            res_u1024.0[3],
            res_u1024.0[4],
            res_u1024.0[5],
            res_u1024.0[6],
            res_u1024.0[7],
        ]))
    }
}

impl Zero for U320X192 {
    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
    fn zero() -> Self {
        Self(U512::zero())
    }
    fn set_zero(&mut self) {
        self.0.set_zero();
    }
}

impl From<U320X64> for U320X192 {
    fn from(v: U320X64) -> Self {
        U320X192(U512([
            0, 0, v.0 .0[0], v.0 .0[1], v.0 .0[2], v.0 .0[3], v.0 .0[4], v.0 .0[5],
        ]))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sum() {
        let one = U320X192(U512::one());
        let two = U320X192(U512::one() * 2);
        assert_eq!(one + one, two);
    }

    #[test]
    fn test_sub() {
        let one = U320X192(U512::one());
        let two = U320X192(U512::one() * 2);
        assert_eq!(two - one, one);
    }

    #[test]
    fn test_mul() {
        let real_one = U320X192(U512::one() << 192);
        let real_two = U320X192((U512::one() << 192) * 2);
        assert_eq!(real_two * real_one, real_two);
    }

    #[test]
    #[should_panic(expected = "Numeric overflow")]
    fn test_mul_overflow() {
        let two_pow_90 = U320X192::from(1u128 << 90);
        let two_pow_180 = two_pow_90 * two_pow_90;
        let _: U320X192 = two_pow_180 * two_pow_180;
    }

    #[test]
    fn test_mul_moderate_integers() {
        let v1 = 7_362_374_734_662_634_773_247_u128;
        let v2 = 7_362_467_237_u128;
        assert_eq!(
            U320X192::from(v1) * U320X192::from(v2),
            U320X192::from(v1 * v2)
        );
    }

    #[test]
    fn test_div() {
        let real_one = U320X192(U512::one() << 192);
        let real_two = U320X192((U512::one() << 192) * 2);
        assert_eq!(real_two / real_one, real_two);
    }

    #[test]
    fn test_floor() {
        let unit = U512::one() << 64;
        let cases = [
            U320X192(unit * 2),
            U320X192(unit * 34141),
            U320X192(unit * 1_435_134_134),
            U320X192(unit * 1),
            U320X192((unit >> 2) + 111),
            U320X192((unit >> 34) + 33),
            U320X192(unit << 32),
        ];
        for x in cases {
            println!("Case: {}", x.0);
            assert_eq!(x.floor(), U320X192((x.0 >> 192) << 192));
        }
    }

    #[test]
    fn test_ceil() {
        assert_eq!((U320X192::from(0)).ceil(), U320X192::from(0));
        assert_eq!(U320X192::from(237).ceil(), U320X192::from(237));
        assert_eq!(
            (U320X192::from(115) / U320X192::from(10)).ceil(),
            U320X192::from(12)
        );
        assert_eq!(
            (U320X192::from(723_000_000_001) / U320X192::from(1_000_000_000)).ceil(),
            U320X192::from(724)
        );
    }

    #[test]
    fn test_fract() {
        let unit = U512::one();
        let cases = [
            U320X192(unit * 2),
            U320X192(unit * 34141),
            U320X192(unit * 1_435_134_134),
            U320X192(unit * 6),
            U320X192((unit << 2) + 111),
            U320X192((unit << 34) + 13),
            U320X192(unit << 32),
            U320X192((unit << 64) + 1231),
        ];
        for x in cases {
            println!("Case: {}", x.0);
            assert_eq!(x.fract(), U320X192((x.0 << 64) >> 64));
        }
    }

    #[test]
    fn test_integer_sqrt() {
        let four = U320X192::from(4);
        let two = U320X192::from(2);
        assert_eq!(four.integer_sqrt(), two);
    }

    #[test]
    fn test_integer_cbrt() {
        let u320x192_27 = U320X192::from(27);
        let three = U320X192::from(3);
        assert_eq!(u320x192_27.integer_cbrt(), three);
    }
}
