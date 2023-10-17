mod overlay_factory;
mod overlay_map;
mod pool_overlay;

use std::{borrow::Borrow, cell::RefCell, cmp::Ordering, collections::HashMap, ops::Deref};

use itertools::Itertools;

use pool_overlay::PoolStateOverlay;

use crate::{
    dex::{
        errors::Result,
        latest::{EffSqrtprices, RawFeeLevelsArray},
        pool::{
            self, eff_sqrtprice_from_spot_sqrtprice, eval_initial_eff_sqrtprice, fee_rate_ticks,
            fee_rates_ticks, find_pivot, Pool as _,
        },
        traits::{ItemFactory as _, Map as _},
        utils::{next_down, next_up, swap_if, MinSome},
        v0::{position_state_ex::eval_position_balance_ufp, FeeLevelsArray, NUM_FEE_LEVELS},
        BasisPoints, EffTick, ErrorKind, EstimateAddLiquidityResult, EstimateRemoveLiquidityResult,
        EstimateSwapExactResult, FeeLevel, ItemFactory as _, Pool, PoolId, PositionId,
        PositionInit, PositionOpenedInfo, Range, Side, State, Tick, TxCostEstimate, Types,
        BASIS_POINT_DIVISOR, MAX_NET_LIQUIDITY, MIN_NET_LIQUIDITY,
    },
    ensure, ensure_here, error_here, AccountId, Amount, AmountSFP, AmountUFP, Float, Liquidity,
    LiquiditySFP, NetLiquidityUFP, TokenId,
};

use self::overlay_factory::OverlayItemFactory;

use super::Dex;

// constant was obtained from the calculate_gas_constants test
// to calculate open_position gas costs:
// gas_cost = OPEN_POSITION_COST_PER_TICK_LOG * ticks_len.log2() as u128
//            + OPEN_POSITION_COST_BASE;
#[cfg(feature = "near")]
const OPEN_POSITION_COST_PER_TICK_LOG: u128 = 1_504_431_931_951;
#[cfg(feature = "near")]
const OPEN_POSITION_COST_BASE: u128 = 11_893_661_811_952;
#[cfg(feature = "concordium")]
const OPEN_POSITION_COST_PER_TICK_LOG: u128 = 1_004;
#[cfg(feature = "concordium")]
const OPEN_POSITION_COST_BASE: u128 = 21_184;
#[cfg(feature = "multiversx")]
const OPEN_POSITION_COST_PER_TICK_LOG: u128 = 856_316;
#[cfg(feature = "multiversx")]
const OPEN_POSITION_COST_BASE: u128 = 62_594_412;

// constant was obtained from the calculate_gas_constants test
// to calculate close_position gas costs:
// gas_cost = CLOSE_POSITION_COST_PER_TICK_LOG * ticks_len.log2() as u128
//            + CLOSE_POSITION_COST_BASE;
#[cfg(feature = "near")]
const CLOSE_POSITION_COST_PER_TICK_LOG: u128 = 1_578_264_217_702;
#[cfg(feature = "near")]
const CLOSE_POSITION_COST_BASE: u128 = 15_467_214_199_464;

pub trait Estimations {
    fn estimate_swap_exact(
        &self,
        is_exact_in: bool,
        token_in: TokenId,
        token_out: TokenId,
        amount: Amount,
        slippage_tolerance_bp: BasisPoints,
    ) -> Result<EstimateSwapExactResult>;

    #[allow(clippy::too_many_arguments)]
    fn estimate_liq_add(
        &self,
        tokens: (TokenId, TokenId),
        fee_rate: BasisPoints,
        ticks_range: (Option<i32>, Option<i32>),
        amount_a: Option<Amount>,
        amount_b: Option<Amount>,
        user_price: Option<Float>,
        slippage_tolerance_bp: BasisPoints,
    ) -> Result<EstimateAddLiquidityResult>;

    fn estimate_liq_remove(&self, position_id: u64) -> Result<EstimateRemoveLiquidityResult>;
}

