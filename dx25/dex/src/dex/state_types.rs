use super::map_with_context::{MapContext, MapWithContext};
use super::{v0, BasisPoints, ErrorKind, FeeLevel, Float, Side, Types};
use crate::chain::{AccountId, Amount, AmountUFP, LPFeePerFeeLiquidity, Liquidity, LiquiditySFP};
use crate::dex::tick::{EffTick, Tick};
use paste::paste;
use std::marker::PhantomData;

pub type VersionNumber = u16;

#[cfg(feature = "near")]
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};

/// Helper which serializes any serializable value with version number as prefix
#[cfg(feature = "near")]
fn serialize_ver<U: BorshSerialize, W: std::io::Write>(
    writer: &mut W,
    version: VersionNumber,
    value: &U,
) -> std::io::Result<()> {
    version.serialize(writer)?;
    value.serialize(writer)
}

#[cfg(feature = "concordium")]
use concordium_std::{
    Deletable, Deserial, DeserialWithState, HasStateApi, ParseResult, Read, Serial,
};

/// Helper which serializes any serializable value with version number as prefix
#[cfg(feature = "concordium")]
fn serial_ver<U: Serial, W: concordium_std::Write>(
    writer: &mut W,
    version: VersionNumber,
    value: &U,
) -> Result<(), W::Err> {
    version.serial(writer)?;
    value.serial(writer)
}

#[cfg(feature = "multiversx")]
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode, TopDecode, TopEncode},
    NestedDecode, NestedEncode,
};

/// Generates versioned wrapper for data structure
/// Each version is serialized and deserialized by using `u16` prefix
/// with actual version number. (De)serialization code is generated
/// for NEAR and Concordium blockchains.
/// Please note that all structures get parametrized with `T: dex::Types + ?Sized`.
/// If some structure doesn't use type parameter T, it should use `PhantomData<T>`
/// as one of its fields, to appease type checker.
///
/// Example:
/// ```ignore
/// versioned! {
///     pub Foo {
///         0 => {
///             pub bar: i32,
///             pub phantom_t: PhantomData<T>
///         },
///         1 => {
///             pub baz: u64,
///             pub phantom_t: PhantomData<T>
///         }
///     }
/// }
/// ```
/// will generate
/// ```ignore
/// #[cfg_attr(feature = "concordium",
///     derive(Deletable),
///     concordium(state_parameter = "T::Bound")
/// )]
/// pub enum Foo<T: Types + ?Sized> {
///     V0(FooV0<T>),
///     V1(FooV1<T>),
/// }
///
/// #[cfg(feature = near)]
/// impl<T: Types + ?Sized> BorshSerialize for Foo<T> {
///     // manual impl
/// }
///
/// #[cfg(feature = "near")]
/// impl<T: Types + ?Sized> BorshDeserialize for Foo<T> {
///     // manual impl
/// }
///
/// #[cfg(feature = "concordium")]
/// impl<T: Types + ?Sized> Serial for Foo<T> where T::Bound: HasStateApi {
///     // manual impl
/// }
///
/// #[cfg(feature = "concordium")]
/// impl<T: Types + ?Sized> DeserialWithState<T::Bound> for Foo<T>
/// where
///     T::Bound: HasStateApi
/// {
///     // manual impl
/// }
///
/// #[cfg(feature = "multiversx")]
/// impl<T: Types + ?Sized> multiversx_sc_codec::TopEncode for Foo<T> where T: multiversx_sc_codec::TopEncode {
///     // manual impl
/// }

