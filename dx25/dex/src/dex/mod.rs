pub use dex_impl::{estimations::Estimations, AccountCallbackType, Dex};
pub use errors::*;
pub use primitives::*;
pub use state_types::*;
pub use tick::*;
pub use traits::{
    AccountExtra, AccountWithdrawTracker, ItemFactory, KeyAt, Logger, Map, MapRemoveKey,
    OrderedMap, Persistent, Set, State, StateMembersMut, StateMut, Types, WasmApi,
};
pub use util_types::*;
pub use utils::PairExt;

mod dex_impl;
mod errors;
pub mod pool;
mod primitives;
mod traits;
mod util_types;
mod utils;

pub mod map_with_context;
pub mod state_types;
pub mod tick;

mod rational_fraction;
pub use crate::chain::{
    Amount, FeeLiquidityUFP, GrossLiquidityUFP, LongestSFP, LongestUFP, NetLiquiditySFP,
    NetLiquidityUFP,
};
pub use rational_fraction::Fraction;

#[cfg(test)]
mod dex_tests_liquidity;
#[cfg(test)]
mod dex_tests_swap;
#[cfg(any(test, feature = "test-utils"))]
#[cfg_attr(feature = "test-utils", allow(unused_imports, dead_code))]
pub mod test_utils;

pub mod collection_helpers;
pub mod tick_state_ex;
pub mod v0;
pub mod withdraw_trackers;

pub use v0 as latest;

pub type BasisPoints = u16;
pub type PositionId = u64;
pub type FeeLevel = u8;
pub type PoolsNumber = usize;

pub const BASIS_POINT_DIVISOR: BasisPoints = 10_000;

pub const MIN_PROTOCOL_FEE_FRACTION: BasisPoints = 1;
pub const MAX_PROTOCOL_FEE_FRACTION: BasisPoints = BASIS_POINT_DIVISOR / 2;

/// Minimal net liquidity required to open a position.
///
/// Should be not too large to enable opening positions with broad range.
/// Should be not too small to limit the error of truncation to 32 frational bits.
/// Current value is chosen to avoid precision loss in conversion to ...X64 types
/// ```
/// assert_eq!(((1 << (64 - f64::MANTISSA_DIGITS)) as f64).recip().to_bits(), 0x3f_40_00_00_00_00_00_00_u64);
/// ```
pub const MIN_NET_LIQUIDITY: Float = Float::from_bits(0x3f_40_00_00_00_00_00_00_u64);

/// Maximum net liquidity, that each individual position may not exceed.
///
/// Should be not too big to allow opening multiple positions without overflowing total liquidity.
///
/// Maximum liquidity is set to be 143 (128+15) bits.
/// This should allow to create 2^(192-143)=2^49 positions which should be enough.
/// ```
/// assert_eq!(143.0f64.exp2().to_bits(), 0x48_e0_00_00_00_00_00_00_u64);
/// ```
#[cfg(any(feature = "near", feature = "multiversx"))]
pub const MAX_NET_LIQUIDITY: Float = Float::from_bits(0x48_e0_00_00_00_00_00_00_u64);

/// Maximum net liquidity, that each individual position may not exceed.
///
/// Should be not too big to allow opening multiple positions without overflowing total liquidity.
///
/// Maximum liquidity is set to be 271 (256+15) bits.
/// This should allow to create 2^(320-271)=2^49 positions which should be enough.
/// ```
/// assert_eq!(271.0f64.exp2().to_bits(), 0x50_e0_00_00_00_00_00_00_u64);
/// ```
#[cfg(feature = "concordium")]
pub const MAX_NET_LIQUIDITY: Float = Float::from_bits(0x50_e0_00_00_00_00_00_00_u64);

pub fn validate_protocol_fee_fraction(
    protocol_fee_fraction: BasisPoints,
) -> Result<BasisPoints, ErrorKind> {
    if (MIN_PROTOCOL_FEE_FRACTION..=MAX_PROTOCOL_FEE_FRACTION).contains(&protocol_fee_fraction) {
        Ok(protocol_fee_fraction)
    } else {
        Err(ErrorKind::IllegalFee)
    }
}