impl<T: Types, S: State<T>, SS: Borrow<S>> Estimations for Dex<T, S, SS> {
    fn estimate_swap_exact(
        &self,
        is_exact_in: bool,
        token_in: TokenId,
        token_out: TokenId,
        amount: Amount,
        slippage_tolerance_bp: BasisPoints,
    ) -> Result<EstimateSwapExactResult> {
        let (pool_id, swapped) =
            PoolId::try_from_pair((token_in, token_out)).map_err(|e| error_here!(e))?;
        let direction = if swapped { Side::Right } else { Side::Left };

        let contract = self.contract().as_ref();

        contract.pools.try_inspect(&pool_id, |Pool::V0(ref pool)| {
            let init_eff_sqrtprice = pool.eff_sqrtprice(0, direction);

            let mut pool = PoolStateOverlay::<T>::from(pool);

            let position_reserves_before: AmountUFP = pool
                .position_reserves()
                .into_iter()
                .map(|position_reserves_at_level| position_reserves_at_level[direction])
                .sum();

            let (amount_in, amount_out, num_tick_crossings) = if is_exact_in {
                pool.swap_exact_in(direction, amount, contract.protocol_fee_fraction)?
            } else {
                pool.swap_exact_out(direction, amount, contract.protocol_fee_fraction)?
            };

            let position_reserves_after: AmountUFP = pool
                .position_reserves()
                .into_iter()
                .map(|position_reserves_at_level| position_reserves_at_level[direction])
                .sum();
            let fee_in_spent_tok = Amount::try_from(
                AmountUFP::from(amount_in) - (position_reserves_after - position_reserves_before),
            )
            .map_err(|_| error_here!(ErrorKind::InternalLogicError))?;

            let amount_in_float = Float::from(amount_in);
            let amount_out_float = Float::from(amount_out);

            let result = if is_exact_in { amount_out } else { amount_in };
            let slippage_tolerance =
                Float::from(slippage_tolerance_bp) / Float::from(BASIS_POINT_DIVISOR);
            let slippage_factor = Float::one() - slippage_tolerance;
            let result_bound_float = if is_exact_in {
                amount_out_float * slippage_factor
            } else {
                amount_in_float / slippage_factor
            };
            let result_bound = Amount::try_from(result_bound_float).map_err(|e| error_here!(e))?;

            let swap_price = if amount_out_float.is_zero() {
                None
            } else {
                Some(amount_in_float / amount_out_float)
            };

            let swap_price_worst = if is_exact_in {
                if result_bound_float.is_normal() {
                    Some(amount_in_float / result_bound_float)
                } else {
                    None
                }
            } else {
                // Exact-out swap => amount_out must be >= 1.
                ensure_here!(amount_out_float.is_normal(), ErrorKind::InternalLogicError);

                Some(result_bound_float / amount_out_float)
            };

            let price_impact = swap_price.map_or(Float::zero(), |swap_price| {
                (swap_price - init_eff_sqrtprice * init_eff_sqrtprice) / swap_price
            });

            Ok(EstimateSwapExactResult {
                result,
                result_bound,
                price_impact,
                swap_price,
                swap_price_worst,
                fee_in_spent_tok,
                num_tick_crossings,
            })
        })?
    }

