#![allow(unused_imports)]
use crate::{dex, AmountUFP, FeeLiquidityUFP, GrossLiquidityUFP, Liquidity, NetLiquidityUFP};
use dex::v0::RawFeeLevelsArray;
use dex::{
    Amount, Error, FeeLevel, Float, PoolId, PoolInfo, PositionId, PositionInfo, Result, Side, Types,
};

pub mod pool_impl;
pub mod pool_state;
pub use pool_impl::*;
pub use pool_state::*;

use super::{BasisPoints, PositionClosedInfo, PositionInit, PositionOpenedInfo, SwapKind};

/// What fraction of amount-in may be underpaid by a trader in a swap.
/// ```
/// assert_eq!(((1u64<<49) as f64).recip().to_bits(), 0x3c_e0_00_00_00_00_00_00_u64);
/// assert_eq!(1.7763568394002505e-15_f64.to_bits(), 0x3c_e0_00_00_00_00_00_00_u64);
/// ```
pub const SWAP_MAX_UNDERPAY: Float = Float::from_bits(0x3c_e0_00_00_00_00_00_00_u64);

pub trait Pool<T: Types> {
    fn spot_sqrtprice(&self, side: Side, level: FeeLevel) -> Float;

    fn spot_price(&self, side: Side, level: FeeLevel) -> Float;

    fn spot_sqrtprices(&self, side: Side) -> RawFeeLevelsArray<Float>;

    fn pool_info(&self, side: Side) -> Result<PoolInfo, Error>;

    fn eff_sqrtprice(&self, level: FeeLevel, side: Side) -> Float;

    fn liquidity(&self, level: FeeLevel) -> Liquidity;

    fn liquidities(&self) -> RawFeeLevelsArray<Liquidity>;

    fn net_liquidity(&self, level: FeeLevel) -> NetLiquidityUFP;

    fn gross_liquidity(&self, level: FeeLevel) -> GrossLiquidityUFP;

    fn fee_liquidity(&self, level: FeeLevel) -> FeeLiquidityUFP;

    fn position_reserves(&self) -> RawFeeLevelsArray<(AmountUFP, AmountUFP)>;

    fn withdraw_fee(&mut self, position_id: u64) -> Result<(Amount, Amount)>;

    fn withdraw_protocol_fee(&mut self) -> Result<(Amount, Amount)>;

    fn withdraw_fee_and_close_position(&mut self, position_id: u64) -> Result<PositionClosedInfo>;

    fn get_position_info(&self, pool_id: &PoolId, position_id: PositionId) -> Result<PositionInfo>;

    fn open_position(
        &mut self,
        position: PositionInit,
        fee_level: FeeLevel,
        position_id: PositionId,
        factory: &mut dyn dex::ItemFactory<T>,
    ) -> Result<PositionOpenedInfo>;

    fn swap(
        &mut self,
        side: Side,
        swap_type: SwapKind,
        amount: Amount,
        protocol_fee_fraction: BasisPoints,
        price_limit: Option<Float>,
    ) -> Result<(Amount, Amount, u32)>;

    /// Returns:
    ///  - `amount_in`
    ///  - `amount_out`
    ///  - number of tick crossings
    fn swap_to_price(
        &mut self,
        side: Side,
        max_amount_in: Amount,
        max_eff_sqrtprice: Float,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(Amount, Amount, u32)>;

    /// Returns:
    ///  - actually spent `amount_in` (may differ from `amount_in` argument)
    ///  - `amount_out`
    ///  - number of tick crossings
    fn swap_exact_in(
        &mut self,
        side: Side,
        amount_in: Amount,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(Amount, Amount, u32)>;

    /// Returns:
    ///  - `amount_in`
    ///  - `amount_out`
    ///  - number of tick crossings
    fn swap_exact_out(
        &mut self,
        side: Side,
        amount_out: Amount,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(Amount, Amount, u32)>;

    #[cfg(feature = "smart-routing")]
    fn reserves_ratio(&self) -> Liquidity;

    #[cfg(feature = "smart-routing")]
    fn total_liquidity(&self) -> Liquidity;
}

#[cfg(feature = "smartlib")]
pub static mut SWAP_TICKS_COUNTER: usize = 0usize;

#[cfg(feature = "smartlib")]
pub fn get_ticks_counter() -> usize {
    unsafe { SWAP_TICKS_COUNTER }
}

#[cfg(feature = "smartlib")]
pub fn reset_ticks_counter() {
    unsafe {
        SWAP_TICKS_COUNTER = 0usize;
    }
}

#[cfg(feature = "smartlib")]
pub fn inc_ticks_counter(value: usize) {
    unsafe {
        SWAP_TICKS_COUNTER += value;
    }
}
