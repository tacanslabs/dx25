use multiversx_sc::abi::{TypeDescriptionContainer, TypeName};
use multiversx_sc::api::ManagedTypeApi;
use multiversx_sc::types::EgldOrEsdtTokenIdentifier;
use multiversx_sc::types::{
    heap::Address,
    {ManagedAddress, TokenIdentifier},
};
use multiversx_sc::{abi::TypeAbi, types::BigUint};
use multiversx_sc_codec::{self as codec, NestedDecode, NestedEncode};

use crate::dex::latest::RawFeeLevelsArray;
use crate::dex::v0::NUM_FEE_LEVELS;
use crate::dex::{self, BasisPoints, Float, PairExt, Tick};

use crate::chain::{dex_types::token_id::TokenId as VmTokenId, AccountId, Amount, TokenId, VmApi};
use crate::fp::{U128, U192X64, U256};
use crate::WasmAmount;

use multiversx_sc::derive::TypeAbi;
use multiversx_sc_codec::derive::{NestedDecode, NestedEncode, TopDecode, TopEncode};

#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, TypeAbi, Default, Clone)]
pub struct Fraction {
    nominator: WasmAmount,
    denominator: WasmAmount,
}

impl TryFrom<Float> for Fraction {
    type Error = dex::Error;

    fn try_from(value: Float) -> Result<Self, Self::Error> {
        let fraction = dex::Fraction::try_from(value)?;

        Ok(Self {
            nominator: fraction.nominator.into(),
            denominator: fraction.denominator.into(),
        })
    }
}

impl From<Fraction> for Float {
    fn from(value: Fraction) -> Self {
        let fraction = dex::Fraction {
            nominator: value.nominator.into(),
            denominator: value.denominator.into(),
        };

        fraction.into()
    }
}

/// Contract metadata
#[derive(TopEncode, TopDecode, TypeAbi)]
pub struct Metadata {
    /// Account that is allowed to change DEX configuration and withdraw protocol fee.
    /// Normally it is the account with the governance smart contract.
    pub owner: AccountId,

    /// Number of existing pools.
    pub pool_count: u64,

    /// Fraction of fee which goes to DEX.
    pub protocol_fee_fraction: BasisPoints,

    /// Fee rate scaled up by fee_divisor.
    pub fee_rates: dex::latest::RawFeeLevelsArray<BasisPoints>,

    /// Scale factor for the fee rates and protocol fee fraction.
    pub fee_divisor: BasisPoints,
}

/// Pool info
#[derive(NestedDecode, NestedEncode, TypeAbi)]

pub struct PoolInfo {
    /// Total amounts of tokens in the pool: sum of all positions and collected fees (LP and protocol).
    pub total_reserves: (WasmAmount, WasmAmount),

    /// Total amount of tokens locked in the pool (in positions)
    pub position_reserves: (WasmAmount, WasmAmount),

    /// Square root of the spot price on each of the fee levels.
    /// Represented as a rational fraction. First element of the tuple is the nominator,
    /// and the second is denominator.
    /// Zero values mean the pool is empty, so the price is undefined.
    pub sqrt_spot_prices: RawFeeLevelsArray<Fraction>,

    /// Square root of the effective price on each of the fee levels, for the
    /// forward ("left") and reverse ("right") swap direcions, respectively.
    /// The values are represented as rational fractions. First element of each tuple
    /// is the nominator, and the second is denominator.
    /// Zero values mean the pool is empty, so the price is undefined.
    #[allow(clippy::type_complexity)]
    pub sqrt_effective_prices: dex::latest::RawFeeLevelsArray<(Fraction, Fraction)>,

    /// Liquidity on each of the fee levels.
    /// The value is approximate, as interlally a different representation is used.
    pub liquidities: dex::latest::RawFeeLevelsArray<WasmAmount>,