    /// Estimate outcome of opening a position.
    ///
    /// # Argumetns
    ///  * `(token_a, token_b)` - tokens identifying the pool
    ///  * `fee_rate` - fee rate in ticks, identifying the fee level.
    ///  * `ticks_range` - position price range, in ticks
    ///  * `amount_a` - amount of token A to be deposited
    ///  * `amount_b` - amount of token B to be deposited
    ///  * `user_price` - spot price to set, if pool did not exist
    ///  * `slippage_tolerance` - defines tolerable spot price deviation
    ///
    /// If one (or both) of `ticks_range` bounds is not specified,
    /// `MIN_TICK` or `MAX_TICK` (respectively) is taken as the price range bound.
    ///
    /// When pool doesn't exist, out of three arguments: `amount_a`, `amount_b` and `user_price`,
    /// exactly two must be specified: either a) one amount and `user_price`, or  b) two amounts.
    /// The third (unspecified) quantity will be evaluated from the specified two.
    ///
    /// When pool exists, the position is opened in accordance with the current spot price,
    /// and only one amount need to be specified. Specifying `user_price` when pool already exists
    /// is an error.
    ///
    /// `slippage_tolerance` makes no sense for single-sided positions, so specifying it is an error.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn estimate_liq_add(
        &self,
        (token_a, token_b): (TokenId, TokenId),
        fee_rate: BasisPoints,
        ticks_range: (Option<i32>, Option<i32>),
        amount_a: Option<Amount>,
        amount_b: Option<Amount>,
        user_price: Option<Float>,
        slippage_tolerance_bp: BasisPoints,
    ) -> Result<EstimateAddLiquidityResult> {
        let tokens = (token_a, token_b);

        let fee_rates = fee_rates_ticks();

        #[allow(clippy::cast_possible_truncation)]
        let fee_level: FeeLevel = fee_rates
            .iter()
            .position(|&r| r == fee_rate)
            .ok_or_else(|| error_here!(ErrorKind::IllegalFee))?
            as FeeLevel;

        let pool_info = self.get_pool_info(tokens.clone())?;

        let spot_price = pool_info
            .as_ref()
            .map(|pool_info| pool_info.spot_sqrtprices[fee_level as usize].powi(2))
            .filter(|price| !price.is_zero());

        ensure!(
            amount_a.is_some() || amount_b.is_some(),
            error_here!(ErrorKind::SwapAmountTooSmall)
        );

        let ticks_range_unwrapped = Tick::unwrap_range(ticks_range).map_err(|e| error_here!(e))?;

        // Upper limits on `max_amount_a` and `max_amount_b` arguments of open_postion.
        // The limits ensure that resulting net_liquidity does not exceed MAX_NET_LIQUIDITY
        let max_amout_a_max = Amount::try_from(
            (ticks_range_unwrapped.1.eff_sqrtprice(fee_level, Side::Left)
                - ticks_range_unwrapped.0.eff_sqrtprice(fee_level, Side::Left))
                * MAX_NET_LIQUIDITY,
        )
        .unwrap_or(Amount::MAX)
        .min(
            Amount::MAX
                - pool_info
                    .as_ref()
                    .map_or(Amount::from(0u16), |pool_info| pool_info.total_reserves.0),
        );

        let max_amout_b_max = Amount::try_from(
            (ticks_range_unwrapped
                .1
                .eff_sqrtprice(fee_level, Side::Right)
                - ticks_range_unwrapped
                    .0
                    .eff_sqrtprice(fee_level, Side::Right))
                * MAX_NET_LIQUIDITY,
        )
        .unwrap_or(Amount::MAX)
        .min(
            Amount::MAX
                - pool_info.map_or(Amount::from(0u16), |pool_info| pool_info.total_reserves.1),
        );

        let (max_amount_a, max_amount_b) = if amount_a.is_none() {
            let max_amont_b = amount_b.ok_or_else(|| error_here!(ErrorKind::SwapAmountTooSmall))?;

            let max_amount_a = if let Some(user_price) = user_price {
                ensure!(
                    spot_price.is_none(),
                    error_here!(ErrorKind::PoolNotRegistered)
                );

                let user_eff_sqrtprices = EffSqrtprices::from_value(
                    eff_sqrtprice_from_spot_sqrtprice(user_price.sqrt(), fee_level),
                    Side::Left,
                    fee_level,
                    None,
                )
                .map_err(|e| error_here!(e))?;

                self.evaluate_open_position_at_eff_sqrtprices(
                    fee_rate,
                    ticks_range_unwrapped,
                    max_amout_a_max,
                    max_amont_b,
                    user_eff_sqrtprices,
                )?
                .0
            } else {
                ensure!(
                    spot_price.is_some(),
                    error_here!(ErrorKind::PoolNotRegistered)
                );
                self.evaluate_open_position(
                    &tokens,
                    fee_rate,
                    ticks_range,
                    max_amout_a_max,
                    max_amont_b,
                )?
                .0
            };
            (max_amount_a, max_amont_b)
        } else if amount_b.is_none() {
            let max_amount_a =
                amount_a.ok_or_else(|| error_here!(ErrorKind::SwapAmountTooSmall))?;

            let max_amont_b = if let Some(user_price) = user_price {
                ensure!(
                    spot_price.is_none(),
                    error_here!(ErrorKind::PoolNotRegistered)
                );

                let user_eff_sqrtprices = EffSqrtprices::from_value(
                    eff_sqrtprice_from_spot_sqrtprice(user_price.sqrt(), fee_level),
                    Side::Left,
                    fee_level,
                    None,
                )
                .map_err(|e| error_here!(e))?;

                self.evaluate_open_position_at_eff_sqrtprices(
                    fee_rate,
                    ticks_range_unwrapped,
                    max_amount_a,
                    max_amout_b_max,
                    user_eff_sqrtprices,
                )?
                .1
            } else {
                ensure!(
                    spot_price.is_some(),
                    error_here!(ErrorKind::PoolNotRegistered)
                );
                self.evaluate_open_position(
                    &tokens,
                    fee_rate,
                    ticks_range,
                    max_amount_a,
                    max_amout_b_max,
                )?
                .1
            };
            (max_amount_a, max_amont_b)
        } else {
            let amount_a = amount_a.ok_or_else(|| error_here!(ErrorKind::InternalLogicError))?;
            let amount_b = amount_b.ok_or_else(|| error_here!(ErrorKind::InternalLogicError))?;

            let (max_amount_a, max_amont_b, _price, _, _) =
                self.evaluate_open_position(&tokens, fee_rate, ticks_range, amount_a, amount_b)?;
            (max_amount_a, max_amont_b)
        };

        let (min_amount_a, min_amount_b, position_price, net_liquidity, expected_eff_sqrtprices) =
            self.evaluate_open_position(
                &tokens,
                fee_rate,
                ticks_range,
                max_amount_a,
                max_amount_b,
            )?;

        let slippage_tolerance =
            Float::from(slippage_tolerance_bp) / Float::from(BASIS_POINT_DIVISOR);
        let slippage_factor = (Float::one() - slippage_tolerance).sqrt();

        let min_a_eff_sqrtprices = {
            let mut min_a_eff_sqrtprices = EffSqrtprices::from_value(
                (expected_eff_sqrtprices.0 * slippage_factor)
                    .max(Tick::MIN.eff_sqrtprice(fee_level, Side::Left)),
                Side::Left,
                fee_level,
                None,
            )
            .map_err(|e| error_here!(e))?;
            min_a_eff_sqrtprices.1 = min_a_eff_sqrtprices.1.max(expected_eff_sqrtprices.1);
            min_a_eff_sqrtprices
        };

        let min_b_eff_sqrtprices = {
            let mut min_b_eff_sqrtprices = EffSqrtprices::from_value(
                (expected_eff_sqrtprices.1 * slippage_factor)
                    .max(Tick::MAX.eff_sqrtprice(fee_level, Side::Right)),
                Side::Right,
                fee_level,
                None,
            )
            .map_err(|e| error_here!(e))?;
            min_b_eff_sqrtprices.0 = min_b_eff_sqrtprices.0.max(expected_eff_sqrtprices.0);
            min_b_eff_sqrtprices
        };

        let min_amount_a = if max_amount_b > Amount::from(0u16) {
            self.evaluate_open_position_at_eff_sqrtprices(
                fee_rate,
                ticks_range_unwrapped,
                max_amount_a,
                max_amount_b,
                min_a_eff_sqrtprices,
            )?
            .0
        } else {
            min_amount_a
        };
        ensure!(
            min_amount_a <= max_amount_a,
            error_here!(ErrorKind::InternalLogicError)
        );

        let min_amount_b = if max_amount_a > Amount::from(0u16) {
            self.evaluate_open_position_at_eff_sqrtprices(
                fee_rate,
                ticks_range_unwrapped,
                max_amount_a,
                max_amount_b,
                min_b_eff_sqrtprices,
            )?
            .1
        } else {
            min_amount_b
        };
        ensure!(
            min_amount_b <= max_amount_b,
            error_here!(ErrorKind::InternalLogicError)
        );

        #[allow(clippy::useless_conversion)]
        let mut tx_cost = TxCostEstimate {
            gas_cost_max: Amount::from(OPEN_POSITION_COST_BASE),
            storage_fee_max: Amount::from(0u16),
        };

        if let Some(ticks_len) = self.get_pool_ticks(tokens, fee_level) {
            if ticks_len > 0 {
                #[cfg(feature = "near")]
                {
                    let ticks_len_log2 = u128::from(ticks_len.ilog2());
                    tx_cost.gas_cost_max =
                        OPEN_POSITION_COST_PER_TICK_LOG * ticks_len_log2 + OPEN_POSITION_COST_BASE;
                }
                #[cfg(any(feature = "multiversx", feature = "concordium"))]
                {
                    let ticks_len_log2 = u128::from(ticks_len.ilog2());
                    tx_cost.gas_cost_max = Amount::from(
                        OPEN_POSITION_COST_PER_TICK_LOG * ticks_len_log2 + OPEN_POSITION_COST_BASE,
                    );
                }
            }
        }

        Ok(EstimateAddLiquidityResult {
            min_a: min_amount_a,
            max_a: max_amount_a,
            min_b: min_amount_b,
            max_b: max_amount_b,
            pool_exists: spot_price.is_some(),
            spot_price: spot_price.map(Into::into),
            position_price,
            tx_cost,
            position_net_liquidity: Float::from(net_liquidity),
        })
    }

    fn estimate_liq_remove(&self, position_id: u64) -> Result<EstimateRemoveLiquidityResult> {
        #[cfg(not(feature = "near"))]
        {
            let _: u64 = position_id;
            unimplemented!();
        }

        #[cfg(feature = "near")]
        {
            let mut tx_cost = TxCostEstimate {
                gas_cost_max: CLOSE_POSITION_COST_BASE,
                storage_fee_max: 0,
            };

            let pos_info = self.get_position_info(position_id)?;

            if let Some(ticks_len) = self.get_pool_ticks(pos_info.tokens_ids, pos_info.fee_level) {
                if ticks_len > 0 {
                    let ticks_len_log2 = u128::from(ticks_len.ilog2());

                    tx_cost.gas_cost_max = CLOSE_POSITION_COST_PER_TICK_LOG * ticks_len_log2
                        + CLOSE_POSITION_COST_BASE;
                }
            }

            Ok(EstimateRemoveLiquidityResult { tx_cost })
        }
    }
}

