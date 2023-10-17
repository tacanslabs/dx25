use bitvec::macros::internal::funty::Fundamental;
use bitvec::prelude::*;
use itertools::Itertools;

use super::Float;
use crate::chain::{MAX_TICK, MIN_TICK};

#[cfg(feature = "near")]
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
#[cfg(feature = "near")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "concordium")]
use concordium_std::{Deserial, SchemaType, Serial};

use crate::dex::pool::fee_rate_ticks;
use crate::dex::{ErrorKind, FeeLevel, Side};
use crate::{MAX_EFF_TICK, MIN_EFF_TICK};
#[cfg(feature = "multiversx")]
use multiversx_sc::derive::TypeAbi;
#[cfg(feature = "multiversx")]
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode, TopDecode, TopEncode},
};

/// generated with test:
///
///   ```bash
///   cd core/veax/dex
///   cargo test test_precalculate_ticks_bit_repr -- --nocapture
///   ```
///
#[allow(clippy::unreadable_literal)]
pub const PRECALCULATED_TICKS: [u64; 21] = [
    4607182643974369558,
    4607182869159980145,
    4607183319564978878,
    4607184220510102349,
    4607186022940979433,
    4607189629966263589,
    4607196852679033204,
    4607211332818125533,
    4607240432470062669,
    4607299193450302128,
    4607418995971640537,
    4607668000704051496,
    4608205938457857923,
    4609462070376259803,
    4612290832146940624,
    4617480469329378893,
    4628148512120721768,
    4649381992504848318,
    4692198734602598674,
    4777248888797670312,
    4947442543280771895,
];

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Copy, Clone, Debug, Default, Ord, PartialOrd, Eq, PartialEq)]
#[cfg_attr(
    feature = "near",
    derive(BorshSerialize, BorshDeserialize, Deserialize, Serialize)
)]
#[cfg_attr(feature = "concordium", derive(Serial, Deserial, SchemaType))]
#[cfg_attr(
    feature = "multiversx",
    derive(TopDecode, TopEncode, NestedDecode, NestedEncode, TypeAbi)
)]
#[cfg_attr(
    all(feature = "concordium", feature = "smartlib"),
    derive(serde::Serialize)
)]
#[repr(transparent)]
/// A point on the price scale which corresponds to a specific _spot_ price
pub struct Tick(i32);

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Copy, Clone, Debug, Default, Ord, PartialOrd, Eq, PartialEq)]
#[cfg_attr(
    feature = "near",
    derive(BorshSerialize, BorshDeserialize, Deserialize, Serialize)
)]
#[cfg_attr(feature = "concordium", derive(Serial, Deserial, SchemaType))]
#[cfg_attr(
    feature = "multiversx",
    derive(TopDecode, TopEncode, NestedDecode, NestedEncode, TypeAbi)
)]
#[cfg_attr(
    all(feature = "concordium", feature = "smartlib"),
    derive(serde::Serialize)
)]
#[repr(transparent)]
/// A point on the price scale which corresponds to a specific _effective_ price
pub struct EffTick(i32);

impl Tick {
    pub const BASE: Float = Float::from_bits(PRECALCULATED_TICKS[0]);
    pub const MIN: Self = Self(MIN_TICK);
    pub const MAX: Self = Self(MAX_TICK);

    pub fn new(value: i32) -> Result<Self, ErrorKind> {
        if Self::is_valid(value) {
            Ok(Self(value))
        } else {
            Err(ErrorKind::PriceTickOutOfBounds)
        }
    }

    /// # Safety
    ///
    /// This function should be called only with values for which `Tick::is_valid` return true
    pub const unsafe fn new_unchecked(value: i32) -> Self {
        Self(value)
    }

    pub const fn is_valid(value: i32) -> bool {
        MIN_TICK <= value && value <= MAX_TICK
    }

    pub const fn index(&self) -> i32 {
        self.0
    }

    pub const fn to_opt_index(&self) -> Option<i32> {
        if MIN_TICK < self.index() && self.index() < MAX_TICK {
            Some(self.index())
        } else {
            None
        }
    }