    /// Fee rate scaled up by fee_divisor.
    pub fee_rates: dex::latest::RawFeeLevelsArray<BasisPoints>, // TODO: consider removing, as it is global to DEX

    /// Scale factor for the fee levels.
    pub fee_divisor: BasisPoints,
}

impl PoolInfo {
    pub fn spot_price(&self, fee_level: usize) -> Option<Float> {
        let fraction = self.sqrt_spot_prices[fee_level].clone();
        if fraction.denominator > 0 {
            Some(
                Float::from(Amount::from(fraction.nominator))
                    / Float::from(Amount::from(fraction.denominator)),
            )
        } else {
            None
        }
    }
}

impl TryFrom<dex::PoolInfo> for PoolInfo {
    type Error = dex::Error;

    fn try_from(info: dex::PoolInfo) -> Result<Self, Self::Error> {
        let mut sqrt_spot_prices = RawFeeLevelsArray::<Fraction>::default();

        for level in 0..NUM_FEE_LEVELS {
            sqrt_spot_prices[level as usize] = info.spot_sqrtprices[level as usize].try_into()?;
        }

        let mut sqrt_effective_prices = RawFeeLevelsArray::<(Fraction, Fraction)>::default();
        for level in 0..NUM_FEE_LEVELS {
            sqrt_effective_prices[level as usize].0 =
                info.eff_sqrtprices[level as usize].0.try_into()?;
            sqrt_effective_prices[level as usize].1 =
                info.eff_sqrtprices[level as usize].1.try_into()?;
        }

        Ok(Self {
            total_reserves: (info.total_reserves.0.into(), info.total_reserves.1.into()),
            position_reserves: (
                info.position_reserves.0.into(),
                info.position_reserves.1.into(),
            ),
            sqrt_spot_prices,
            sqrt_effective_prices,
            liquidities: info.liquidities.map(Into::into),
            fee_rates: info.fee_rates,
            fee_divisor: info.fee_divisor,
        })
    }
}

// Position info
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, TypeAbi)]
pub struct PositionInfo {
    pub tokens_ids: (TokenId, TokenId),
    pub balance: (WasmAmount, WasmAmount),
    pub range_ticks: (Tick, Tick),
    pub reward_since_last_withdraw: (WasmAmount, WasmAmount),
    pub reward_since_creation: (WasmAmount, WasmAmount),
    pub init_sqrt_price: Fraction,
    pub net_liquidity: Fraction,
}

impl TryFrom<dex::PositionInfo> for PositionInfo {
    type Error = dex::Error;

    fn try_from(position_info: dex::PositionInfo) -> Result<Self, Self::Error> {
        Ok(PositionInfo {
            tokens_ids: position_info.tokens_ids,
            balance: position_info.balance.map_into(),
            range_ticks: position_info.range_ticks,
            reward_since_last_withdraw: position_info.reward_since_last_withdraw.map_into(),
            reward_since_creation: position_info.reward_since_creation.map_into(),
            init_sqrt_price: position_info.init_sqrtprice.try_into()?,
            net_liquidity: position_info.net_liquidity.try_into()?,
        })
    }
}

/// Type to provide API for a collection
/// For some reason serialization provided for &[T], but not for Vec<T> for `MultiverseX` API
/// So, we implement it manually
#[derive(TopEncode, TopDecode, NestedDecode, NestedEncode, Debug)]
pub struct ApiVec<T: TypeAbi + NestedDecode + NestedEncode>(pub Vec<T>);

impl<T> TypeAbi for ApiVec<T>
where
    T: TypeAbi + NestedDecode + NestedEncode,
{
    // Default implementation uses single type instead of generics
    fn type_name() -> TypeName {
        format!("List<{}>", T::type_name())
    }

    fn provide_type_descriptions<TDC: TypeDescriptionContainer>(accumulator: &mut TDC) {
        T::provide_type_descriptions(accumulator);
    }
}