// Utility methods mixins
trait EstimationUtils {
    fn evaluate_open_position_at_eff_sqrtprices(
        &self,
        fee_rate: BasisPoints,
        ticks_range: (Tick, Tick),
        max_amount_a: Amount,
        max_amount_b: Amount,
        eff_sqrtprices: EffSqrtprices,
    ) -> Result<(Amount, Amount, Float)>;

    fn evaluate_open_position(
        &self,
        tokens: &(TokenId, TokenId),
        fee_rate: BasisPoints,
        ticks_range: (Option<i32>, Option<i32>),
        max_amount_a: Amount,
        max_amount_b: Amount,
    ) -> Result<(Amount, Amount, Float, Liquidity, EffSqrtprices)>;
}

impl<T: Types, S: State<T>, SS: Borrow<S>> EstimationUtils for Dex<T, S, SS> {
    /// Evaluate the outcome of opening a position given some effective price in the pool
    fn evaluate_open_position_at_eff_sqrtprices(
        &self,
        fee_rate: BasisPoints,
        ticks_range: (Tick, Tick),
        max_amount_a: Amount,
        max_amount_b: Amount,
        eff_sqrtprices: EffSqrtprices,
    ) -> Result<(Amount, Amount, Float)> {
        let mut factory = OverlayItemFactory::new();

        let fee_rates = fee_rates_ticks();
        #[allow(clippy::cast_possible_truncation)]
        let fee_level: FeeLevel = fee_rates
            .iter()
            .position(|&r| r == fee_rate)
            .ok_or_else(|| error_here!(ErrorKind::IllegalFee))?
            as FeeLevel;

        let mut pool = PoolStateOverlay::<T>::default();

        // set spot price to the specified value
        pool.open_position(
            PositionInit {
                amount_ranges: (
                    Range {
                        min: Amount::from(0u16).into(),
                        max: Amount::from(1u16).into(),
                    },
                    Range {
                        min: Amount::from(0u16).into(),
                        max: Amount::from(1u16).into(),
                    },
                ),
                ticks_range: (None, None),
            },
            fee_level,
            PositionId::MAX,
            &mut factory,
        )?;

        // Because of ambiquities we need to qualify full path
        for i_level in 0..NUM_FEE_LEVELS {
            pool::PoolState::set_eff_sqrtprices_at(&mut pool, i_level, eff_sqrtprices);
        }
        pool::PoolState::reset_top_active_level(&mut pool);
        pool::PoolState::set_active_side(&mut pool, Side::Left);

        let PositionOpenedInfo {
            deposited_amounts, ..
        } = pool.open_position(
            PositionInit {
                amount_ranges: (
                    Range {
                        min: Amount::from(0u16).into(),
                        max: max_amount_a.into(),
                    },
                    Range {
                        min: Amount::from(0u16).into(),
                        max: max_amount_b.into(),
                    },
                ),
                ticks_range: Tick::wrap_range(ticks_range),
            },
            fee_level,
            1,
            &mut factory,
        )?;
        let spot_price = pool.spot_price(Side::Left, fee_level);
        Ok((deposited_amounts.0, deposited_amounts.1, spot_price))
    }

