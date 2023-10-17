use super::{dex, NUM_FEE_LEVELS};
use crate::dex::pool::eff_sqrtprice_opposite_side;
use crate::dex::{EffTick, ErrorKind, Tick};
use dex::{FeeLevel, Float, Side};
#[cfg(feature = "near")]
use std::io::Write;
use std::ops::{Deref, DerefMut};
use typed_index_collections::TiSlice;

#[cfg(feature = "concordium")]
use concordium_std::{Deserial, SchemaType, Serial};
#[cfg(feature = "near")]
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};

#[cfg(feature = "multiversx")]
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode, TopDecode, TopEncode},
    {NestedDecode, NestedEncode},
};

#[cfg(feature = "near")]
pub trait Serializable: BorshSerialize + BorshDeserialize {}

#[cfg(feature = "near")]
impl<T: BorshSerialize + BorshDeserialize> Serializable for T {}

#[cfg(feature = "concordium")]
pub trait Serializable: Serial {}

#[cfg(feature = "concordium")]
impl<T: Serial> Serializable for T {}

#[cfg(feature = "multiversx")]
pub trait Serializable: NestedDecode + NestedEncode {}
#[cfg(feature = "multiversx")]
impl<T: NestedDecode + NestedEncode> Serializable for T {}

pub type RawFeeLevelsArray<T> = [T; NUM_FEE_LEVELS as usize];

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "multiversx", derive(NestedDecode, NestedEncode))]
pub struct FeeLevelsArray<T: Serializable>(RawFeeLevelsArray<T>);

#[cfg(feature = "near")]
impl<T: Serializable> BorshSerialize for FeeLevelsArray<T> {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        for item in &self.0 {
            item.serialize(writer)?;
        }

        Ok(())
    }
}

// Custom BorshDeserialize impl, because default implementation for arrays requires Default, but LookupMap isn't Default.
#[cfg(feature = "near")]
impl<T: Serializable> BorshDeserialize for FeeLevelsArray<T> {
    fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
        let mut array: [Option<T>; NUM_FEE_LEVELS as usize] = Default::default();
        for item in array.iter_mut().take(NUM_FEE_LEVELS as usize) {
            *item = Some(T::deserialize(buf)?);
        }
        Ok(Self(array.map(|item| item.unwrap())))
    }
}

#[cfg(feature = "concordium")]
mod concordium_traits_impl {
    use super::{FeeLevelsArray, Serializable, NUM_FEE_LEVELS};
    use concordium_std::{Deletable, DeserialWithState, HasStateApi, ParseResult, Read, Serial};

    impl<T: Serializable> Serial for FeeLevelsArray<T> {
        fn serial<W: concordium_std::Write>(&self, out: &mut W) -> Result<(), W::Err> {
            for item in &self.0 {
                item.serial(out)?;
            }
            Ok(())
        }
    }

    impl<S: HasStateApi, T: Serializable + DeserialWithState<S>> DeserialWithState<S>
        for FeeLevelsArray<T>
    {
        fn deserial_with_state<R: Read>(state: &S, source: &mut R) -> ParseResult<Self> {
            let mut array: [Option<T>; NUM_FEE_LEVELS as usize] = Default::default();
            for item in &mut array {
                *item = Some(T::deserial_with_state(state, source)?);
            }
            Ok(Self(array.map(|item| item.unwrap())))
        }
    }

    impl<T: Serializable + Deletable> Deletable for FeeLevelsArray<T> {
        fn delete(self) {
            for item in self.0 {
                item.delete();
            }
        }
    }
}

impl<T: Serializable + Copy + Default> Default for FeeLevelsArray<T> {
    fn default() -> Self {
        Self::from_value(Default::default())
    }
}

impl<T: Serializable> From<RawFeeLevelsArray<T>> for FeeLevelsArray<T> {
    fn from(array: RawFeeLevelsArray<T>) -> Self {
        Self(array)
    }
}

impl<T: Serializable> From<FeeLevelsArray<T>> for RawFeeLevelsArray<T> {
    fn from(array: FeeLevelsArray<T>) -> Self {
        array.0
    }
}

impl<T: Serializable + Copy> FeeLevelsArray<T> {
    pub fn from_value(value: T) -> Self {
        Self([value; NUM_FEE_LEVELS as usize])
    }
}

impl<T: Serializable> FeeLevelsArray<T> {
    pub fn from_fn<F: FnMut(usize) -> T>(f: F) -> Self {
        Self(std::array::from_fn(f))
    }
}

impl<T: Serializable> Deref for FeeLevelsArray<T> {
    type Target = TiSlice<FeeLevel, T>;

    fn deref(&self) -> &Self::Target {
        TiSlice::from_ref(&self.0)
    }
}

impl<T: Serializable> DerefMut for FeeLevelsArray<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        TiSlice::from_mut(&mut self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
#[cfg_attr(feature = "near", derive(BorshDeserialize, BorshSerialize))]
#[cfg_attr(feature = "concordium", derive(Serial, Deserial, SchemaType))]
#[cfg_attr(
    feature = "multiversx",
    derive(TopDecode, TopEncode, NestedDecode, NestedEncode)
)]
pub struct EffSqrtprices(pub Float, pub Float);

impl EffSqrtprices {
    pub fn from_value(
        eff_sqrtprice: Float,
        side: Side,
        fee_level: FeeLevel,
        pivot: Option<EffTick>,
    ) -> Result<Self, ErrorKind> {
        Ok(match side {
            Side::Left => Self(
                eff_sqrtprice,
                eff_sqrtprice_opposite_side(eff_sqrtprice, fee_level, pivot)?,
            ),
            Side::Right => Self(
                eff_sqrtprice_opposite_side(eff_sqrtprice, fee_level, pivot)?,
                eff_sqrtprice,
            ),
        })
    }

    pub fn from_tick(tick: &Tick, fee_level: FeeLevel) -> Self {
        Self(
            tick.eff_sqrtprice(fee_level, Side::Left),
            tick.eff_sqrtprice(fee_level, Side::Right),
        )
    }

    pub fn swap_if(self, is_swap: bool) -> Self {
        if is_swap {
            Self(self.1, self.0)
        } else {
            self
        }
    }

    pub fn left(&self) -> Float {
        self.0
    }

    pub fn right(&self) -> Float {
        self.1
    }

    pub fn value(&self, side: Side) -> Float {
        match side {
            Side::Left => self.left(),
            Side::Right => self.right(),
        }
    }

    pub fn as_array(&self) -> [Float; 2] {
        [self.0, self.1] // todo: transmute?
    }

    pub fn as_tuple(&self) -> (Float, Float) {
        (self.0, self.1) // todo: transmute?
    }

    pub fn as_tuple_swapped_if(&self, cond: bool) -> (Float, Float) {
        if cond {
            (self.1, self.0)
        } else {
            (self.0, self.1)
        }
    }
}