/// #[cfg(feature = "multiversx")]
/// impl<T: Types + ?Sized> multiversx_sc_codec::TopDecode for Foo<T> where T: multiversx_sc_codec::TopDecode {
///     // maunal impl
/// }
///
/// #[cfg_attr(feature = "near", derive(BorshSerialize, BorshDeserialize))]
/// #[cfg_attr(
///     feature = "concordium",
///     derive(Serial, DeserialWithState, Deletable),
///     concordium(state_parameter = "T::Bound")
/// )]
/// pub struct FooV0<T: Types + ?Sized> {
///     pub bar: i32,
///     pub phantom_t: PhantomData<T>
/// }
///
/// #[cfg_attr(feature = "near", derive(BorshSerialize, BorshDeserialize))]
/// #[cfg_attr(
///     feature = "concordium",
///     derive(Serial, DeserialWithState, Deletable),
///     concordium(state_parameter = "T::Bound")
/// )]
/// #[cfg_attr(feature = "multiversx", derive(TopEncode, TopDecode))]
/// pub struct FooV1<T: Types + ?Sized> {
///     pub baz: u64,
///     pub phantom_t: PhantomData<T>
/// }
/// // Always refers to last variant
/// pub type FooLatest<T> = FooV1<T>;
/// ```
macro_rules! versioned {
    ($pub:vis $enum_name:ident {
        $($ver_num:literal => { $($struct_body:tt)* }),+
    }) => {
        paste! {
            #[cfg_attr(feature = "concordium",
                derive(Deletable),
                concordium(state_parameter = "T::Bound")
            )]
            $pub enum $enum_name<T: Types> {
                $(
                    [<V $ver_num>]([<$enum_name V $ver_num>]<T>),
                )+
            }

            #[cfg(feature = "near")]
            impl<T: Types + ?Sized> BorshSerialize for $enum_name<T> {
                fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
                    match self {
                        $(
                            $enum_name::[<V $ver_num>](value) => serialize_ver(writer, $ver_num, value),
                        )+
                    }
                }
            }

            #[cfg(feature = "near")]
            impl<T: Types + ?Sized> BorshDeserialize for $enum_name<T> {
                fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
                    match VersionNumber::deserialize(buf)? {
                        $(
                            $ver_num => Ok($enum_name::[<V $ver_num>](
                                [<$enum_name V $ver_num>]::deserialize(buf)?
                            )),
                        )+
                        _ => Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Invalid version number",
                        )),
                    }
                }
            }

            #[cfg(feature = "concordium")]
            impl<T: Types + ?Sized> Serial for $enum_name<T> where T::Bound: HasStateApi {
                fn serial<W: concordium_std::Write>(&self, out: &mut W) -> Result<(), W::Err> {
                    match self {
                        $(
                            $enum_name::[<V $ver_num>](value) => serial_ver(out, $ver_num, value),
                        )+
                    }
                }
            }

            #[cfg(feature = "concordium")]
            impl<T: Types + ?Sized> DeserialWithState<T::Bound> for $enum_name<T> where T::Bound: HasStateApi {
                fn deserial_with_state<R: concordium_std::Read>(
                    state: &T::Bound,
                    source: &mut R,
                ) -> concordium_std::ParseResult<Self> {
                    match VersionNumber::deserial(source)? {
                        $(
                            $ver_num => Ok($enum_name::[<V $ver_num>](
                                [<$enum_name V $ver_num>]::deserial_with_state(state, source)?
                            )),
                        )+
                        _ => Err(concordium_std::ParseError{}),
                    }
                }
            }

            #[cfg(feature = "multiversx")]
            impl<T: Types + ?Sized> multiversx_sc_codec::TopEncode for $enum_name<T> {
                fn top_encode<O: multiversx_sc_codec::TopEncodeOutput>(
                    &self,
                    output: O,
                ) -> Result<(), multiversx_sc_codec::EncodeError> {
                    match self {
                        $(
                            $enum_name::[<V $ver_num>](value) => {
                                let mut nested_output = output.start_nested_encode();

                                ($ver_num as VersionNumber).dep_encode(&mut nested_output)?;
                                value.dep_encode(&mut nested_output)?;

                                output.finalize_nested_encode(nested_output);
                                Ok(())
                            }
                        )+
                    }
                }
            }

            #[cfg(feature = "multiversx")]
            impl<T: Types + ?Sized> multiversx_sc_codec::TopDecode for $enum_name<T> {
                fn top_decode<I: multiversx_sc_codec::TopDecodeInput>(
                    input: I,
                ) -> Result<Self, multiversx_sc_codec::DecodeError> {
                    let mut nested_input = input.into_nested_buffer();

                    match VersionNumber::dep_decode(&mut nested_input)? {
                        $(
                            $ver_num => Ok($enum_name::[<V $ver_num>](
                                [<$enum_name V $ver_num>]::dep_decode(&mut nested_input)?
                            )),
                        )+
                        _ => Err(multiversx_sc_codec::DecodeError::INPUT_OUT_OF_RANGE),
                    }
                }
            }

            #[cfg(feature = "multiversx")]
            impl<T: Types + ?Sized> multiversx_sc_codec::NestedEncode for $enum_name<T> {
                fn dep_encode<O: multiversx_sc_codec::NestedEncodeOutput>(
                    &self,
                    dest: &mut O,
                ) -> Result<(), multiversx_sc_codec::EncodeError> {
                    match self {
                        $(
                            $enum_name::[<V $ver_num>](value) => {
                                ($ver_num as VersionNumber).dep_encode(dest)?;
                                value.dep_encode(dest)
                            }
                        )+
                    }
                }
            }

            #[cfg(feature = "multiversx")]
            impl<T: Types + ?Sized> multiversx_sc_codec::NestedDecode for $enum_name<T> {
                fn dep_decode<I: multiversx_sc_codec::NestedDecodeInput>(
                    input: &mut I,
                ) -> Result<Self, multiversx_sc_codec::DecodeError> {
                    match VersionNumber::dep_decode(input)? {
                        $(
                            $ver_num => Ok($enum_name::[<V $ver_num>](
                                [<$enum_name V $ver_num>]::dep_decode(input)?
                            )),
                        )+
                        _ => Err(multiversx_sc_codec::DecodeError::INPUT_OUT_OF_RANGE),
                    }
                }
            }

            $(
                #[cfg_attr(feature = "near", derive(BorshSerialize, BorshDeserialize))]
                #[cfg_attr(
                    feature = "concordium",
                    derive(Serial, DeserialWithState, Deletable),
                    concordium(state_parameter = "T::Bound")
                )]
                #[cfg_attr(feature = "multiversx", derive(NestedEncode, NestedDecode, TopEncode, TopDecode))]
                $pub struct [<$enum_name V $ver_num>]<T: Types> {
                    $($struct_body)*
                }
            )+

            versioned!{ @latest $pub $enum_name => $($ver_num)+ }
        }
    };
    // Generates type alias for last struct definition
    // Unfortunately Rust doesn't seem to be able to match last literal in sequence,
    // so we use classic tail recursion here
    (@latest $pub:vis $enum_name:ident => $ver_num_head:literal $($ver_num_tail:literal)+) => {
        versioned! { @latest $pub $enum_name => $($ver_num_tail)+ }
    };
    (@latest $pub:vis $enum_name:ident => $ver_num:literal) => {
        paste!{
            $pub type [<$enum_name Latest>]<T> = [<$enum_name V $ver_num>]<T>;
        }
    };
}