    #[allow(clippy::too_many_lines)]
    fn evaluate_open_position(
        &self,
        tokens: &(TokenId, TokenId),
        fee_rate: BasisPoints,
        ticks_range: (Option<i32>, Option<i32>),
        max_amount_a: Amount,
        max_amount_b: Amount,
    ) -> Result<(Amount, Amount, Float, Liquidity, EffSqrtprices)> {
        let ticks_range = Tick::unwrap_range(ticks_range).map_err(|e| error_here!(e))?;

        let (amount_a, amount_b, liquidity, spot_price, expected_eff_sqrtprices) = {
            let position = PositionInit {
                amount_ranges: (
                    Range {
                        min: Amount::from(0u16).into(),
                        max: max_amount_a.into(),
                    },
                    Range {
                        min: Amount::from(0u16).into(),
                        max: max_amount_b.into(),
                    },
                ),
                ticks_range: Tick::wrap_range(ticks_range),
            };

            let (pool_id, transposed) = PoolId::try_from_pair((tokens.0.clone(), tokens.1.clone()))
                .map_err(|e| error_here!(e))?;
            let price_side = if transposed { Side::Right } else { Side::Left };

            let contract = self.contract().as_ref();

            let position = position.transpose_if(transposed);
            let fee_rates = fee_rates_ticks();

            let fee_level: FeeLevel = fee_rates
                .iter()
                .find_position(|r| **r == fee_rate)
                .ok_or(error_here!(ErrorKind::IllegalFee))?
                .0
                .try_into()
                .map_err(|_| error_here!(ErrorKind::ConvOverflow))?;

            let position_id = contract.next_free_position_id;
            let mut factory = OverlayItemFactory::new();
            let pos_clone = position.clone();

            let (
                PositionOpenedInfo {
                    deposited_amounts,
                    /// Accounted net liquidity
                    net_liquidity,
                    ..
                },
                spot_price,
                expected_eff_sqrtprices,
            ) = if let Ok(result) = contract.pools.try_inspect(&pool_id, |Pool::V0(ref pool)| {
                let mut pool = PoolStateOverlay::from(pool);

                let result = pool.open_position(pos_clone, fee_level, position_id, &mut factory)?;

                let spot_price = pool.spot_sqrtprices(price_side)[0].powi(2);
                let expected_eff_sqrtprices = pool::PoolState::eff_sqrtprices_at(&pool, fee_level);

                Ok((result, spot_price, expected_eff_sqrtprices))
            }) {
                result?
            } else {
                let mut pool = PoolStateOverlay::<T>::default();

                let result = pool.open_position(position, fee_level, position_id, &mut factory)?;

                let spot_price = pool.spot_sqrtprices(price_side)[0].powi(2);
                let expected_eff_sqrtprices = pool::PoolState::eff_sqrtprices_at(&pool, fee_level);

                (result, spot_price, expected_eff_sqrtprices)
            };

            let deposited_amounts_in_user_order = swap_if(transposed, deposited_amounts);
            let expected_eff_sqrtprices = expected_eff_sqrtprices.swap_if(transposed);
            (
                deposited_amounts_in_user_order.0,
                deposited_amounts_in_user_order.1,
                net_liquidity,
                spot_price,
                expected_eff_sqrtprices,
            )
        };

        Ok((
            amount_a,
            amount_b,
            spot_price,
            liquidity,
            expected_eff_sqrtprices,
        ))
    }
}
