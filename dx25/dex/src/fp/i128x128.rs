use super::{signed, signed::Signed, u128x128::U128X128, Error, U256X256};
use crate::chain::Float;

pub type I128X128 = Signed<U128X128>;

impl TryFrom<Float> for I128X128 {
    type Error = Error;
    fn try_from(value: Float) -> Result<Self, Self::Error> {
        signed::try_from_float::<U128X128, 4, 2>(value)
    }
}

impl TryFrom<U256X256> for I128X128 {
    type Error = Error;
    fn try_from(value: U256X256) -> Result<Self, Self::Error> {
        Ok(Self {
            value: U128X128::try_from(value)?,
            non_negative: true,
        })
    }
}

impl From<u128> for I128X128 {
    fn from(value: u128) -> Self {
        I128X128 {
            value: U128X128::from(value),
            non_negative: true,
        }
    }
}