macro_rules! map_with_ctxt {
    ($map:ident, $error:expr) => {
        paste::paste!(
            pub struct [<$map Context>];

            impl MapContext for [<$map Context>] {
                fn not_found_error() -> ErrorKind {
                    $error
                }
            }

            pub type $map<T> = MapWithContext<<T as Types>::$map, [<$map Context>]>;
        );
    }
}

map_with_ctxt!(PoolsMap, ErrorKind::PoolNotRegistered);
map_with_ctxt!(AccountsMap, ErrorKind::AccountNotRegistered);
map_with_ctxt!(PositionToPoolMap, ErrorKind::PositionDoesNotExist);
#[cfg(feature = "smart-routing")]
map_with_ctxt!(TokenConnectionsMap, ErrorKind::PoolNotRegistered);
#[cfg(feature = "smart-routing")]
map_with_ctxt!(TopPoolsMap, ErrorKind::PoolNotRegistered);

versioned! {
    pub Contract {
        0 => {
            /// Account of the owner.
            pub owner_id: AccountId,
            /// Accounts that are allowed to set permitions for payable methods.
            pub guards: T::AccountIdSet,
            /// Payable API state
            pub suspended: bool,
            /// Map of all the pools.
            pub pools: PoolsMap<T>,
            /// Accounts registered, keeping track all the amounts deposited, storage and more.
            pub accounts: AccountsMap<T>,
            /// Set of allowed tokens by "owner".
            pub verified_tokens: T::VerifiedTokensSet,
            /// number of pools
            pub pool_count: u64,
            /// Counter for position
            pub next_free_position_id: u64,
            /// Map of position to token_pair, in pool of which it exists
            pub position_to_pool_id: PositionToPoolMap<T>,
            /// Fraction of the total fee, that will go to the DEX.
            /// The rest of the fee will be distributed among the liquidity providers.
            /// Specified in units of 1/FEE_DIVISOR. For example, if FEE_DIVISOR
            /// is 10000, and one wants 13% of the total fee to go to the DEX, one must set
            /// protocol_fee_fraction = 0.13*10000 = 1300. In such case, if a swap is performed
            /// on a level with e.g. 0.2% total fee rate, and the total amount paid by the
            /// trader is e.g. 100000 tokens, then the total charged fee will be 2000 tokens,
            /// out of which 260 tokens will go to the DEX, and the rest 1740 tokens
            /// will be distributed among the LPs.
            pub protocol_fee_fraction: BasisPoints,
        },
        1 => {
            /// Account of the owner.
            pub owner_id: AccountId,
            /// Accounts that are allowed to set permitions for payable methods.
            pub guards: T::AccountIdSet,
            /// Payable API state
            pub suspended: bool,
            /// Map of all the pools.
            pub pools: PoolsMap<T>,
            /// Accounts registered, keeping track all the amounts deposited, storage and more.
            pub accounts: AccountsMap<T>,
            /// Set of allowed tokens by "owner".
            pub verified_tokens: T::VerifiedTokensSet,
            /// number of pools
            pub pool_count: u64,
            /// Counter for position
            pub next_free_position_id: u64,
            /// Map of position to token_pair, in pool of which it exists
            pub position_to_pool_id: PositionToPoolMap<T>,
            /// Fraction of the total fee, that will go to the DEX.
            /// The rest of the fee will be distributed among the liquidity providers.
            /// Specified in units of 1/FEE_DIVISOR. For example, if FEE_DIVISOR
            /// is 10000, and one wants 13% of the total fee to go to the DEX, one must set
            /// protocol_fee_fraction = 0.13*10000 = 1300. In such case, if a swap is performed
            /// on a level with e.g. 0.2% total fee rate, and the total amount paid by the
            /// trader is e.g. 100000 tokens, then the total charged fee will be 2000 tokens,
            /// out of which 260 tokens will go to the DEX, and the rest 1740 tokens
            /// will be distributed among the LPs.
            pub protocol_fee_fraction: BasisPoints,

            pub extra: T::ContractExtraV1,
        }
    }
}