    /// For a given `swap_direction`, returns tick with same effective price on `other_level`
    /// as this tick has on `this_level`.
    pub fn with_same_eff_price(
        self,
        this_level: FeeLevel,
        other_level: FeeLevel,
        swap_direction: Side,
    ) -> Result<Self, ErrorKind> {
        EffTick::from_tick(self, this_level, swap_direction).to_tick(other_level, swap_direction)
    }

    /// Spot sqrtprice corresponding to a tick, for a left-side (i.e. forward direction) swap.
    pub fn spot_sqrtprice(&self) -> Float {
        self.index()
            .abs()
            .as_u32()
            .view_bits::<Lsb0>() // least significant bit has position 0 as opposite to Msb0
            .iter_ones()
            // safe because tick values are validated when tick created
            // so bit index cannot exceed range of precalculated ticks
            .map(|index| unsafe { *PRECALCULATED_TICKS.get_unchecked(index) })
            .map(Float::from_bits)
            .product1()
            .map_or(Float::one(), |scale_by| {
                if self.index().is_positive() {
                    scale_by
                } else {
                    scale_by.recip()
                }
            })
    }

    /// Effective sqrtprice corresponding to a tick, for a given fee level and swap direciton.
    pub fn eff_sqrtprice(&self, fee_level: FeeLevel, side: Side) -> Float {
        EffTick::from_tick(*self, fee_level, side).eff_sqrtprice()
    }

    /// Tick corresponding to the opposite spot sqrtprice
    pub fn opposite(&self) -> Self {
        // unwrap will succeed as long as tick itself is valid and the range of valid ticks is symmetric
        Tick::new(-self.index()).unwrap()
    }

    /// Convenience function allowing to take the opposite tick conditionally
    pub fn opposite_if(&self, is_opposite: bool) -> Self {
        if is_opposite {
            self.opposite()
        } else {
            *self
        }
    }

    pub fn unwrap_range(as_options: (Option<i32>, Option<i32>)) -> Result<(Tick, Tick), ErrorKind> {
        Ok((
            match as_options.0 {
                Some(tick_low) => Tick::new(tick_low)?,
                None => Tick::MIN,
            },
            match as_options.1 {
                Some(tick_high) => Tick::new(tick_high)?,
                None => Tick::MAX,
            },
        ))
    }

    pub fn wrap_range(as_ticks: (Tick, Tick)) -> (Option<i32>, Option<i32>) {
        (
            if as_ticks.0 <= Tick::MIN {
                None
            } else {
                Some(as_ticks.0.index())
            },
            if as_ticks.1 >= Tick::MAX {
                None
            } else {
                Some(as_ticks.1.index())
            },
        )
    }
}

impl EffTick {
    pub const fn is_valid(index: i32) -> bool {
        MIN_EFF_TICK <= index && index <= MAX_EFF_TICK
    }

    pub fn new(index: i32) -> Result<Self, ErrorKind> {
        if Self::is_valid(index) {
            Ok(Self(index))
        } else {
            Err(ErrorKind::PriceTickOutOfBounds)
        }
    }

    pub const fn index(&self) -> i32 {
        self.0
    }

    pub fn from_tick(tick: Tick, fee_level: FeeLevel, side: Side) -> EffTick {
        let eff_tick_index = match side {
            Side::Left => tick.index() + i32::from(fee_rate_ticks(fee_level)),
            Side::Right => -tick.index() + i32::from(fee_rate_ticks(fee_level)),
        };
        // Unwrap will succeed as long as `tick` is valid.
        // See `test_eff_tick_from_tick_and_opposite_succeeds`
        EffTick::new(eff_tick_index).unwrap()
    }

    pub fn to_tick(&self, fee_level: FeeLevel, side: Side) -> Result<Tick, ErrorKind> {
        let tick_index = match side {
            Side::Left => self.index() - i32::from(fee_rate_ticks(fee_level)),
            Side::Right => -self.index() + i32::from(fee_rate_ticks(fee_level)),
        };
        Tick::new(tick_index)
    }

    pub fn eff_sqrtprice(&self) -> Float {
        // The constructed tick is not strictly valid, but as long as `self.index()` is within
        // MIN_EFF_TICK..=MAX_EFF_TICK range, the spot price is still calculateable.
        // See `test_eff_sqrtprice_for_extreme_eff_ticks_succeed`.
        Tick(self.index()).spot_sqrtprice()
    }