impl<T> From<Vec<T>> for ApiVec<T>
where
    T: TypeAbi + NestedDecode + NestedEncode,
{
    fn from(value: Vec<T>) -> Self {
        Self(value)
    }
}

impl<T> Default for ApiVec<T>
where
    T: TypeAbi + NestedDecode + NestedEncode,
{
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<T> FromIterator<T> for ApiVec<T>
where
    T: TypeAbi + NestedDecode + NestedEncode,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self(Vec::from_iter(iter))
    }
}

pub type ApiMap<K, V> = ApiVec<(K, V)>;

#[cfg(not(target_arch = "wasm32"))]
impl<K, V, S: std::hash::BuildHasher + Default> From<ApiVec<(K, V)>>
    for std::collections::HashMap<K, V, S>
where
    K: TypeAbi + NestedDecode + NestedEncode + Eq + std::hash::Hash,
    V: TypeAbi + NestedDecode + NestedEncode,
{
    fn from(val: ApiVec<(K, V)>) -> Self {
        val.0.into_iter().collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<T, S: std::hash::BuildHasher + Default> From<ApiVec<T>> for std::collections::HashSet<T, S>
where
    T: TypeAbi + NestedDecode + NestedEncode + Eq + std::hash::Hash,
{
    fn from(val: ApiVec<T>) -> Self {
        val.0.into_iter().collect()
    }
}

// MultiverseX BigUint conversion
impl<M: ManagedTypeApi> From<Amount> for BigUint<M> {
    fn from(value: Amount) -> Self {
        let bytes: [u8; 16] = value.into();
        Self::from_bytes_be(&bytes)
    }
}

impl<M: ManagedTypeApi> From<BigUint<M>> for Amount {
    fn from(value: BigUint<M>) -> Self {
        Self::from(value.to_bytes_be().as_slice())
    }
}

impl<M: ManagedTypeApi> From<U192X64> for BigUint<M> {
    fn from(value: U192X64) -> Self {
        let bytes: [u8; 32] = value.0.into();
        Self::from_bytes_be(&bytes)
    }
}

// We need this functions, beause we can't have generic ID's, and MultiverseX contract interface
// parameterizes contract with VM API. So, sometimes we want to convers for an accoiated API to a concrete
// API. Which in fact always is API for a target platform
pub fn into_account_id<M: ManagedTypeApi>(account: &ManagedAddress<M>) -> AccountId {
    ManagedAddress::from_address(&account.to_address())
}

// Convert parameterized ID into target VM ID
pub fn into_token_id<M: ManagedTypeApi>(token: &TokenIdentifier<M>) -> VmTokenId<VmApi> {
    VmTokenId::from_bytes(token.to_boxed_bytes().as_slice())
}

// Manual implementation of TypeAbi because MultiverseX derive macro doesn't parse doc strings properly
impl TypeAbi for U128 {}
impl TypeAbi for U256 {}

/// Carries out single withdrawal item, which is produced by `send_tokens` and consumed by withdrawal callback
#[must_use]
#[derive(TypeAbi, NestedDecode, NestedEncode, TopDecode, TopEncode)]
pub struct Withdrawal {
    pub account_id: Address,
    pub token_id: TokenId,
    pub amount: Amount,
    pub callback: Option<MethodCall>,
}

#[derive(TypeAbi, NestedDecode, NestedEncode, TopDecode, TopEncode, Debug)]
pub struct MethodCall {
    pub entrypoint: String,
    pub arguments: ApiVec<Vec<u8>>,
}
/// Defines batch action for DX25 blockchain.
/// Difference from `dex::Action` -  token identifier type in `Withdraw` action
#[cfg_attr(
    feature = "multiversx",
    derive(TopDecode, TopEncode, NestedEncode, NestedDecode, TypeAbi)
)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
pub enum Action {
    /// Request account registration; can occur at most once, as frst action in batch
    RegisterAccount,
    /// Register specified tokens for account
    RegisterTokens(Vec<TokenId>),
    /// Perform swap-in exchange of tokens
    SwapExactIn(dex::SwapAction),
    /// Perform swap-out exchange of tokens
    SwapExactOut(dex::SwapAction),
    /// Perform swap-out exchange of tokens
    SwapToPrice(dex::SwapToPriceAction),
    /// Deposit token to account; account, token and amount are passed as part of call context;
    /// should appear exactly once in batch
    Deposit,
    /// Withdraw specified token from account
    Withdraw(
        EgldOrEsdtTokenIdentifier<VmApi>,
        WasmAmount,
        Option<MethodCall>,
    ),
    /// Opens position with specified tokens and their specified amounts
    OpenPosition {
        tokens: (TokenId, TokenId),
        fee_rate: BasisPoints,
        position: dex::PositionInit,
    },
    /// Closes specified position
    ClosePosition(dex::PositionId),
    /// Withdraw fees collected on specific position. User must own it
    WithdrawFee(dex::PositionId),
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
#[derive(TopDecode, TopEncode, TypeAbi)]
pub struct EstimateSwapExactResult {
    pub result: WasmAmount,
    pub result_bound: WasmAmount,
    pub price_impact: Fraction,
    pub swap_price: Option<Fraction>,
    pub swap_price_worst: Option<Fraction>,
    pub fee_in_spent_tok: WasmAmount,
    pub num_tick_crossings: u32,
}

impl TryFrom<dex::EstimateSwapExactResult> for EstimateSwapExactResult {
    type Error = dex::Error;

    fn try_from(res: dex::EstimateSwapExactResult) -> Result<Self, Self::Error> {
        Ok(EstimateSwapExactResult {
            result: res.result.into(),
            result_bound: res.result_bound.into(),
            swap_price: res.swap_price.map(TryInto::try_into).transpose()?,
            swap_price_worst: res.swap_price_worst.map(TryInto::try_into).transpose()?,
            fee_in_spent_tok: res.fee_in_spent_tok.into(),
            price_impact: res.price_impact.try_into()?,
            num_tick_crossings: res.num_tick_crossings,
        })
    }
}

#[derive(NestedDecode, NestedEncode, TypeAbi)]
pub struct TxCostEstimate {
    pub gas_cost_max: WasmAmount,
    pub storage_fee_max: WasmAmount,
}

impl From<dex::TxCostEstimate> for TxCostEstimate {
    fn from(res: dex::TxCostEstimate) -> Self {
        Self {
            gas_cost_max: res.gas_cost_max.into(),
            storage_fee_max: res.storage_fee_max.into(),
        }
    }
}

#[derive(TopDecode, TopEncode, TypeAbi)]
pub struct EstimateAddLiquidityResult {
    pub min_a: WasmAmount,
    pub max_a: WasmAmount,
    pub min_b: WasmAmount,
    pub max_b: WasmAmount,
    pub pool_exists: bool,
    pub spot_price: Option<Fraction>,
    pub position_price: Fraction,
    pub position_net_liquidity: Fraction,
    pub tx_cost: TxCostEstimate,
}

impl TryFrom<dex::EstimateAddLiquidityResult> for EstimateAddLiquidityResult {
    type Error = dex::Error;

    fn try_from(res: dex::EstimateAddLiquidityResult) -> Result<Self, Self::Error> {
        Ok(EstimateAddLiquidityResult {
            min_a: res.min_a.into(),
            max_a: res.max_a.into(),
            min_b: res.min_b.into(),
            max_b: res.max_b.into(),
            pool_exists: res.pool_exists,
            spot_price: res.spot_price.map(TryInto::try_into).transpose()?,
            position_price: res.position_price.try_into()?,
            position_net_liquidity: res.position_net_liquidity.try_into()?,
            tx_cost: res.tx_cost.into(),
        })
    }
}