pub struct ContractRef<'a, T: Types> {
    pub owner_id: &'a AccountId,
    pub guards: &'a T::AccountIdSet,
    pub suspended: bool,
    pub pools: &'a PoolsMap<T>,
    pub accounts: &'a AccountsMap<T>,
    pub verified_tokens: &'a T::VerifiedTokensSet,
    pub pool_count: u64,
    pub next_free_position_id: u64,
    pub position_to_pool_id: &'a PositionToPoolMap<T>,
    pub protocol_fee_fraction: BasisPoints,
}

impl<T: Types> Contract<T> {
    /// Automatically upgrade Contract to latest version and return reference
    pub fn latest(&mut self) -> &mut ContractLatest<T> {
        match self {
            Contract::V0(ref mut contract) => unsafe {
                // It's a well-known method of swapping enum variant in-place
                // without `Option` or other overheads.
                // Any operations which may panic should be performed *before*
                // swapping data entries. Moving data around should be safe
                // since it's just `memcpy`

                let ContractV0 {
                    owner_id,
                    guards,
                    suspended,
                    pools,
                    accounts,
                    verified_tokens,
                    pool_count,
                    next_free_position_id,
                    position_to_pool_id,
                    protocol_fee_fraction,
                } = std::ptr::read(contract as *const _);

                std::ptr::write(
                    self as *mut _,
                    Contract::V1(ContractLatest {
                        owner_id,
                        guards,
                        suspended,
                        pools,
                        accounts,
                        verified_tokens,
                        pool_count,
                        next_free_position_id,
                        position_to_pool_id,
                        protocol_fee_fraction,
                        extra: T::ContractExtraV1::default(),
                    }),
                );

                self.latest()
            },
            Contract::V1(ref mut contract) => contract,
        }
    }
    /// Retrieves immutable view of contract root state, regardless of its version
    pub fn as_ref(&self) -> ContractRef<'_, T> {
        match self {
            Contract::V0(ref contract) => ContractRef {
                owner_id: &contract.owner_id,
                guards: &contract.guards,
                suspended: contract.suspended,
                pools: &contract.pools,
                accounts: &contract.accounts,
                verified_tokens: &contract.verified_tokens,
                pool_count: contract.pool_count,
                next_free_position_id: contract.next_free_position_id,
                position_to_pool_id: &contract.position_to_pool_id,
                protocol_fee_fraction: contract.protocol_fee_fraction,
            },
            Contract::V1(ref contract) => ContractRef {
                owner_id: &contract.owner_id,
                guards: &contract.guards,
                suspended: contract.suspended,
                pools: &contract.pools,
                accounts: &contract.accounts,
                verified_tokens: &contract.verified_tokens,
                pool_count: contract.pool_count,
                next_free_position_id: contract.next_free_position_id,
                position_to_pool_id: &contract.position_to_pool_id,
                protocol_fee_fraction: contract.protocol_fee_fraction,
            },
        }
    }
}

