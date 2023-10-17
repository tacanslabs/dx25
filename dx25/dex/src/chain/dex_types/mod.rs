mod collections;
mod float;
mod ordered_map;
pub mod token_id;

use std::marker::PhantomData;

use multiversx_sc::{
    api::StorageMapperApi,
    types::{BigUint, ManagedAddress},
};
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode},
};

use crate::dex::latest::NUM_FEE_LEVELS;
use crate::dex::{self, PoolId, PositionId};

/// Maximum value for price tick
pub const MAX_TICK: i32 = 887_273;
/// Minimum value for price tick
pub const MIN_TICK: i32 = -887_273;

pub const MIN_EFF_TICK: i32 = MIN_TICK - 2i32.pow(NUM_FEE_LEVELS as u32 - 1);
pub const MAX_EFF_TICK: i32 = MAX_TICK + 2i32.pow(NUM_FEE_LEVELS as u32 - 1);

/// Number of precalculated ticks
pub const NUM_PRECALCULATED_TICKS: usize = 20;

// VM API to use with MultiverseX managed types
// Always a golang runtume for WASM and debug for tests
#[cfg(not(target_arch = "wasm32"))]
pub use multiversx_sc_scenario::DebugApi as VmApi;
#[cfg(target_arch = "wasm32")]
pub use multiversx_sc_wasm_adapter::api::VmApiImpl as VmApi;

pub use crate::fp::I192X64 as FixedPointSigned;
pub use crate::fp::U128 as UInt;
pub use crate::fp::U128X128 as FixedPoint;
pub use crate::fp::U256 as UIntBig;
pub use crate::fp::U320X192 as FixedPointBig;
pub use float::Float;
pub use UInt as Amount;
pub type AmountUFP = crate::fp::U256X256;
pub type AmountSFP = crate::fp::I256X256;
pub type Liquidity = crate::fp::U192X64;
pub type LiquiditySFP = crate::fp::I192X64;
pub type LongestUFP = crate::fp::U256X256;
pub type LongestSFP = crate::fp::I256X256;
pub type NetLiquidityUFP = crate::fp::U192X64;
pub type NetLiquiditySFP = crate::fp::I192X64;
pub type GrossLiquidityUFP = crate::fp::U192X192;
pub type FeeLiquidityUFP = crate::fp::U192X192;
pub type LPFeePerFeeLiquidity = crate::fp::I128X128;
pub type SqrtpriceUFP = crate::fp::U128X128;
pub type SqrtpriceSFP = crate::fp::I128X128;
pub type AccSqrtpriceSFP = crate::fp::I128X128;
pub type LiquidityMulSqrtpriceUFP = crate::fp::U256X256;

// MultiversX IDs
pub type AccountId = ManagedAddress<VmApi>;
pub type TokenId = token_id::TokenId<VmApi>;
pub type WasmAmount = BigUint<VmApi>;

pub use collections::*;
pub use ordered_map::*;

// Types set
#[derive(NestedDecode, NestedEncode)]
pub struct Types<S: StorageMapperApi>(PhantomData<S>);

#[cfg(test)]
pub type TestTypes = Types<multiversx_sc_scenario::DebugApi>;

impl dex::AccountExtra for () {}

#[derive(Default, NestedEncode, NestedDecode)]
pub struct ContractExtra {
    pub wegld: Option<(AccountId, TokenId)>,
}

impl<S: StorageMapperApi> dex::Types for Types<S> {
    type Bound = ();
    type ContractExtraV1 = ContractExtra;
    type AccountsMap = StorageMap<S, AccountId, dex::Account<Self>>;
    type TickStatesMap = StorageOrderedMap<S, dex::Tick, dex::TickState<Types<S>>>;
    type AccountTokenBalancesMap = StorageMap<S, TokenId, Amount>;
    type AccountWithdrawTracker = dex::withdraw_trackers::FullTracker;
    type AccountExtra = ();
    type PoolsMap = StorageMap<S, PoolId, dex::Pool<Self>>;
    type PoolPositionsMap = StorageMap<S, PositionId, dex::Position<Self>>;
    type AccountPositionsSet = StorageSet<S, PositionId>;
    type VerifiedTokensSet = StorageSet<S, TokenId>;
    type PositionToPoolMap = StorageMap<S, PositionId, PoolId>;
    type AccountIdSet = StorageSet<S, AccountId>;
    #[cfg(feature = "smart-routing")]
    type TokenConnectionsMap = StorageMap<S, TokenId, Self::TokensSet>;
    #[cfg(feature = "smart-routing")]
    type TokensSet = StorageSet<S, TokenId>;
    #[cfg(feature = "smart-routing")]
    type TokensArraySet = StorageSet<S, TokenId>;
    #[cfg(feature = "smart-routing")]
    type TopPoolsMap = StorageMap<S, TokenId, Self::TokensArraySet>;
}