    pub fn opposite(&self, fee_level: FeeLevel) -> Self {
        let opposite_eff_tick_index = -self.index() + 2_i32.pow(u32::from(fee_level) + 1);
        debug_assert!(Self::is_valid(opposite_eff_tick_index));
        // unwrap will succeed as long as the effective tick itself is valid
        // See `test_eff_tick_from_tick_and_opposite_succeeds`
        EffTick::new(opposite_eff_tick_index).unwrap()
    }

    pub fn shifted(&self, step: i32) -> Result<Self, ErrorKind> {
        EffTick::new(self.index() + step)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EffTick, FeeLevel, Float, Side, Tick, MAX_EFF_TICK, MAX_TICK, MIN_EFF_TICK, MIN_TICK,
        PRECALCULATED_TICKS,
    };
    use crate::assert_eq_rel_tol;
    use crate::chain::NUM_PRECALCULATED_TICKS;
    use crate::dex::pool::eff_sqrtprice_opposite_side;
    use crate::dex::utils::{next_down, next_up};
    use bitvec::macros::internal::funty::Fundamental;
    use bitvec::order::Lsb0;
    use bitvec::view::BitView;
    use itertools::Itertools;
    use rstest::rstest;
    use rug::ops::Pow;

    #[rstest]
    #[case::success_zero(0)]
    #[case::success_min(MIN_TICK)]
    #[case::success_max(MAX_TICK)]
    #[should_panic(expected = "invalid Tick value")]
    #[case::failed_less_than_min(MIN_EFF_TICK - 1)]
    #[should_panic(expected = "invalid Tick value")]
    #[case::failed_more_than_max(MAX_EFF_TICK + 1)]
    fn create_tick_with_limited_range_of_value(#[case] value: i32) {
        let tick = Tick::new(value).expect("invalid Tick value");

        assert_eq!(tick.index(), value);
    }

    #[rstest]
    #[case::max_tick(MAX_TICK)]
    #[case::all_ones(0b111_1111_1111_1111_1111)]
    #[case::large_pos1(283_784)]
    #[case::large_pos2(21_114)]
    #[case::one(1)]
    #[case::zero(0)]
    #[case::neg_one(-1)]
    #[case::large_neg1(-1784)]
    #[case::large_neg2(-41114)]
    #[case::min_tick(MIN_TICK)]
    fn test_spot_sqrtprice(#[case] tick_number: i32) {
        let actual_sqrtprice = Tick::new(tick_number).unwrap().spot_sqrtprice();
        let precision = 500;
        let tick_base =
            rug::Float::with_val(precision, rug::Float::parse("1.0001").unwrap()).sqrt();
        let expected_sqrtprice = Float::from(tick_base.pow(tick_number).to_f64());
        assert_eq_rel_tol!(actual_sqrtprice, expected_sqrtprice, 3);
    }

    #[rstest]
    fn test_eff_sqrtprice_for_extreme_ticks_succeed(
        #[values(MIN_TICK, MIN_TICK + 1, MAX_TICK - 1, MAX_TICK)] tick_index: i32,
        #[values(Side::Left, Side::Right)] side: Side,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        Tick::new(tick_index)
            .unwrap()
            .eff_sqrtprice(fee_level, side);
    }

    #[rstest]
    fn test_eff_sqrtprice_for_extreme_eff_ticks_succeed(
        #[values(MIN_EFF_TICK, MIN_EFF_TICK + 1, MAX_EFF_TICK - 1, MAX_EFF_TICK)]
        eff_tick_index: i32,
    ) {
        EffTick::new(eff_tick_index).unwrap().eff_sqrtprice();
    }