map_with_ctxt!(AccountTokenBalancesMap, ErrorKind::TokenNotRegistered);

versioned! {
    pub Account {
        0 => {
            /// Amounts of various tokens deposited to this account
            pub token_balances: AccountTokenBalancesMap<T>,
            /// Positions which belong to current account
            pub positions: T::AccountPositionsSet,
            /// Tracks withdrawals which may be multistage or even asynchronous
            pub withdraw_tracker: T::AccountWithdrawTracker,
            /// Blockchain-specific extra information, may be `()`
            pub extra: T::AccountExtra,
        }
    }
}

map_with_ctxt!(PoolPositionsMap, ErrorKind::PositionDoesNotExist);
map_with_ctxt!(TickStatesMap, ErrorKind::InternalTickNotFound);

versioned! {
    pub Pool {
        0 => {
            /// Liquidity positions of this pool
            pub positions: PoolPositionsMap<T>,
            /// Tick states per fee level
            pub tick_states: v0::FeeLevelsArray<TickStatesMap<T>>,
            /// Total amounts of tokens, including the positions and collected fees (LP and protocol)
            pub total_reserves: (Amount, Amount),
            /// Amounts of tokens locked in positions.
            pub position_reserves: v0::FeeLevelsArray<(AmountUFP, AmountUFP)>,
            /// Total amount of LP fee reward to be paid out to all LPs (in case all pasitions are closed)
            pub acc_lp_fee: (AmountUFP, AmountUFP),
            /// Global sqrtprice shift accumulators per top-active-level and for each swap direction.
            /// These are sums of price shifts, performed in swaps with top active level equal to
            /// the index of the array. Hence, to get the total price shift on level `k`
            /// one has to sum up the values from index k to NUM_FEE_LEVELS.
            pub acc_lp_fees_per_fee_liquidity: v0::FeeLevelsArray<(LPFeePerFeeLiquidity, LPFeePerFeeLiquidity)>,
            /// Effective price on each of the levels
            pub eff_sqrtprices: v0::FeeLevelsArray<v0::EffSqrtprices>,
            /// next active ticks for swaps in left direction
            pub next_active_ticks_left: v0::FeeLevelsArray<Option<Tick>>,
            /// next active ticks for swaps in right direction
            pub next_active_ticks_right: v0::FeeLevelsArray<Option<Tick>>,
            /// Current effective net liquidity. Equal to: liquidity * sqrt(1-fee_rate)
            pub net_liquidities: v0::FeeLevelsArray<Liquidity>,
            /// Current top active level
            pub top_active_level: FeeLevel,
            pub active_side: Side,
            /// A tick which spot price is sufficiently close (less than 1 tick away) to the
            /// current effective sqrtprice in the active direction. It is used to evaluate the
            /// effective sqrtprice in the opposite direction.
            /// See `eff_sqrtprice_opposite_side` for details.
            pub pivot: EffTick,
        }
    }
}

