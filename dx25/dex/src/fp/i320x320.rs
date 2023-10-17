use super::{signed::Signed, u320x320::U320X320, Error, I256X320, U256X320, U320X64};
use crate::chain::Float;
use crate::fp::signed;

pub type I320X320 = Signed<U320X320>;

impl TryFrom<Float> for I320X320 {
    type Error = Error;
    fn try_from(value: Float) -> Result<Self, Self::Error> {
        signed::try_from_float::<U320X320, 10, 5>(value)
    }
}

impl From<I320X320> for Float {
    fn from(v: I320X320) -> Self {
        signed::into_float::<U320X320, 10, 5>(v)
    }
}

impl From<U320X64> for I320X320 {
    fn from(value: U320X64) -> Self {
        I320X320 {
            value: U320X320::from(value),
            non_negative: true,
        }
    }
}

impl TryFrom<I320X320> for I256X320 {
    type Error = Error;
    fn try_from(i320x320: I320X320) -> Result<Self, Self::Error> {
        Ok(Self {
            value: U256X320::try_from(i320x320.value)?,
            non_negative: i320x320.non_negative,
        })
    }
}

impl From<U256X320> for I320X320 {
    fn from(value: U256X320) -> Self {
        I320X320 {
            value: U320X320::from(value),
            non_negative: true,
        }
    }
}