    #[rstest]
    fn test_eff_tick_from_tick_and_opposite_succeeds(
        #[values(MIN_TICK,
                 MIN_TICK + 1,
                 MIN_TICK + 7,
                 0,
                 MAX_TICK - 7,
                 MAX_TICK - 1,
                 MAX_TICK)]
        tick_index: i32,
        #[values(Side::Left, Side::Right)] side: Side,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        let tick = Tick::new(tick_index).unwrap();
        // unwrap inside EffTick::from_tick succeeds:
        let eff_tick = EffTick::from_tick(tick, fee_level, side);
        // unwrap inside EffTick::opposite succeeds:
        eff_tick.opposite(fee_level);
    }

    #[rstest]
    #[case(MAX_TICK)]
    #[case(1)]
    #[case(0)]
    #[case(-1)]
    #[case(MIN_TICK)]
    fn scale_back_and_forth(#[case] tick: i32) {
        let positive_tick = Tick::new(tick).unwrap();
        let negative_tick = Tick::new(-tick).unwrap();
        let price = Float::one();
        let upscaled_price = price * positive_tick.spot_sqrtprice();
        let downscaled_price = upscaled_price * negative_tick.spot_sqrtprice();
        assert_eq!(price, downscaled_price);
    }

    #[rstest]
    fn max_bit_index_for_price_tick() {
        let ones = MAX_TICK
            .abs()
            .as_u32()
            .view_bits::<Lsb0>()
            .iter_ones()
            .collect_vec();

        assert!(ones.iter().max().unwrap() < &NUM_PRECALCULATED_TICKS);
    }

    /// Pick ticks on different level, which should have equal effective sqrtprice,
    /// and check that the effective sqrtprice is indeed exactly equal.
    #[rstest]
    #[case(MAX_TICK)]
    #[case(3800)]
    #[case(1)]
    #[case(0)]
    #[case(-1)]
    #[case(-2200)]
    #[case(MIN_TICK)]
    fn eff_sqrtprices_on_different_levels_match(#[case] tick: i32) {
        // By definition, effective price of tick T on level L is 2^L ticks higher
        // (i.e. is the same as spot sqrtprice of tick T+2^L).
        // So ticks
        //    T-1 on level 0
        //    T-2 on level 1
        //    T-4 on level 2
        //    T-8 on level 3
        //    ...
        //    T-128 on level 7
        // should have equal effective sqrtprice:
        let eff_sqrtprice_tier0 = Tick(tick - 1).eff_sqrtprice(0, Side::Left);
        assert_eq!(
            eff_sqrtprice_tier0,
            Tick(tick - 2).eff_sqrtprice(1, Side::Left)
        );
        assert_eq!(
            eff_sqrtprice_tier0,
            Tick(tick - 4).eff_sqrtprice(2, Side::Left)
        );
        assert_eq!(
            eff_sqrtprice_tier0,
            Tick(tick - 8).eff_sqrtprice(3, Side::Left)
        );
        assert_eq!(
            eff_sqrtprice_tier0,
            Tick(tick - 16).eff_sqrtprice(4, Side::Left)
        );
        assert_eq!(
            eff_sqrtprice_tier0,
            Tick(tick - 32).eff_sqrtprice(5, Side::Left)
        );
        assert_eq!(
            eff_sqrtprice_tier0,
            Tick(tick - 64).eff_sqrtprice(6, Side::Left)
        );
        assert_eq!(
            eff_sqrtprice_tier0,
            Tick(tick - 128).eff_sqrtprice(7, Side::Left)
        );
    }

    /// Check that `PRECALCULATED_TICKS` are exactly what they should be.
    #[test]
    fn test_precalculated_ticks() {
        for (power, _bit_repr) in PRECALCULATED_TICKS.iter().enumerate() {
            let precision = 500;
            let tick_base =
                rug::Float::with_val(precision, rug::Float::parse("1.0001").unwrap()).sqrt();
            #[allow(clippy::cast_possible_truncation)]
            let tick_number = 2.pow(power as u32);
            let expected_rug: rug::Float = tick_base.pow(tick_number);
            let expected: Float = expected_rug.to_f64().into();
            let actual: Float = Tick(tick_number).spot_sqrtprice();
            #[allow(clippy::float_cmp)]
            let exactly_equal = actual == expected;
            assert!(
                exactly_equal,
                "Values are not equal. Actual {actual} vs expected {expected}"
            );
        }
    }

    /// See the comment to `assert_ordering_holds_for_opposite_side`.
    ///
    /// The test takes ~1/2 hour per case for VEAX and is impractically long for CDEX and DX25,
    /// so we don't run it automatically.
    #[ignore]
    #[rstest]
    fn test_ordering_holds_for_opposite_side_exhaustive(
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] level: FeeLevel,
        #[values(Side::Left, Side::Right)] side: Side,
    ) {
        for index in MIN_TICK..=MAX_TICK {
            assert_ordering_holds_for_opposite_side(index, level, side);
        }
    }

    /// See the comment to `assert_ordering_holds_for_opposite_side`
    #[rstest]
    fn test_ordering_holds_for_opposite_side(
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] level: FeeLevel,
        #[values(Side::Left, Side::Right)] side: Side,
    ) {
        use std::collections::btree_set::BTreeSet;

        let mut indices = BTreeSet::new();

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        for index_base in (0..=(f64::from(MAX_TICK).log2() as u32)).map(|p| 2_i32.pow(p)) {
            for index_offset in -130..=130 {
                let index = index_base + index_offset;
                if index <= MAX_TICK {
                    indices.insert(index);
                }

                let index = -index_base + index_offset;
                if index >= MIN_TICK {
                    indices.insert(index);
                }
            }
        }

        for index_offset in 0..=130 {
            indices.insert(MIN_TICK + index_offset);
            indices.insert(MAX_TICK - index_offset);
        }

        for index in indices {
            assert_ordering_holds_for_opposite_side(index, level, side);
        }
    }

    /// Check price ordering: e.g. some effective price for left-side swap is below tick T,
    /// then for right-side swap direction the corresponding effective price must be above tick T.
    fn assert_ordering_holds_for_opposite_side(index: i32, level: FeeLevel, side: Side) {
        let tick = Tick::new(index).unwrap();
        let tick_eff_sqrtprice = tick.eff_sqrtprice(level, side);
        let lower_eff_sqrtprice = next_down(tick_eff_sqrtprice);
        let higher_eff_sqrtprice = next_up(tick_eff_sqrtprice);
        let opposite_of_tick_eff_sqrtprice = tick.eff_sqrtprice(level, side.opposite());
        let pivot = EffTick::from_tick(tick, level, side);
        let opposite_of_lower_eff_sqrtprice =
            eff_sqrtprice_opposite_side(lower_eff_sqrtprice, level, Some(pivot)).unwrap();
        let opposite_of_higher_eff_sqrtprice =
            eff_sqrtprice_opposite_side(higher_eff_sqrtprice, level, Some(pivot)).unwrap();

        assert!(
            opposite_of_lower_eff_sqrtprice >= opposite_of_tick_eff_sqrtprice
                && opposite_of_higher_eff_sqrtprice <= opposite_of_tick_eff_sqrtprice,
            "Opposite-side eff.sqrtprice ordering violation for:\n\
            tick: {index},\n\
            level: {level},\n\
            side: {side:?},\n\
            lower_eff_sqrtprice             : {lower_eff_sqrtprice}\n\
            tick_eff_sqrtprice              : {tick_eff_sqrtprice}\n\
            higher_eff_sqrtprice            : {higher_eff_sqrtprice}\n\
            opposite_of_lower_eff_sqrtprice : {opposite_of_lower_eff_sqrtprice}\n\
            opposite_of_tick_eff_sqrtprice  : {opposite_of_tick_eff_sqrtprice}\n\
            opposite_of_higher_eff_sqrtprice: {opposite_of_higher_eff_sqrtprice}\n"
        );
    }

    /// Generate precalculated tick values
    #[test]
    fn test_precalculate_ticks_bit_repr() {
        #[allow(clippy::cast_possible_truncation)]
        let num_precalculated_ticks = PRECALCULATED_TICKS.len() as u32;
        for power in 0..num_precalculated_ticks {
            let precision = 500;
            let tick_base =
                rug::Float::with_val(precision, rug::Float::parse("1.0001").unwrap()).sqrt();
            let tick_number = 2.pow(power);
            let value_rug: rug::Float = tick_base.pow(tick_number);
            let value: f64 = value_rug.to_f64();
            let bits = value.to_bits();
            println!("{bits},");
        }
    }
}