versioned! {
    pub Position {
        0 => {
            /// Fee level index where the position is open
            pub fee_level: FeeLevel,
            /// Liquidity of the position
            pub net_liquidity: Liquidity,
            /// Accumulated effective sqrt price shifts in the pool by the moment when the position was created.
            pub init_acc_lp_fees_per_fee_liquidity: (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity),
            /// Accumulated effective sqrt price shifts in the pool by the last time when the fees were withdrawn.
            pub unwithdrawn_acc_lp_fees_per_fee_liquidity: (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity),
            /// Square root of price at the momemnt of position creation
            pub init_sqrtprice: Float,
            /// Concentrated liquidity bounds
            pub tick_bounds: (Tick, Tick),
            /// Phantom data, to bind T and unify all state types declarations
            pub phantom_t: PhantomData<T>,
        }
    }
}

versioned! {
    pub TickState {
        0 => {
            pub net_liquidity_change: LiquiditySFP,
            pub reference_counter: u32,
            pub acc_lp_fees_per_fee_liquidity_outside: (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity),
            pub phantom_t: PhantomData<T>,
        }
    }
}

impl<T: Types> Clone for Position<T> {
    fn clone(&self) -> Self {
        match self {
            Position::V0(position) => Position::V0(PositionV0 {
                fee_level: position.fee_level,
                net_liquidity: position.net_liquidity,
                init_acc_lp_fees_per_fee_liquidity: position.init_acc_lp_fees_per_fee_liquidity,
                unwithdrawn_acc_lp_fees_per_fee_liquidity: position
                    .unwithdrawn_acc_lp_fees_per_fee_liquidity,
                init_sqrtprice: position.init_sqrtprice,
                tick_bounds: position.tick_bounds,
                phantom_t: PhantomData,
            }),
        }
    }
}

impl<T: Types> Clone for TickState<T> {
    fn clone(&self) -> Self {
        match self {
            TickState::V0(tick_state) => TickState::V0(TickStateV0 {
                net_liquidity_change: tick_state.net_liquidity_change,
                reference_counter: tick_state.reference_counter,
                acc_lp_fees_per_fee_liquidity_outside: tick_state
                    .acc_lp_fees_per_fee_liquidity_outside,
                phantom_t: PhantomData,
            }),
        }
    }
}
