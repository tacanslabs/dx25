use crate::{chain, dex, dex::pool, ensure_here, error_here, fp};
use array_init::array_init;
use chain::{
    AmountSFP, AmountUFP, FeeLiquidityUFP, Float, GrossLiquidityUFP, LPFeePerFeeLiquidity,
    Liquidity, LiquiditySFP, LongestSFP, LongestUFP, NetLiquidityUFP, MAX_EFF_TICK, MIN_EFF_TICK,
};
use dex::latest::{
    position_state_ex::eval_position_balance_ufp, EffSqrtprices, RawFeeLevelsArray, NUM_FEE_LEVELS,
};
use dex::traits::{Map as _, OrderedMap as _};
use dex::utils::{next_down, next_up, swap_if, MinSome as _, PairExt as _};
use dex::{
    traits, Amount, BasisPoints, EffTick, Error, ErrorKind, FeeLevel, PoolId, PoolInfo, PoolV0,
    Position, PositionClosedInfo, PositionId, PositionInfo, PositionInit, PositionOpenedInfo,
    PositionV0, Range, Result, Side, SwapKind, Tick, TickState, BASIS_POINT_DIVISOR,
    MAX_NET_LIQUIDITY, MIN_NET_LIQUIDITY, PRECALCULATED_TICKS,
};
use num_traits::{CheckedAdd, CheckedMul, CheckedSub, Zero};
#[cfg(feature = "smartlib")]
use pool::{inc_ticks_counter, reset_ticks_counter};
use pool::{Pool, PoolState, SWAP_MAX_UNDERPAY};
use std::cmp::Ordering;
use std::ops::Neg;

#[derive(PartialEq, Eq, Clone, Copy)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
pub enum StepLimit {
    StepComplete,
    LevelActivation,
    TickCrossing,
}

impl<T: traits::Types> PoolV0<T> {
    pub fn get_all_ticks_liquidity_change(
        &self,
        fee_level: FeeLevel,
        side: Side,
    ) -> Vec<(Tick, Float)> {
        let mut ticks = self.tick_states[fee_level]
            .iter()
            .map(|(tick, tick_state)| {
                let TickState::V0(ref tick_state) = *tick_state;
                (*tick, Float::from(tick_state.net_liquidity_change))
            })
            .collect::<Vec<_>>();

        if side == Side::Right {
            ticks = ticks
                .into_iter()
                .rev()
                .map(|(tick, liq_change)| (tick.opposite(), -liq_change))
                .collect();
        }

        ticks
    }

    pub fn get_ticks_liquidity_change(
        &self,
        fee_level: FeeLevel,
        side: Side,
        start_tick: Tick,
        number: u8,
    ) -> Vec<(Tick, Float)> {
        let mut ticks: Vec<(Tick, Float)> = Vec::new();

        // Include start tick
        let mut target_tick = unsafe { Tick::new_unchecked(start_tick.index() - 1) };

        for _ in 0..number {
            let result =
                self.tick_states[fee_level].inspect_above(&target_tick, |tick, tick_state| {
                    let TickState::V0(ref tick_state) = *tick_state;
                    (*tick, Float::from(tick_state.net_liquidity_change))
                });

            match result {
                Some((tick, net_liquidity_change)) => {
                    target_tick = tick;
                    ticks.push((tick, net_liquidity_change));
                }
                None => break,
            }
        }

        if side == Side::Right {
            ticks = ticks
                .into_iter()
                .rev()
                .map(|(tick, liq_change)| (tick.opposite(), -liq_change))
                .collect();
        }

        ticks
    }
}

impl<T: traits::Types, PS: PoolState<T>> Pool<T> for PS {
    fn spot_sqrtprice(&self, side: Side, level: FeeLevel) -> Float {
        self.eff_sqrtprice(level, side) / one_over_sqrt_one_minus_fee_rate(level)
    }

    fn spot_price(&self, side: Side, fee_level: FeeLevel) -> Float {
        let spot_sqrtprice = self.spot_sqrtprice(side, fee_level);
        spot_sqrtprice * spot_sqrtprice
    }

    fn spot_sqrtprices(&self, side: Side) -> RawFeeLevelsArray<Float> {
        array_init(|level| self.spot_sqrtprice(side, as_fee_level(level)))
    }

    fn eff_sqrtprice(&self, level: FeeLevel, side: Side) -> Float {
        self.eff_sqrtprice(level, side)
    }

    fn pool_info(&self, side: Side) -> Result<PoolInfo, Error> {
        let total_reserves = swap_if(side == Side::Right, self.total_reserves());
        let position_reserves_ufp = swap_if(side == Side::Right, self.sum_position_reserves());
        let position_reserves = position_reserves_ufp
            .try_map_into::<Amount, _>()
            .map_err(|e| error_here!(e))?;
        Ok(PoolInfo {
            total_reserves,
            position_reserves,
            spot_sqrtprices: self.spot_sqrtprices(side),
            eff_sqrtprices: self
                .eff_sqrtprices()
                .map(|eff_sqrtprices| eff_sqrtprices.as_tuple_swapped_if(side == Side::Right)),
            liquidities: self.liquidities(),
            fee_rates: fee_rates_ticks(),
            fee_divisor: BASIS_POINT_DIVISOR,
        })
    }

    fn liquidity(&self, fee_level: FeeLevel) -> Liquidity {
        // one_over_sqrt_one_minus_fee_rate < 1.0 so it will always fit into Liquidity
        let one_over_sqrt_one_minus_fee_rate =
            Liquidity::try_from(one_over_sqrt_one_minus_fee_rate(fee_level)).unwrap();

        self.net_liquidity_at(fee_level) * one_over_sqrt_one_minus_fee_rate
    }

    fn liquidities(&self) -> RawFeeLevelsArray<Liquidity> {
        array_init(|level| self.liquidity(as_fee_level(level)))
    }

    fn net_liquidity(&self, level: FeeLevel) -> NetLiquidityUFP {
        self.net_liquidity_at(level)
    }

    fn gross_liquidity(&self, level: FeeLevel) -> GrossLiquidityUFP {
        gross_liquidity_from_net_liquidity(self.net_liquidity_at(level), level)
    }

    fn fee_liquidity(&self, level: FeeLevel) -> FeeLiquidityUFP {
        fee_liquidity_from_net_liquidity(self.net_liquidity_at(level), level)
    }

    fn position_reserves(&self) -> RawFeeLevelsArray<(AmountUFP, AmountUFP)> {
        self.position_reserves()
    }

    fn withdraw_fee(&mut self, position_id: u64) -> Result<(Amount, Amount)> {
        let Position::V0(pos) = self
            .get_position(position_id)
            .ok_or(error_here!(ErrorKind::PositionDoesNotExist))?;

        let acc_lp_fees_per_fee_liquidity =
            self.acc_range_lp_fees_per_fee_liquidity(pos.fee_level, pos.tick_bounds)?;
        let reward_ufp = self.position_reward_ufp(&pos, false)?;

        let reward = reward_ufp
            .try_map_into::<Amount, _>()
            .map_err(|e| error_here!(e))?;

        let Position::V0(mut pos) = self
            .get_position(position_id)
            .ok_or(error_here!(ErrorKind::PositionDoesNotExist))?;

        pos.unwithdrawn_acc_lp_fees_per_fee_liquidity = acc_lp_fees_per_fee_liquidity;
        self.insert_position(position_id, Position::V0(pos));

        self.dec_total_reserves(reward)
            .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;

        self.dec_acc_lp_fee(Side::Left, reward_ufp.0);
        self.dec_acc_lp_fee(Side::Right, reward_ufp.1);

        Ok(reward)
    }

    fn withdraw_protocol_fee(&mut self) -> Result<(Amount, Amount)> {
        let total_reserves = self.total_reserves().map_into::<AmountUFP>();
        let sum_position_reserves = self.sum_position_reserves();

        let payout_x = Amount::try_from(
            (total_reserves.0 - sum_position_reserves.0 - self.acc_lp_fee(Side::Left)).floor(),
        )
        .map_err(|e| error_here!(e))?;
        let payout_y = Amount::try_from(
            (total_reserves.1 - sum_position_reserves.1 - self.acc_lp_fee(Side::Right)).floor(),
        )
        .map_err(|e| error_here!(e))?;

        self.dec_total_reserves((payout_x, payout_y))
            .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;

        Ok((payout_x, payout_y))
    }

    /// Withdraw LP reward fees and close position.
    /// We intentionally prohibit closing position without withdrawing the reward.
    fn withdraw_fee_and_close_position(&mut self, position_id: u64) -> Result<PositionClosedInfo> {
        let fees = self.withdraw_fee(position_id)?;

        let Position::V0(pos) = self
            .get_position(position_id)
            .ok_or(error_here!(ErrorKind::PositionDoesNotExist))?;

        let balance_ufp = pos.eval_position_balance_ufp(self.eff_sqrtprices_at(pos.fee_level))?;

        self.remove_position(position_id);

        let balance = balance_ufp
            .try_map_into::<Amount, _>()
            .map_err(|e| error_here!(e))?;

        self.dec_total_reserves(balance)
            .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;

        self.dec_position_reserve_at(pos.fee_level, Side::Left, balance_ufp.0)
            .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;
        self.dec_position_reserve_at(pos.fee_level, Side::Right, balance_ufp.1)
            .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;

        if self
            .cmp_spot_price_to_position_range(pos.fee_level, pos.tick_bounds)?
            .is_eq()
        {
            self.dec_net_liquidity_at(pos.fee_level, pos.net_liquidity);
        }

        let net_liquidity_sfp = LiquiditySFP::from(pos.net_liquidity);

        let new_net_liquidity_change_lower =
            self.tick_remove_liquidity(pos.fee_level, pos.tick_bounds.0, net_liquidity_sfp)?;
        let new_net_liquidity_change_upper =
            self.tick_remove_liquidity(pos.fee_level, pos.tick_bounds.1, net_liquidity_sfp.neg())?;

        // Reset pool state if the last position is closed
        if !self.contains_any_positions() {
            self.reset_eff_sqrtprices();
            self.reset_top_active_level();

            // Consistency check:
            for level in 0..NUM_FEE_LEVELS {
                ensure_here!(
                    self.next_active_tick(level, Side::Left).is_none(),
                    ErrorKind::InternalLogicError
                );
                ensure_here!(
                    self.next_active_tick(level, Side::Right).is_none(),
                    ErrorKind::InternalLogicError
                );
            }
        }

        Ok(PositionClosedInfo {
            fees,
            balance,
            fee_level: pos.fee_level,
            low_tick_liquidity_change: (
                pos.tick_bounds.0,
                Float::from(new_net_liquidity_change_lower),
            ),
            high_tick_liquidity_change: (
                pos.tick_bounds.1,
                Float::from(new_net_liquidity_change_upper),
            ),
        })
    }

    fn get_position_info(&self, pool_id: &PoolId, position_id: PositionId) -> Result<PositionInfo> {
        let Position::V0(pos) = self
            .get_position(position_id)
            .ok_or(error_here!(ErrorKind::PositionDoesNotExist))?;
        Ok(PositionInfo {
            tokens_ids: pool_id.as_refs().map(Clone::clone),
            fee_level: pos.fee_level,
            balance: self.eval_position_balance(&pos)?,
            init_sqrtprice: pos.init_sqrtprice,
            range_ticks: pos.tick_bounds,
            reward_since_last_withdraw: self.position_reward(&pos, false)?,
            reward_since_creation: self.position_reward(&pos, true)?,
            net_liquidity: Float::from(pos.net_liquidity),
        })
    }

    /// Evaluate amounts of tokens to be deposited in the pool,
    /// and actually accunted net liquidity of the position.
    #[allow(clippy::too_many_lines)] // Refactor?
    #[allow(clippy::needless_pass_by_value)] // `position` is actually deconstructed, no idea why Clippy complains
    fn open_position(
        &mut self,
        position: PositionInit,
        fee_level: FeeLevel,
        position_id: PositionId,
        factory: &mut dyn dex::ItemFactory<T>,
    ) -> Result<PositionOpenedInfo> {
        let PositionInit {
            amount_ranges:
                (
                    Range {
                        min: left_min,
                        max: left_max,
                    },
                    Range {
                        min: right_min,
                        max: right_max,
                    },
                ),
            ticks_range,
        } = position;
        let left_min: Amount = left_min.into();
        let left_max: Amount = left_max.into();
        let right_min: Amount = right_min.into();
        let right_max: Amount = right_max.into();

        let (tick_low, tick_high) = Tick::unwrap_range(ticks_range).map_err(|e| error_here!(e))?;

        ensure_here!(left_max >= left_min, ErrorKind::InvalidParams);
        ensure_here!(right_max >= right_min, ErrorKind::InvalidParams);
        ensure_here!(tick_high > tick_low, ErrorKind::InvalidParams);

        let left_max_float: Float = next_down(left_max.into());
        let right_max_float: Float = next_down(right_max.into());

        if !self.is_spot_price_set() {
            self.init_pool_from_position(
                left_max_float,
                right_max_float,
                tick_low,
                tick_high,
                fee_level,
            )?;
        }

        // Update next active ticks taking into account the new position:
        for new_tick in [tick_low, tick_high] {
            self.update_next_active_ticks(new_tick, fee_level)?;
        }

        let accounted_net_liquidity = self.eval_accounted_net_liquidity(
            (left_max_float, right_max_float),
            (tick_low, tick_high),
            fee_level,
        )?;

        let init_acc_lp_fees_per_fee_liquidity =
            self.acc_range_lp_fees_per_fee_liquidity(fee_level, (tick_low, tick_high))?;
        let init_sqrtprice = self.spot_sqrtprice(Side::Right, fee_level);

        ensure_here!(
            self.get_position(position_id).is_none(),
            ErrorKind::PositionAlreadyExists
        );

        self.insert_position(
            position_id,
            factory.new_position(
                fee_level,
                accounted_net_liquidity,
                init_acc_lp_fees_per_fee_liquidity,
                (tick_low, tick_high),
                init_sqrtprice,
            )?,
        );

        let low_tick_liquidity_change = self.tick_add_liquidity(
            factory,
            fee_level,
            tick_low,
            LiquiditySFP::from(accounted_net_liquidity),
        )?;

        let high_tick_liquidity_change = self.tick_add_liquidity(
            factory,
            fee_level,
            tick_high,
            LiquiditySFP::from(accounted_net_liquidity).neg(),
        )?;

        let accounted_deposit_ufp = eval_position_balance_ufp(
            accounted_net_liquidity,
            tick_low,
            tick_high,
            self.eff_sqrtprices_at(fee_level),
            fee_level,
        )?;

        self.inc_position_reserve_at(fee_level, Side::Left, accounted_deposit_ufp.0)
            .map_err(|()| error_here!(ErrorKind::DepositWouldOverflow))?;
        self.inc_position_reserve_at(fee_level, Side::Right, accounted_deposit_ufp.1)
            .map_err(|()| error_here!(ErrorKind::DepositWouldOverflow))?;

        // In case the spot price is within the position range, we need to add up the deposited liquidity
        // to the current active liquidity.
        if self
            .cmp_spot_price_to_position_range(fee_level, (tick_low, tick_high))?
            .is_eq()
        {
            self.inc_net_liquidity_at(fee_level, accounted_net_liquidity);
        }

        // We can't charge LP with a non-integer amount of tokens, so we round the amounts up.
        // The difference will effectively go into the protocol fee.
        let actual_deposit = (
            Amount::try_from(accounted_deposit_ufp.0.ceil()).map_err(|e| error_here!(e))?,
            Amount::try_from(accounted_deposit_ufp.1.ceil()).map_err(|e| error_here!(e))?,
        );

        // Accounted deposit must never exceed the actual one:
        ensure_here!(actual_deposit.0 <= left_max, ErrorKind::InternalLogicError);
        ensure_here!(actual_deposit.1 <= right_max, ErrorKind::InternalLogicError);

        // Check if token ranges are consistent with the current spot price:
        ensure_here!(actual_deposit.0 >= left_min, ErrorKind::Slippage);
        ensure_here!(actual_deposit.1 >= right_min, ErrorKind::Slippage);

        // At least one of the tokens must be deposited:
        ensure_here!(
            actual_deposit.0 >= Amount::zero() || actual_deposit.1 >= Amount::zero(),
            ErrorKind::InternalLogicError
        );

        ensure_here!(
            AmountSFP::from(accounted_deposit_ufp.0) <= AmountSFP::from(actual_deposit.0),
            ErrorKind::InternalDepositMoreThanMax
        );
        ensure_here!(
            AmountSFP::from(accounted_deposit_ufp.1) <= AmountSFP::from(actual_deposit.1),
            ErrorKind::InternalDepositMoreThanMax
        );

        self.inc_total_reserves(actual_deposit)
            .map_err(|()| error_here!(ErrorKind::DepositWouldOverflow))?;

        Ok(PositionOpenedInfo {
            deposited_amounts: actual_deposit,
            net_liquidity: accounted_net_liquidity,
            low_tick_liquidity_change: (tick_low, Float::from(low_tick_liquidity_change)),
            high_tick_liquidity_change: (tick_high, Float::from(high_tick_liquidity_change)),
        })
    }

    fn swap_exact_in(
        &mut self,
        side: Side,
        amount_in: Amount,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(Amount, Amount, u32)> {
        self.swap_exact_in_or_to_price_impl((side, amount_in, protocol_fee_fraction, None))
    }

    fn swap_exact_out(
        &mut self,
        side: Side,
        amount_out: Amount,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(Amount, Amount, u32)> {
        ensure_here!(!amount_out.is_zero(), ErrorKind::InvalidParams);
        ensure_here!(self.is_spot_price_set(), ErrorKind::InsufficientLiquidity);

        #[cfg(feature = "smartlib")]
        reset_ticks_counter();

        self.update_active_side(side);
        let init_eff_sqrtprice = self.active_eff_sqrtprice();

        let mut amount_in_float = Float::zero();
        let mut amount_out_sfp = AmountSFP::from(amount_out);
        let mut num_tick_crossings = 0_u32;

        while amount_out_sfp > AmountSFP::zero() {
            let sum_gross_liquidities = Float::from(self.active_gross_liquidity());

            let new_eff_sqrtprice = eval_required_new_eff_sqrtprice_exact_out(
                self.active_eff_sqrtprice(),
                Float::from(amount_out_sfp),
                sum_gross_liquidities,
            )?;
            let (in_amount_change, out_amount_change, _limit_kind, num_tick_crossings_this_step) =
                self.try_step_to_price(
                    new_eff_sqrtprice,
                    sum_gross_liquidities,
                    protocol_fee_fraction,
                )?;
            num_tick_crossings += num_tick_crossings_this_step;

            amount_in_float += in_amount_change;
            amount_out_sfp -= AmountSFP::from(out_amount_change);
        }

        // round the amount-to-pay in favor of dex:
        amount_in_float = amount_in_float.ceil();

        let amount_in = Amount::try_from(amount_in_float)
            .map_err(|e: fp::Error| match e {
                fp::Error::Overflow => ErrorKind::SwapAmountTooLarge,
                other => ErrorKind::from(other),
            })
            .map_err(|e| error_here!(e))?;

        ensure_here!(amount_in > Amount::zero(), ErrorKind::SwapAmountTooSmall);
        ensure_here!(
            amount_in_float / Float::from(amount_out)
                >= (Float::one() - SWAP_MAX_UNDERPAY) * init_eff_sqrtprice * init_eff_sqrtprice,
            ErrorKind::InternalLogicError
        );

        self.inc_total_reserve(side, amount_in)
            .map_err(|()| error_here!(ErrorKind::DepositWouldOverflow))?;
        self.dec_total_reserve(side.opposite(), amount_out)
            .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;
        Ok((amount_in, amount_out, num_tick_crossings))
    }

    fn swap(
        &mut self,
        side: Side,
        swap_type: SwapKind,
        amount: Amount,
        protocol_fee_fraction: BasisPoints,
        price_limit: Option<Float>,
    ) -> Result<(Amount, Amount, u32)> {
        match swap_type {
            SwapKind::ExactIn => self.swap_exact_in(side, amount, protocol_fee_fraction),
            SwapKind::ExactOut => self.swap_exact_out(side, amount, protocol_fee_fraction),
            SwapKind::ToPrice => {
                ensure_here!(price_limit.is_some(), ErrorKind::InvalidParams);

                self.swap_to_price(side, amount, price_limit.unwrap(), protocol_fee_fraction)
            }
        }
    }

    fn swap_to_price(
        &mut self,
        side: Side,
        max_amount_in: Amount,
        max_eff_sqrtprice: Float,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(Amount, Amount, u32)> {
        if max_eff_sqrtprice <= self.eff_sqrtprice(0, side) {
            return Ok((Amount::zero(), Amount::zero(), 0));
        }
        self.swap_exact_in_or_to_price_impl((
            side,
            max_amount_in,
            protocol_fee_fraction,
            Some(max_eff_sqrtprice),
        ))
    }

    #[cfg(feature = "smart-routing")]
    fn reserves_ratio(&self) -> Liquidity {
        let left_u128x128: Liquidity = From::from(self.total_reserves().0);
        let right_u128x128: Liquidity = From::from(self.total_reserves().1);
        left_u128x128 / right_u128x128
    }

    #[cfg(feature = "smart-routing")]
    fn total_liquidity(&self) -> Liquidity {
        self.liquidities().into_iter().sum()
    }
}

pub(crate) trait PoolImpl<T: traits::Types>: PoolState<T> {
    /// Set active swap side, and if the side has changed, reset `top_active_level` and flip pivot tick
    /// Should be called in the beginning of a swap.
    fn update_active_side(&mut self, side: Side) {
        if side != self.active_side() {
            self.reset_top_active_level();
            self.set_active_side(side);
            self.set_pivot(self.pivot().opposite(0));
        }
    }

    /// Sqrt of effective price in the active swap direction (equal for all active levels).
    fn active_eff_sqrtprice(&self) -> Float {
        self.eff_sqrtprice(self.top_active_level(), self.active_side())
    }

    fn sum_position_reserves(&self) -> (AmountUFP, AmountUFP) {
        let mut amounts = (AmountUFP::zero(), AmountUFP::zero());
        for level in 0..NUM_FEE_LEVELS {
            amounts.0 += self.position_reserves_at(level).0;
            amounts.1 += self.position_reserves_at(level).1;
        }
        amounts
    }

    /// Sum of gross liquidities on levels from 0 to top active level including
    fn active_gross_liquidity(&self) -> GrossLiquidityUFP {
        let mut sum_gross_liquidities = GrossLiquidityUFP::zero();
        for level in 0..=self.top_active_level() {
            sum_gross_liquidities +=
                gross_liquidity_from_net_liquidity(self.net_liquidity_at(level), level);
        }
        sum_gross_liquidities
    }

    /// Sum of fee liquidities on levels from 0 to top active level including
    fn active_fee_liquidity(&self) -> FeeLiquidityUFP {
        let mut sum_fee_liquidities = FeeLiquidityUFP::zero();
        for level in 0..=self.top_active_level() {
            sum_fee_liquidities +=
                fee_liquidity_from_net_liquidity(self.net_liquidity_at(level), level);
        }
        sum_fee_liquidities
    }

    /// Amount of tokens locked in position
    fn eval_position_balance(&self, pos: &PositionV0<T>) -> Result<(Amount, Amount), Error> {
        let balances_ufp = pos.eval_position_balance_ufp(self.eff_sqrtprices_at(pos.fee_level))?;

        let balance = balances_ufp
            .try_map_into::<Amount, _>()
            .map_err(|e| error_here!(e))?;

        Ok(balance)
    }

    /// Fast check if pool is not empty. Relies on that `eff_sqrtprices` are reset.
    fn is_spot_price_set(&self) -> bool {
        // When pool is just created, or all positions are deleted,
        // we set all eff_sqrtprices to zero, which is otherwise invalid.
        !self.eff_sqrtprice(0, Side::Left).is_zero()
    }

    /// Determine if current spot price on `fee_level` is within `ticks_range`, lesser than or greater than `ticks_range`.
    ///
    /// Comparing the prices is not reliable, as the price may lay exactly on a tick,
    /// and we must clearly and unambiguously distinguish, whether a tick was already crossed.
    /// Therefore, we compare next active ticks.
    ///
    /// Notice: `self.next_active_ticks_left` and `self.next_active_ticks_right` must already be updated with `ticks_range` ticks.
    ///
    /// # Arguments
    ///
    /// - `fee_level` - fee level, spot price may be different on different fee levels
    /// - `ticks_range` - must be initialized ticks, or may return an error
    ///
    /// # Returns
    /// - `Ok(Ordering::Equal)` if spot price is within `ticks_range`
    /// - `Ok(Ordering::Less)` if spot price is less than lower bound of `ticks_range`
    /// - `Ok(Ordering::Greater)` if spot price is greater than upper bound of `ticks_range`
    /// - `Err(_)` if some ticks were not found
    fn cmp_spot_price_to_position_range(
        &self,
        fee_level: FeeLevel,
        ticks_range: (Tick, Tick),
    ) -> Result<Ordering> {
        match (
            self.next_active_tick(fee_level, Side::Left),
            self.next_active_tick(fee_level, Side::Right),
        ) {
            (Some(next_active_tick_left), Some(next_active_tick_right)) => {
                if ticks_range[Side::Left] <= next_active_tick_right
                    && next_active_tick_left <= ticks_range[Side::Right]
                {
                    Ok(Ordering::Equal)
                } else if ticks_range[Side::Right] <= next_active_tick_right {
                    Ok(Ordering::Greater)
                } else if next_active_tick_left <= ticks_range[Side::Left] {
                    Ok(Ordering::Less)
                } else {
                    Err(error_here!(ErrorKind::InternalLogicError))
                }
            }
            (Some(next_active_tick_left), None)
                if next_active_tick_left <= ticks_range[Side::Left] =>
            {
                Ok(Ordering::Less)
            }
            (None, Some(next_active_tick_right))
                if ticks_range[Side::Right] <= next_active_tick_right =>
            {
                Ok(Ordering::Greater)
            }
            _ => Err(error_here!(ErrorKind::InternalTickNotFound)),
        }
    }

    // fn nearest_active_eff_tick(&self, side: Side, top_active_level: FeeLevel) -> Option<EffTick> {
    //     // select next active ticks for the given swap direction:
    //     match side {
    //         Side::Left => &self.next_active_ticks_left,
    //         Side::Right => &self.next_active_ticks_right,
    //     }
    //     .iter()
    //     // consider only active levels:
    //     .take((top_active_level + 1).into())
    //     // map into EffTicks and filter-out Nones:
    //     .enumerate()
    //     .filter_map(|(level, opt_tick)| {
    //         opt_tick.map(|tick| EffTick::from_tick(tick, as_fee_level(level), side))
    //     })
    //     // select nearest EffTick
    //     .min()
    // }

    fn nearest_active_eff_tick(&self) -> Option<EffTick> {
        let mut nearest_active_eff_tick: Option<EffTick> = None;
        for level in 0..=self.top_active_level() {
            if let Some(tick) = self.next_active_tick(level, self.active_side()) {
                let eff_tick = EffTick::from_tick(tick, level, self.active_side());
                nearest_active_eff_tick = match nearest_active_eff_tick {
                    None => Some(eff_tick),
                    Some(nearest_active_eff_tick) => Some(nearest_active_eff_tick.min(eff_tick)),
                }
            }
        }
        nearest_active_eff_tick
    }

    /// Global accumulated LP fee (one side) per net liquidity, since the very beginning of dex operation.
    fn acc_lp_fee_per_fee_liquidity(
        &self,
        fee_level: FeeLevel,
        side: Side,
    ) -> LPFeePerFeeLiquidity {
        let mut acc_lp_fee_per_fee_liquidity = LPFeePerFeeLiquidity::zero();
        for level in fee_level..NUM_FEE_LEVELS {
            acc_lp_fee_per_fee_liquidity += self.acc_lp_fee_per_fee_liquidity_at(level, side);
        }
        acc_lp_fee_per_fee_liquidity
    }

    /// Initialize effective prices, pivot, top active level and active side,
    /// based on a given effective price on a given level.
    fn init_pool_from_eff_sqrtprice(
        &mut self,
        eff_sqrtprice: Float,
        side: Side,
        fee_level: FeeLevel,
    ) -> Result<()> {
        self.set_pivot(find_pivot(EffTick::default(), eff_sqrtprice).map_err(|e| error_here!(e))?);
        for i_fee_level in 0..NUM_FEE_LEVELS {
            let pivot_opposite_this_level = EffTick::new(
                self.pivot().index() - i32::from(fee_rate_ticks(fee_level))
                    + i32::from(fee_rate_ticks(i_fee_level)),
            )
            .map_err(|e| error_here!(e))?;
            let eff_sqrtprice_this_level = (eff_sqrtprice / self.pivot().eff_sqrtprice())
                * pivot_opposite_this_level.eff_sqrtprice();
            let eff_sqrtprices_this_level = EffSqrtprices::from_value(
                eff_sqrtprice_this_level,
                side,
                i_fee_level,
                Some(self.pivot()),
            )
            .map_err(|e| error_here!(e))?;
            self.set_eff_sqrtprices_at(i_fee_level, eff_sqrtprices_this_level);
        }
        self.reset_top_active_level();
        self.set_active_side(side);
        Ok(())
    }

    /// Initialize effective prices, pivot, top active level and active side,
    /// based on parameters of the first position.
    fn init_pool_from_position(
        &mut self,
        left_max: Float,
        right_max: Float,
        tick_low: Tick,
        tick_high: Tick,
        fee_level: FeeLevel,
    ) -> Result<()> {
        let (eff_sqrtprice, side) =
            eval_initial_eff_sqrtprice(left_max, right_max, tick_low, tick_high, fee_level)?;
        self.init_pool_from_eff_sqrtprice(eff_sqrtprice, side, fee_level)?;
        Ok(())
    }

    /// Evaluate net liquidity corresponding to `max_amounts` and `tick_bounds` on the given `fee_level`.
    /// Notice: `self.next_active_ticks` must be already updated with `tick_bounds`.
    fn eval_accounted_net_liquidity(
        &self,
        max_amounts: (Float, Float),
        (tick_low, tick_high): (Tick, Tick),
        fee_level: FeeLevel,
    ) -> Result<NetLiquidityUFP> {
        // Determine if the spot price is below, between, or above the position bounds.
        // Here we determine it based on the next ticks to cross. However, if the spot
        // price is exactly on one of the ticks, `spot_price_wrt_position_bounds`
        // is not well defined:
        let spot_price_wrt_position_bounds =
            self.cmp_spot_price_to_position_range(fee_level, (tick_low, tick_high))?;

        // Handle the cases when the spot price is exactly on one of the bounds:
        let is_on_low_tick = self.eff_sqrtprice(fee_level, Side::Left)
            == tick_low.eff_sqrtprice(fee_level, Side::Left)
            || self.eff_sqrtprice(fee_level, Side::Right)
                == tick_low.eff_sqrtprice(fee_level, Side::Right);
        let is_on_high_tick = self.eff_sqrtprice(fee_level, Side::Left)
            == tick_high.eff_sqrtprice(fee_level, Side::Left)
            || self.eff_sqrtprice(fee_level, Side::Right)
                == tick_high.eff_sqrtprice(fee_level, Side::Right);
        let spot_price_wrt_position_bounds = if is_on_low_tick {
            Ordering::Less
        } else if is_on_high_tick {
            Ordering::Greater
        } else {
            spot_price_wrt_position_bounds
        };

        let net_liquidity_float = match spot_price_wrt_position_bounds {
            Ordering::Less => {
                // Spot price is below or at tick_low -- position consists of right token only
                ensure_here!(
                    max_amounts[Side::Right] > Float::zero(),
                    ErrorKind::Slippage
                );
                let eff_sqrtprice_right_high = tick_low.eff_sqrtprice(fee_level, Side::Right);
                let eff_sqrtprice_right_low = tick_high.eff_sqrtprice(fee_level, Side::Right);
                ensure_here!(
                    eff_sqrtprice_right_high > eff_sqrtprice_right_low,
                    ErrorKind::InternalLogicError
                );
                let liquidity_right = max_amounts[Side::Right]
                    / next_up(eff_sqrtprice_right_high - eff_sqrtprice_right_low);
                ensure_here!(liquidity_right.is_normal(), ErrorKind::InternalLogicError); // implies != 0

                liquidity_right
            }
            Ordering::Equal => {
                // Spot price is between tick_low and tick_high (and not either of the bounds)
                // -- position consists of both tokens.
                ensure_here!(
                    max_amounts[Side::Right] > Float::zero(),
                    ErrorKind::Slippage
                );
                ensure_here!(max_amounts[Side::Left] > Float::zero(), ErrorKind::Slippage);
                let eff_sqrtprice_left = self.eff_sqrtprice(fee_level, Side::Left);
                let eff_sqrtprice_left_low = tick_low.eff_sqrtprice(fee_level, Side::Left);
                ensure_here!(
                    eff_sqrtprice_left > eff_sqrtprice_left_low,
                    ErrorKind::InternalLogicError
                );
                let liquidity_left =
                    max_amounts[Side::Left] / next_up(eff_sqrtprice_left - eff_sqrtprice_left_low);

                let eff_sqrtprice_right = self.eff_sqrtprice(fee_level, Side::Right);
                let eff_sqrtprice_right_low = tick_high.eff_sqrtprice(fee_level, Side::Right);
                ensure_here!(
                    eff_sqrtprice_right > eff_sqrtprice_right_low,
                    ErrorKind::InternalLogicError
                );
                let liquidity_right = max_amounts[Side::Right]
                    / next_up(eff_sqrtprice_right - eff_sqrtprice_right_low);

                ensure_here!(liquidity_left.is_normal(), ErrorKind::InternalLogicError); // implies != 0
                ensure_here!(liquidity_right.is_normal(), ErrorKind::InternalLogicError); // implies != 0

                liquidity_left.min(liquidity_right)
            }
            Ordering::Greater => {
                // Spot price is above tick_high -- position consists of left token only
                ensure_here!(max_amounts[Side::Left] > Float::zero(), ErrorKind::Slippage);
                let eff_sqrtprice_left_high = tick_high.eff_sqrtprice(fee_level, Side::Left);
                let eff_sqrtprice_left_low = tick_low.eff_sqrtprice(fee_level, Side::Left);
                ensure_here!(
                    eff_sqrtprice_left_high > eff_sqrtprice_left_low,
                    ErrorKind::InternalLogicError
                );
                let liquidity_left = max_amounts[Side::Left]
                    / next_up(eff_sqrtprice_left_high - eff_sqrtprice_left_low);
                ensure_here!(liquidity_left.is_normal(), ErrorKind::InternalLogicError); // implies != 0

                liquidity_left
            }
        };

        ensure_here!(
            net_liquidity_float >= MIN_NET_LIQUIDITY,
            ErrorKind::LiquidityTooSmall
        );

        ensure_here!(
            net_liquidity_float <= MAX_NET_LIQUIDITY,
            ErrorKind::LiquidityTooBig
        );

        let net_liquidity_ufp =
            Liquidity::try_from(net_liquidity_float).map_err(|e| error_here!(e))?;

        Ok(net_liquidity_ufp)
    }

    /// Update `self.next_active_ticks_left` and `self.next_active_ticks_right`
    /// with newly inserted tick (upon opening a position).
    fn update_next_active_ticks(&mut self, new_tick: Tick, fee_level: FeeLevel) -> Result<()> {
        // The implementation must account, among other, for cases when:
        //  - one of the prices (left or right) is exactly on the new tick, and the other
        //    price is very close to it, but not exactly equal to it
        //  - when both prices exactly on the tick, but this tick was already active --
        //    in such case the next active ticks must not change

        if self.eff_sqrtprice(fee_level, Side::Left) < new_tick.eff_sqrtprice(fee_level, Side::Left)
        {
            ensure_here!(
                self.eff_sqrtprice(fee_level, Side::Right)
                    >= new_tick.eff_sqrtprice(fee_level, Side::Right),
                ErrorKind::InternalLogicError
            );
            ensure_here!(
                Some(new_tick) > self.next_active_tick(fee_level, Side::Right),
                ErrorKind::InternalLogicError
            );
            self.set_next_active_tick(
                fee_level,
                Side::Left,
                self.next_active_tick(fee_level, Side::Left)
                    .min_some(Some(new_tick)),
            );
        } else {
            ensure_here!(
                self.eff_sqrtprice(fee_level, Side::Right)
                    <= new_tick.eff_sqrtprice(fee_level, Side::Right),
                ErrorKind::InternalLogicError
            );

            if self.next_active_tick(fee_level, Side::Left) == Some(new_tick) {
                ensure_here!(
                    self.eff_sqrtprice(fee_level, Side::Left)
                        == new_tick.eff_sqrtprice(fee_level, Side::Left),
                    ErrorKind::InternalLogicError
                );
            } else {
                self.set_next_active_tick(
                    fee_level,
                    Side::Right,
                    self.next_active_tick(fee_level, Side::Right)
                        .max(Some(new_tick)),
                );
            }
        }
        Ok(())
    }

    /// LP fee per net liquidity, accumulated from the very beginning of dex operation, in the given range.
    fn acc_range_lp_fees_per_fee_liquidity(
        &self,
        fee_level: FeeLevel,
        tick_bounds: (Tick, Tick),
    ) -> Result<(LPFeePerFeeLiquidity, LPFeePerFeeLiquidity)> {
        // `unwrap_or_default` is used to evaluate `acc_lp_fees_per_fee_liquidity_outside` for new position when some ticks are not yet initialized.
        let lower_tick_acc_lp_fees_per_fee_liquidity_outside =
            self.get_tick_acc_lp_fees_per_fee_liquidity(fee_level, tick_bounds.0);
        let upper_tick_acc_lp_fees_per_fee_liquidity_outside =
            self.get_tick_acc_lp_fees_per_fee_liquidity(fee_level, tick_bounds.1);

        let acc_range_lp_fees_per_fee_liquidity =
            match self.cmp_spot_price_to_position_range(fee_level, tick_bounds)? {
                Ordering::Equal => {
                    // global:        ////////.////////.////////
                    // lower_outside: ////////.        .
                    // upper_outside:         .        .////////
                    // position  = global - lower_outside - upper_outside
                    (
                        self.acc_lp_fee_per_fee_liquidity(fee_level, Side::Left)
                            - lower_tick_acc_lp_fees_per_fee_liquidity_outside.0
                            - upper_tick_acc_lp_fees_per_fee_liquidity_outside.0,
                        self.acc_lp_fee_per_fee_liquidity(fee_level, Side::Right)
                            - lower_tick_acc_lp_fees_per_fee_liquidity_outside.1
                            - upper_tick_acc_lp_fees_per_fee_liquidity_outside.1,
                    )
                }
                Ordering::Less => {
                    // global:        ////////.////////.////////
                    // lower_outside:         .////////.////////
                    // upper_outside:         .        .////////
                    // position  = lower_outside - upper_outside
                    (
                        lower_tick_acc_lp_fees_per_fee_liquidity_outside.0
                            - upper_tick_acc_lp_fees_per_fee_liquidity_outside.0,
                        lower_tick_acc_lp_fees_per_fee_liquidity_outside.1
                            - upper_tick_acc_lp_fees_per_fee_liquidity_outside.1,
                    )
                }
                Ordering::Greater => {
                    // global:        ////////.////////.////////
                    // lower_outside: ////////.        .
                    // upper_outside: ////////.////////.
                    // position  = upper_outside - lower_outside
                    (
                        upper_tick_acc_lp_fees_per_fee_liquidity_outside.0
                            - lower_tick_acc_lp_fees_per_fee_liquidity_outside.0,
                        upper_tick_acc_lp_fees_per_fee_liquidity_outside.1
                            - lower_tick_acc_lp_fees_per_fee_liquidity_outside.1,
                    )
                }
            };
        Ok(acc_range_lp_fees_per_fee_liquidity)
    }

    fn position_reward_ufp(
        &self,
        pos: &PositionV0<T>,
        since_creation: bool,
    ) -> Result<(AmountUFP, AmountUFP)> {
        let pos_acc_lp_fees_per_fee_liquidity =
            self.acc_range_lp_fees_per_fee_liquidity(pos.fee_level, pos.tick_bounds)?;

        let initial_acc_lp_fees_per_fee_liquidity = if since_creation {
            pos.init_acc_lp_fees_per_fee_liquidity
        } else {
            pos.unwithdrawn_acc_lp_fees_per_fee_liquidity
        };

        let acc_lp_fees_per_fee_liquidity_diff = (
            pos_acc_lp_fees_per_fee_liquidity.0 - initial_acc_lp_fees_per_fee_liquidity.0,
            pos_acc_lp_fees_per_fee_liquidity.1 - initial_acc_lp_fees_per_fee_liquidity.1,
        );

        ensure_here!(
            acc_lp_fees_per_fee_liquidity_diff.0 >= LPFeePerFeeLiquidity::zero(),
            ErrorKind::InternalLogicError
        );
        ensure_here!(
            acc_lp_fees_per_fee_liquidity_diff.1 >= LPFeePerFeeLiquidity::zero(),
            ErrorKind::InternalLogicError
        );

        let fee_liquidity = LongestUFP::from(pos.fee_liquidity());
        let position_reward_ufp =
            acc_lp_fees_per_fee_liquidity_diff.map(|d| fee_liquidity * LongestUFP::from(d.value));

        #[cfg_attr(
            any(feature = "near", feature = "multiversx"),
            allow(clippy::useless_conversion)
        )]
        Ok((
            AmountUFP::try_from(position_reward_ufp.0).map_err(|e| error_here!(e))?,
            AmountUFP::try_from(position_reward_ufp.1).map_err(|e| error_here!(e))?,
        ))
    }

    fn position_reward(
        &self,
        pos: &PositionV0<T>,
        since_creation: bool,
    ) -> Result<(Amount, Amount)> {
        self.position_reward_ufp(pos, since_creation)?
            .try_map_into::<Amount, _>()
            .map_err(|e| error_here!(e))
    }

    /// Returns:
    ///  - number of crossed ticks
    fn cross_ticks(
        &mut self,
        crossed_eff_tick: EffTick,
        top_active_level: FeeLevel,
        side: Side,
    ) -> Result<u32> {
        let mut num_tick_crossings = 0_u32;
        for level in 0..=top_active_level {
            if let Some(next_active_tick) = self.next_active_tick(level, side) {
                if EffTick::from_tick(next_active_tick, level, side) == crossed_eff_tick {
                    self.flip_tick_acc_lp_fees_per_fee_liquidity_and_update_net_liquidity(
                        level,
                        next_active_tick,
                        side,
                    )?;

                    // Update next active ticks:
                    let new_next_active_tick =
                        self.find_next_active_tick_on_level(next_active_tick, level, side);
                    match side {
                        Side::Left => {
                            self.set_next_active_tick(
                                level,
                                Side::Right,
                                self.next_active_tick(level, Side::Left),
                            );
                            self.set_next_active_tick(level, Side::Left, new_next_active_tick);
                        }
                        Side::Right => {
                            self.set_next_active_tick(
                                level,
                                Side::Left,
                                self.next_active_tick(level, Side::Right),
                            );
                            self.set_next_active_tick(level, Side::Right, new_next_active_tick);
                        }
                    };
                    num_tick_crossings += 1;
                }
            }
        }

        #[cfg(feature = "smartlib")]
        inc_ticks_counter(num_tick_crossings as usize);

        Ok(num_tick_crossings)
    }

    fn update_prices_and_position_reserves(
        &mut self,
        fee_level: FeeLevel,
        new_eff_sqrtprices: EffSqrtprices,
    ) -> Result<(AmountSFP, AmountSFP)> {
        let old_eff_sqrtprices_sfp = self
            .eff_sqrtprices_at(fee_level)
            .as_tuple()
            .try_map_into::<LongestSFP, _>()
            .map_err(|e| error_here!(e))?;

        let new_eff_sqrtprices_sfp = (new_eff_sqrtprices.0, new_eff_sqrtprices.1)
            .try_map_into::<LongestSFP, _>()
            .map_err(|e| error_here!(e))?;

        let net_liquidity =
            LongestSFP::try_from(self.net_liquidity_at(fee_level)).map_err(|e| error_here!(e))?;

        let balance_change_sfp = (
            (new_eff_sqrtprices_sfp.0 - old_eff_sqrtprices_sfp.0)
                .checked_mul(&net_liquidity)
                .ok_or(error_here!(ErrorKind::SwapAmountTooLarge))?,
            (new_eff_sqrtprices_sfp.1 - old_eff_sqrtprices_sfp.1)
                .checked_mul(&net_liquidity)
                .ok_or(error_here!(ErrorKind::SwapAmountTooLarge))?,
        );

        #[cfg(feature = "concordium")]
        let balance_change_sfp = (
            AmountSFP::try_from(balance_change_sfp.0).map_err(|e| match e {
                fp::Error::Overflow => error_here!(ErrorKind::SwapAmountTooLarge),
                other => error_here!(ErrorKind::from(other)),
            })?,
            AmountSFP::try_from(balance_change_sfp.1).map_err(|e| match e {
                fp::Error::Overflow => error_here!(ErrorKind::SwapAmountTooLarge),
                other => error_here!(ErrorKind::from(other)),
            })?,
        );

        #[cfg(any(feature = "near", feature = "multiversx"))]
        // Ensure AmountSFP and LongestSFP are identical. Otherwise, replace with a conversion.
        let balance_change_sfp: (AmountSFP, AmountSFP) = balance_change_sfp;

        let swap_side = self.active_side();
        let opposite_side = swap_side.opposite();

        ensure_here!(
            balance_change_sfp[swap_side].non_negative
                || balance_change_sfp[swap_side].value.is_zero(),
            ErrorKind::InternalLogicError
        );
        ensure_here!(
            !balance_change_sfp[opposite_side].non_negative
                || balance_change_sfp[opposite_side].value.is_zero(),
            ErrorKind::InternalLogicError
        );

        self.inc_position_reserve_at(
            fee_level,
            self.active_side(),
            balance_change_sfp[self.active_side()].value,
        )
        .map_err(|()| error_here!(ErrorKind::SwapAmountTooLarge))?;

        // Position reserves must never turn negative, as this means we swap more than available liquidity,
        // but the price step must be already limited by the available liquidity.
        self.dec_position_reserve_at(
            fee_level,
            opposite_side,
            balance_change_sfp[opposite_side].value,
        )
        .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;

        self.set_eff_sqrtprices_at(fee_level, new_eff_sqrtprices);

        Ok(balance_change_sfp)
    }

    fn accumulate_lp_fee(
        &mut self,
        side: Side,
        lp_fee_per_fee_liquidity: LPFeePerFeeLiquidity,
    ) -> Result<()> {
        ensure_here!(
            lp_fee_per_fee_liquidity.non_negative,
            ErrorKind::InternalLogicError
        );
        let lp_fee_per_fee_liquidity = LongestUFP::from(lp_fee_per_fee_liquidity.value);
        let active_fee_liquidity = LongestUFP::from(self.active_fee_liquidity());

        // The multiplication lp_fee_per_fee_liquidity * active_fee_liquidity must be done without rounding,
        // so we check that the lowest words are zeros so that the product fits into LongestUFP.
        #[cfg(any(feature = "near", feature = "multiversx"))]
        ensure_here!(
            active_fee_liquidity.0 .0[0] == 0
                && active_fee_liquidity.0 .0[1] == 0
                && lp_fee_per_fee_liquidity.0 .0[0] == 0
                && lp_fee_per_fee_liquidity.0 .0[1] == 0,
            ErrorKind::InternalLogicError
        );
        #[cfg(feature = "concordium")]
        ensure_here!(
            active_fee_liquidity.0 .0[0] == 0
                && active_fee_liquidity.0 .0[1] == 0
                && active_fee_liquidity.0 .0[2] == 0
                && lp_fee_per_fee_liquidity.0 .0[0] == 0
                && lp_fee_per_fee_liquidity.0 .0[1] == 0,
            ErrorKind::InternalLogicError
        );

        #[cfg_attr(
            any(feature = "near", feature = "multiversx"),
            allow(clippy::useless_conversion)
        )]
        self.inc_acc_lp_fee(
            side,
            AmountUFP::try_from(lp_fee_per_fee_liquidity * active_fee_liquidity)
                .map_err(|e| error_here!(e))?,
        );

        Ok(())
    }

    fn accumulate_lp_fee_per_fee_liquidity(
        &mut self,
        lp_fee_per_fee_liquidity: LPFeePerFeeLiquidity,
    ) {
        self.inc_acc_lp_fee_per_fee_liquidity(
            self.active_side(),
            self.top_active_level(),
            lp_fee_per_fee_liquidity,
        );
    }

    fn accumulate_fees(
        &mut self,
        eff_sqrtprice_shift: Float,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<()> {
        let eff_sqrtprice_shift =
            LongestUFP::try_from(eff_sqrtprice_shift).map_err(|e| error_here!(e))?;
        let lp_fee_factor =
            LongestUFP::from(u128::from(BASIS_POINT_DIVISOR - protocol_fee_fraction))
                / LongestUFP::from(u128::from(BASIS_POINT_DIVISOR));
        let mut lp_fee_per_fee_liquidity = eff_sqrtprice_shift * lp_fee_factor;
        // Truncate `lp_fee_per_fee_liquidity`:
        //   on veax and dx25: X256 -> X128
        //   on cdex: X320 -> X192
        lp_fee_per_fee_liquidity.0 .0[0] = 0;
        lp_fee_per_fee_liquidity.0 .0[1] = 0;
        let lp_fee_per_fee_liquidity =
            LPFeePerFeeLiquidity::try_from(lp_fee_per_fee_liquidity).map_err(|e| error_here!(e))?;

        self.accumulate_lp_fee(self.active_side(), lp_fee_per_fee_liquidity)?;
        self.accumulate_lp_fee_per_fee_liquidity(lp_fee_per_fee_liquidity);

        Ok(())
    }

    /// Returns:
    ///  - `amount_in`,
    ///  - `amount_out`,
    ///  - `step_limit`,
    ///  - number of tick crossings
    #[allow(clippy::too_many_lines)]
    fn try_step_to_price(
        &mut self,
        mut new_eff_sqrtprice: Float,
        sum_gross_liquidities: Float,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(Float, AmountUFP, StepLimit, u32)> {
        ensure_here!(
            new_eff_sqrtprice >= self.active_eff_sqrtprice(),
            ErrorKind::InternalLogicError
        );

        // Check if new level is activated earlier
        let mut limit_kind = StepLimit::StepComplete;
        if self.top_active_level() < NUM_FEE_LEVELS - 1 {
            let next_level_eff_sqrtprice =
                self.eff_sqrtprice(self.top_active_level() + 1, self.active_side());
            if next_level_eff_sqrtprice <= new_eff_sqrtprice {
                new_eff_sqrtprice = next_level_eff_sqrtprice;
                limit_kind = StepLimit::LevelActivation;
            }
        }

        // Check if tick crossing happens earlier
        let nearest_active_eff_tick = self.nearest_active_eff_tick();

        if let Some(nearest_active_eff_tick) = nearest_active_eff_tick {
            let next_active_eff_tick_eff_sqrtprice = nearest_active_eff_tick.eff_sqrtprice();
            if next_active_eff_tick_eff_sqrtprice <= new_eff_sqrtprice {
                new_eff_sqrtprice = next_active_eff_tick_eff_sqrtprice;
                limit_kind = StepLimit::TickCrossing;
            }
        } else {
            ensure_here!(
                self.top_active_level() < NUM_FEE_LEVELS - 1,
                // Insufficient liquidity to complete the swap, and no tick crossing or level activation ahead
                ErrorKind::InsufficientLiquidity
            );
        }

        let init_eff_sqrtprice = self.active_eff_sqrtprice();

        let eff_sqrtprice_shift = new_eff_sqrtprice - init_eff_sqrtprice;

        let in_amount_change = eff_sqrtprice_shift * sum_gross_liquidities;

        self.set_pivot(find_pivot(self.pivot(), new_eff_sqrtprice).map_err(|e| error_here!(e))?);

        let mut out_amount_change = AmountUFP::zero();
        for level in 0..=self.top_active_level() {
            let mut new_eff_sqrtprices = if limit_kind == StepLimit::TickCrossing {
                let next_active_eff_tick =
                    nearest_active_eff_tick.ok_or(error_here!(ErrorKind::InternalLogicError))?;
                EffSqrtprices::from_tick(
                    &next_active_eff_tick
                        .to_tick(level, self.active_side())
                        .map_err(|e| match e {
                            ErrorKind::PriceTickOutOfBounds => ErrorKind::InsufficientLiquidity,
                            other => other,
                        })
                        .map_err(|e| error_here!(e))?,
                    level,
                )
            } else {
                EffSqrtprices::from_value(
                    new_eff_sqrtprice,
                    self.active_side(),
                    level,
                    Some(self.pivot()),
                )
                .map_err(|e| error_here!(e))?
            };

            match self.active_side() {
                Side::Left => {
                    new_eff_sqrtprices.1 = new_eff_sqrtprices
                        .1
                        .min(self.eff_sqrtprice(level, self.active_side().opposite()));
                }
                Side::Right => {
                    new_eff_sqrtprices.0 = new_eff_sqrtprices
                        .0
                        .min(self.eff_sqrtprice(level, self.active_side().opposite()));
                }
            }

            let out_amount_change_this_level = self
                .update_prices_and_position_reserves(level, new_eff_sqrtprices)?
                [self.active_side().opposite()];
            ensure_here!(
                out_amount_change_this_level <= AmountSFP::zero(),
                ErrorKind::InternalLogicError
            );
            out_amount_change += out_amount_change_this_level.value;
        }

        out_amount_change = out_amount_change.min(
            AmountUFP::try_from(in_amount_change / init_eff_sqrtprice / new_eff_sqrtprice)
                .map_err(|e| match e {
                    fp::Error::Overflow => ErrorKind::SwapAmountTooLarge,
                    other => ErrorKind::from(other),
                })
                .map_err(|e| error_here!(e))?,
        );

        self.accumulate_fees(eff_sqrtprice_shift, protocol_fee_fraction)?;

        if limit_kind == StepLimit::LevelActivation {
            self.inc_top_active_level();
        }

        let num_tick_crossings = if limit_kind == StepLimit::TickCrossing {
            let next_active_eff_tick =
                nearest_active_eff_tick.ok_or(error_here!(ErrorKind::InternalLogicError))?;
            self.cross_ticks(
                next_active_eff_tick,
                self.top_active_level(),
                self.active_side(),
            )?
        } else {
            0
        };

        Ok((
            in_amount_change,
            out_amount_change,
            limit_kind,
            num_tick_crossings,
        ))
    }

    fn swap_exact_in_or_to_price_impl(
        &mut self,
        // workaround of the bug with incorrectly passed arguments:
        // side: Side,
        // max_amount_in: Amount,
        // protocol_fee_fraction: BasisPoints,
        // max_eff_sqrtprice: Option<Float>,
        args: (Side, Amount, BasisPoints, Option<Float>),
    ) -> Result<(Amount, Amount, u32)> {
        let (side, max_amount_in, protocol_fee_fraction, max_eff_sqrtprice) = args;

        ensure_here!(!max_amount_in.is_zero(), ErrorKind::InvalidParams);
        ensure_here!(self.is_spot_price_set(), ErrorKind::InsufficientLiquidity);

        #[cfg(feature = "smartlib")]
        reset_ticks_counter();

        self.update_active_side(side);
        let init_eff_sqrtprice = self.active_eff_sqrtprice();

        let max_amount_in_float = Float::from(max_amount_in);
        let mut amount_in_float = Float::zero();
        let mut remaining_amount_in_float = max_amount_in_float;
        let mut amount_out_ufp = AmountUFP::zero();
        let mut num_tick_crossings = 0_u32;

        loop {
            let sum_gross_liquidities = Float::from(self.active_gross_liquidity());

            let mut new_eff_sqrtprice = eval_required_new_eff_sqrtprice_exact_in(
                self.active_eff_sqrtprice(),
                remaining_amount_in_float,
                sum_gross_liquidities,
            );

            if let Some(eff_sqrtprice_limit) = max_eff_sqrtprice {
                new_eff_sqrtprice = new_eff_sqrtprice.min(eff_sqrtprice_limit);
            }

            let (in_amount_change, out_amount_change, limit_kind, num_tick_crossings_this_step) =
                self.try_step_to_price(
                    new_eff_sqrtprice,
                    sum_gross_liquidities,
                    protocol_fee_fraction,
                )?;

            remaining_amount_in_float -= in_amount_change;
            amount_in_float += in_amount_change;
            amount_out_ufp += out_amount_change;
            num_tick_crossings += num_tick_crossings_this_step;

            if limit_kind == StepLimit::StepComplete {
                break;
            }
        }

        // Amount-in corresponding to the actual price shift may slightly exceed specified amount_in
        // due to numberic errors. The difference will be covered from the protocol fee.
        ensure_here!(
            remaining_amount_in_float >= -max_amount_in_float * SWAP_MAX_UNDERPAY,
            ErrorKind::InternalLogicError
        );
        // In exact-in swap we charge all provided amount_in
        // In swap-to-price we charge amount-in that corresponds to the price shift
        let amount_in = if max_eff_sqrtprice.is_some() {
            Amount::try_from(amount_in_float.ceil())
                .map_err(|e| match e {
                    fp::Error::Overflow => ErrorKind::SwapAmountTooLarge,
                    other => ErrorKind::from(other),
                })
                .map_err(|e| error_here!(e))?
                .min(max_amount_in)
        } else {
            max_amount_in
        };

        // implicit rounding-down
        let amount_out = Amount::try_from(amount_out_ufp)
            .map_err(|e| match e {
                fp::Error::Overflow => ErrorKind::SwapAmountTooLarge,
                other => ErrorKind::from(other),
            })
            .map_err(|e| error_here!(e))?;

        if max_eff_sqrtprice.is_none() {
            // Exact-in swap must result in non-zero amount-out (in contrast to swap-to-price).
            ensure_here!(amount_out > Amount::zero(), ErrorKind::SwapAmountTooSmall);
        }

        // Rough cross-check of resulting swap price:
        ensure_here!(
            amount_out.is_zero()
                || amount_in_float / Float::from(amount_out)
                    >= (Float::one() - SWAP_MAX_UNDERPAY) * init_eff_sqrtprice * init_eff_sqrtprice,
            ErrorKind::InternalLogicError
        );

        self.inc_total_reserve(side, amount_in)
            .map_err(|()| error_here!(ErrorKind::DepositWouldOverflow))?;
        self.dec_total_reserve(side.opposite(), amount_out)
            .map_err(|()| error_here!(ErrorKind::InternalLogicError))?;

        Ok((amount_in, amount_out, num_tick_crossings))
    }
}

impl<T: traits::Types, PS: PoolState<T>> PoolImpl<T> for PS {}

#[allow(clippy::cast_possible_truncation)]
pub fn as_fee_level(level: usize) -> FeeLevel {
    level as FeeLevel
}

/// Fee rate on the given fee level
pub fn fee_rate(fee_level: FeeLevel) -> Float {
    let one_over_one_minus_fee_rate = one_over_one_minus_fee_rate(fee_level);
    (one_over_one_minus_fee_rate - Float::one()) / one_over_one_minus_fee_rate
}

/// `1 / sqrt(1 - fee_rate)` for a given fee level
/// This quantity originates from the calculation method and determines the fee rates on each level.
pub fn one_over_sqrt_one_minus_fee_rate(fee_level: FeeLevel) -> Float {
    let tick_index = i32::from(fee_rate_ticks(fee_level));
    // Unwrap must succeed as long as fee_level is valid.
    debug_assert!(Tick::is_valid(tick_index));
    unsafe { Tick::new_unchecked(tick_index) }.spot_sqrtprice()
}

pub fn one_over_one_minus_fee_rate(fee_level: FeeLevel) -> Float {
    let tick_index = i32::from(2 * fee_rate_ticks(fee_level));
    // Unwrap must succeed as long as fee_level is valid.
    debug_assert!(Tick::is_valid(tick_index));
    unsafe { Tick::new_unchecked(tick_index) }.spot_sqrtprice()
}

pub fn fee_rate_ticks(fee_level: FeeLevel) -> BasisPoints {
    2_u16.pow(u32::from(fee_level))
}

pub fn fee_rates_ticks() -> RawFeeLevelsArray<BasisPoints> {
    array_init(|level| fee_rate_ticks(as_fee_level(level)))
}

/// Effective sqrtprice in the opposite swap direction
///
/// Since the ticks are not precisely equidistant, we use pivot tick for the inversion.
/// Pivot tick may be provided as optional argument, and ideally, its spot sqrtprice
/// should be not more than 1 tick away from `eff_sqrtprice`.
/// If pivot tick is not provided, or provided inaccurately, it is from scratch
/// or adjusted, which requires extra computations.
pub fn eff_sqrtprice_opposite_side(
    eff_sqrtprice: Float,
    fee_level: FeeLevel,
    pivot: Option<EffTick>,
) -> Result<Float, ErrorKind> {
    let pivot = find_pivot(pivot.unwrap_or_default(), eff_sqrtprice)?;
    debug_assert!(
        pivot.index() == MAX_EFF_TICK || eff_sqrtprice <= pivot.shifted(1).unwrap().eff_sqrtprice()
    );
    debug_assert!(
        pivot.index() == MIN_EFF_TICK
            || pivot.shifted(-1).unwrap().eff_sqrtprice() <= eff_sqrtprice
    );
    Ok((pivot.eff_sqrtprice() / eff_sqrtprice) * pivot.opposite(fee_level).eff_sqrtprice())
}

pub fn find_pivot(init_pivot: EffTick, eff_sqrtprice: Float) -> Result<EffTick, ErrorKind> {
    /// Min and max "distance" between `pivot.spot_sqrtprice`() and `eff_sqrtprice`, expressed as factor.
    /// This "distance" must not exceed 1 tick in order to achive sufficiently accurate price inversion.
    /// Currently chosen values are +/- 0.625 ticks.
    /// ```
    /// #[cfg(feature = "near")]
    /// use crate::veax_dex::dex::{Float, tick::Tick};
    /// #[cfg(feature = "concordium")]
    /// use crate::cdex::dex::{Float, tick::Tick};
    /// #[cfg(feature = "multiversx")]
    /// use crate::dx25::dex::{Float, tick::Tick};
    /// let base_pow_0625 = Tick::BASE.sqrt() * (Tick::BASE.sqrt().sqrt().sqrt());
    /// assert_eq!(base_pow_0625.recip().to_bits(), 0x3FEF_FFBE_77E2_8A1D);
    /// assert_eq!(base_pow_0625.to_bits(), 0x3FF0_0020_C451_D518);
    /// ```
    const DIST_MIN: Float = Float::from_bits(0x3FEF_FFBE_77E2_8A1D);
    const DIST_MAX: Float = Float::from_bits(0x3FF0_0020_C451_D518);

    /// If `distance_factor` (see below) is within this range, we calculate `log(distance_factor)`
    /// approximately, otherwise we use `PRECALCULATED_TICKS` LUT.
    /// ```
    /// #[cfg(feature = "near")]
    /// use crate::veax_dex::dex::{Float, tick::PRECALCULATED_TICKS};
    /// #[cfg(feature = "concordium")]
    /// use crate::cdex::dex::{Float, tick::PRECALCULATED_TICKS};
    /// #[cfg(feature = "multiversx")]
    /// use crate::dx25::dex::{Float, tick::PRECALCULATED_TICKS};
    /// let MIN_APPROXIMATE_LOG = Float::from_bits(PRECALCULATED_TICKS[12]).recip();
    /// assert_eq!(MIN_APPROXIMATE_LOG, Float::from_bits(0x3FEA_12FE_77BF_A405));
    /// ```
    const MAX_APPROXIMATE_LOG_INDEX: u32 = 12;
    const MAX_APPROXIMATE_LOG: Float =
        Float::from_bits(PRECALCULATED_TICKS[MAX_APPROXIMATE_LOG_INDEX as usize]);
    const MIN_APPROXIMATE_LOG: Float = Float::from_bits(0x3FEA_12FE_77BF_A405);

    let mut pivot = init_pivot;
    loop {
        // "distance" between eff_sqrtprice and pivot spot sqrtprice, expressed as factor.
        // `log(distance_factor)` is the actual distance between eff_sqrtprice
        // and pivot spot sqrtprice in units of log base.
        let distance_factor = eff_sqrtprice / pivot.eff_sqrtprice();

        if DIST_MIN < distance_factor && distance_factor < DIST_MAX {
            break;
        }

        let step_ticks = if distance_factor > MAX_APPROXIMATE_LOG {
            // log(distance_factor) is a large positive number: step by one of the PRECALCULATED_TICKS
            let step_ticks_log2: u32 = PRECALCULATED_TICKS
                .iter()
                .rposition(|&sqrtprice_bits| distance_factor >= Float::from_bits(sqrtprice_bits))
                .unwrap() // will always succeed because distance_factor > MAX_APPROXIMATE_LOG so the index can not be smaller than MAX_APPROXIMATE_LOG_INDEX
                .try_into()
                .unwrap(); // will always succeed as the index is limited to PRECALCULATED_TICKS.len()

            2i32.pow(step_ticks_log2)
        } else if distance_factor < MIN_APPROXIMATE_LOG {
            // log(distance_factor) is a large negative number: step by one of the PRECALCULATED_TICKS
            let step_ticks_log2: u32 = PRECALCULATED_TICKS
                .iter()
                .rposition(|&sqrtprice_bits| {
                    distance_factor.recip() >= Float::from_bits(sqrtprice_bits)
                })
                .unwrap() // will always succeed because distance_factor < MIN_APPROXIMATE_LOG, so distance_factor.recip() > MAX_APPROXIMATE_LOG, so the index can not be smaller than MAX_APPROXIMATE_LOG_INDEX
                .try_into()
                .unwrap(); // will always succeed as the index is limited to PRECALCULATED_TICKS.len()

            -(2i32.pow(step_ticks_log2))
        } else {
            // distance factor is small: use approximation for small x: (1+x)^n ~= 1+n*x
            let step_ticks_float =
                ((distance_factor - Float::one()) / (Tick::BASE - Float::one())).round();
            // Unwrap will always succeed because distance_factor can not exceed +/- 2^MAX_APPROXIMATE_LOG_INDEX (== 4096) ticks
            // and due to the approximation, step_ticks_float can only be slightly larger than that.
            let step_ticks: i32 = step_ticks_float.try_into().unwrap();

            // We limit the step to +/-2^MAX_APPROXIMATE_LOG_INDEX (== 4096) ticks
            // in order to make sure that pivot stays within valid tick range.
            step_ticks
                .clamp(
                    -(2i32.pow(MAX_APPROXIMATE_LOG_INDEX)),
                    2i32.pow(MAX_APPROXIMATE_LOG_INDEX),
                )
                .clamp(MIN_EFF_TICK - pivot.index(), MAX_EFF_TICK - pivot.index())
        };

        if step_ticks == 0 {
            return Err(ErrorKind::InternalLogicError);
        }

        pivot = pivot.shifted(step_ticks)?;
    }

    Ok(pivot)
}

/// Evaluate initial effective sqrtprice
pub fn eval_initial_eff_sqrtprice(
    amount_left: Float,
    amount_right: Float,
    tick_low: Tick,
    tick_high: Tick,
    fee_level: FeeLevel,
) -> Result<(Float, Side)> {
    if amount_left > Float::zero() && amount_right > Float::zero() {
        // The position consists of both left and right tokens. The price is determined
        // from the requirement that net_liquidity evaluated from left and right
        // token amounts is the same:
        // ```
        //     amount_left / (eff_sqrtprice_left - tick_low.eff_sqrtprice_left) =
        //         = amount_right / (eff_sqrtprice_right - tick_high.eff_sqrtprice_right)
        // ```
        // This leads to a quadratic equation, which can be solved either w.r.t. eff_sqrtprice_left
        // or w.r.t. eff_sqrtprice_right. Due to the limited numberic precision, one solution is more
        // accurate than the other. For the solution w.r.t. eff_sqrtprice_left the terms
        // of the quadratic equation are:
        // ```
        //   a = 1
        //   b = amount_left / amount_right * tick_high.eff_sqrtprice_right
        //          - tick_low.eff_sqrtprice_left
        //   c = - (amount_left / amount_right) / (1 - fee_rate)
        // ```
        // For the solution w.r.t eff_sqrtprice_right the terms are:
        // ```
        //   a = 1
        //   b = amount_right / amount_left * tick_low.eff_sqrtprice_left
        //          - tick_high.eff_sqrtprice_right
        //   c = - (amount_right / amount_left) / (1 - fee_rate)
        // ```
        // In both cases the only positive solution is:
        // ```
        //     eff_sqrtprice_(left|right) = [sqrt(b*b - 4*a*c) - b] / 2a
        // ```
        // The solution is more accurate when b term is negative. So we prefer the solution
        // w.r.t. eff_sqrtprice_left if amount_left * eff_sqrtprice_low_right <= amount_right * eff_sqrtprice_low_left
        // and the solution w.r.t. eff_sqrtprice_right otherwise.

        let eff_sqrtprice_low_left = tick_low.eff_sqrtprice(fee_level, Side::Left);
        let eff_sqrtprice_low_right = tick_high.eff_sqrtprice(fee_level, Side::Right);

        let is_eval_left =
            amount_left * eff_sqrtprice_low_right <= amount_right * eff_sqrtprice_low_left;

        let amount_ratio = if is_eval_left {
            amount_left / amount_right
        } else {
            amount_right / amount_left
        };

        // Due to numberic errors, minus_b_term can turn negative.
        // Workaround: calculate minus_b_term in SFP, and then clamp to non-negative.
        let minus_b_term = if is_eval_left {
            let eff_sqrtprice_low_left = LongestSFP::try_from(eff_sqrtprice_low_left)
                .map_err(|_| error_here!(ErrorKind::InternalLogicError))?;
            let amount_ratio_times_eff_sqrtprice_low_right =
                match LongestSFP::try_from(amount_ratio * eff_sqrtprice_low_right) {
                    Ok(value) => value,
                    Err(fp::Error::PrecisionLoss) => LongestSFP::zero(),
                    Err(e) => Err(error_here!(e))?,
                };
            eff_sqrtprice_low_left - amount_ratio_times_eff_sqrtprice_low_right
        } else {
            let eff_sqrtprice_low_right = LongestSFP::try_from(eff_sqrtprice_low_right)
                .map_err(|_| error_here!(ErrorKind::InternalLogicError))?;
            let amount_ratio_times_eff_sqrtprice_low_left =
                match LongestSFP::try_from(amount_ratio * eff_sqrtprice_low_left) {
                    Ok(value) => value,
                    Err(fp::Error::PrecisionLoss) => LongestSFP::zero(),
                    Err(e) => Err(error_here!(e))?,
                };

            eff_sqrtprice_low_right - amount_ratio_times_eff_sqrtprice_low_left
        };
        let minus_b_term = if minus_b_term.non_negative {
            minus_b_term.value
        } else {
            LongestUFP::zero()
        };

        let one_over_one_minus_fee_rate = if is_eval_left {
            eff_sqrtprice_low_left * tick_low.eff_sqrtprice(fee_level, Side::Right)
        } else {
            eff_sqrtprice_low_right * tick_high.eff_sqrtprice(fee_level, Side::Left)
        };

        // The conversion must never fail because LongestUFP has many more bits than Amount.
        let minus_four_a_c =
            LongestUFP::try_from(Float::from(4) * amount_ratio * one_over_one_minus_fee_rate)
                .map_err(|_| error_here!(ErrorKind::InternalLogicError))?;

        let discriminant = minus_b_term * minus_b_term + minus_four_a_c;
        let eff_sqrtprice = next_up(Float::from(discriminant.integer_sqrt() + minus_b_term))
            * Float::from(2).recip();

        if is_eval_left {
            ensure_here!(
                eff_sqrtprice >= tick_low.eff_sqrtprice(fee_level, Side::Left),
                ErrorKind::InternalLogicError
            );
            ensure_here!(
                eff_sqrtprice <= tick_high.eff_sqrtprice(fee_level, Side::Left),
                ErrorKind::InternalLogicError
            );
            Ok((eff_sqrtprice, Side::Left))
        } else {
            ensure_here!(
                eff_sqrtprice >= tick_high.eff_sqrtprice(fee_level, Side::Right),
                ErrorKind::InternalLogicError
            );
            ensure_here!(
                eff_sqrtprice <= tick_low.eff_sqrtprice(fee_level, Side::Right),
                ErrorKind::InternalLogicError
            );

            Ok((eff_sqrtprice, Side::Right))
        }
    } else if amount_left > Float::zero() {
        // The position consists of left token only.
        // We set spot price to the upper bound of position range.

        // Protection against occasional trader's error: it is unlikely
        // that trader wants to create a position setting spot price to TICK::MAX
        // Therefore we return an error. If the trader intends to create a postion
        // at price close to Tick::MAX price, he may explicitly specify e.g. Tick::MAX-1
        // as the upper position range bound.
        ensure_here!(tick_high < Tick::MAX, ErrorKind::Slippage);

        Ok((tick_high.eff_sqrtprice(fee_level, Side::Right), Side::Right))
    } else if amount_right > Float::zero() {
        // The position consists of right token only.
        // We set spot price to the lower bound of position range.

        // Protection against occasional trader's error: it is unlikely
        // that trader wants to create a position setting spot price to TICK::MIN
        // Therefore we return an error. If the trader intends to create a postion
        // at price close to Tick::MIN price, he may explicitly specify e.g. Tick::MIN+1
        // as the lower position range bound.
        ensure_here!(tick_low > Tick::MIN, ErrorKind::Slippage);

        Ok((tick_low.eff_sqrtprice(fee_level, Side::Left), Side::Left))
    } else {
        // Both amounts are zero
        Err(error_here!(ErrorKind::InvalidParams))
    }
}

/// Evaluate new effective sqrtprice, to which the price needs to be shifted
/// in order to complete an exact-in swap with given `amount_in`,
/// assuming constant active liquidity (i.e. no tick crossings, and no level activations).
/// Returns `Float::MAX` if liquidity is insufficient (it is insufficient only when it is zero).
pub fn eval_required_new_eff_sqrtprice_exact_in(
    current_eff_sqrtprice: Float,
    amount_in: Float,
    sum_gross_liquidities: Float,
) -> Float {
    if sum_gross_liquidities.is_zero() {
        return Float::MAX;
    }

    let eff_sqrtprice_shift = amount_in / sum_gross_liquidities;

    if current_eff_sqrtprice > eff_sqrtprice_shift {
        next_down(current_eff_sqrtprice) + eff_sqrtprice_shift
    } else {
        current_eff_sqrtprice + next_down(eff_sqrtprice_shift)
    }
    .max(current_eff_sqrtprice)
}

/// Evaluate new effective sqrtprice, to which the price needs to be shifted
/// in order to complete an exact-out swap with given `amount_out`,
/// assuming constant active liquidity (i.e. no tick crossings, and no level activations).
/// Returns `Float::MAX` if liquidity is insufficient.
pub fn eval_required_new_eff_sqrtprice_exact_out(
    eff_sqrtprice: Float,
    amount_out: Float,
    sum_gross_liquidities: Float,
) -> Result<Float> {
    if sum_gross_liquidities.is_zero() {
        return Ok(Float::MAX);
    }

    let inverse_eff_sqrtprice = eff_sqrtprice.recip();
    let required_inverse_eff_sqrtprice_shift = amount_out / sum_gross_liquidities;

    // Required shift of inverse_eff_sqrtprice may exceed its current value.
    // This corresponds to the case when current active liquidity would not
    // be sufficient to fulfill the swap, even if price is shifted to infinity.
    if required_inverse_eff_sqrtprice_shift >= next_down(inverse_eff_sqrtprice) {
        // Swap amount exceeds available liquidity on the current set of active levels
        // (assuming the same liquidity up to infinite price)
        return Ok(Float::MAX);
    }

    // There is enough active liquidity, assuming liquidity would remain
    // the same towards infinite price.

    // Lower bits of `required_inverse_eff_sqrtprice_shift` will be lost in subtraction,
    // because `required_inverse_eff_sqrtprice_shift` < `inverse_eff_sqrtprice`.
    // We need to subtract _at_least_ `required_inverse_eff_sqrtprice_shift` (as trader
    // must pay for _at_least_ `amount` tokens), therefore we do next_down:
    let new_inverse_eff_sqrtprice =
        next_down(inverse_eff_sqrtprice) - required_inverse_eff_sqrtprice_shift;
    // As `required_inverse_eff_sqrtprice_shift` is strictly less than `next_down(inverse_eff_sqrtprice)`,
    // the minimal difference equals to the significance of the lowest bit of `next_down(inverse_eff_sqrtprice)`,
    // which should still be normal:
    ensure_here!(
        new_inverse_eff_sqrtprice.is_normal(),
        ErrorKind::InternalLogicError
    );

    // Cross-check that the price shift is sufficient to swap the required amount:
    ensure_here!(
        (eff_sqrtprice.recip() - new_inverse_eff_sqrtprice) * sum_gross_liquidities >= amount_out,
        ErrorKind::InternalLogicError
    );

    // Invert the price back with rounding up:
    let new_eff_sqrtprice = next_up(new_inverse_eff_sqrtprice.recip());

    // Ensure that the price did change, at least by the LSB:
    let new_eff_sqrtprice = new_eff_sqrtprice.max(next_up(eff_sqrtprice));

    // Cross-check that the price changed in both directions
    ensure_here!(
        new_eff_sqrtprice > eff_sqrtprice,
        ErrorKind::InternalLogicError
    );

    Ok(new_eff_sqrtprice)
}

/// Evaluate effective sqrtprice from spot sqrtprice
pub fn eff_sqrtprice_from_spot_sqrtprice(spot_sqrtprice: Float, fee_level: FeeLevel) -> Float {
    spot_sqrtprice * one_over_sqrt_one_minus_fee_rate(fee_level)
}

/// Gross liquidity is a factor connecting the total amount paid by a trader in a swap,
/// and the effective sqrtprice shift.
/// `
///     gross_liquidity = liquidity / sqrt(1 - fee_rate)
/// `
pub(crate) fn gross_liquidity_from_net_liquidity(
    net_liqudity: NetLiquidityUFP,
    fee_level: FeeLevel,
) -> GrossLiquidityUFP {
    // The conversion shall not fail, as long as `fee_level` is within the range.
    // See `conversion_one_over_one_minus_fee_rate_to_gross_liquidity_ufp_never_fails_and_within_64_fract_bits`
    let one_over_one_minus_fee_rate =
        GrossLiquidityUFP::try_from(one_over_one_minus_fee_rate(fee_level)).unwrap();
    GrossLiquidityUFP::from(net_liqudity) * one_over_one_minus_fee_rate
}

#[cfg(test)]
#[test]
fn conversion_one_over_one_minus_fee_rate_to_gross_liquidity_ufp_never_fails_and_within_64_fract_bits(
) {
    use super::*;
    for fee_level in 0..NUM_FEE_LEVELS {
        let one_over_one_minus_fee_rate =
            GrossLiquidityUFP::try_from(one_over_one_minus_fee_rate(fee_level)).unwrap();
        // Check that the value fits into 64 fractional bits.
        // As GrossLiquidityUFP has different size on different blockchains, we can't
        // index into underlaying array (as the indices vary).
        // Instead we check the remainder of division by 2^-64:
        assert_eq!(
            one_over_one_minus_fee_rate.0 % (GrossLiquidityUFP::one().0 >> 64),
            GrossLiquidityUFP::zero().0
        );
    }
}

/// Fee liquidity is a factor connecting LP fee and effective sqrtprice shift
/// `
///     fee_liquidity =
///         = liquidity * fee_rate / sqrt(1-fee_rate) =
///         = net_liquidity * fee_rate / (1-fee_rate) =
///         = net_liquidity * [1/(1-fee_rate) - 1]
/// `
pub(crate) fn fee_liquidity_from_net_liquidity(
    net_liqudity: NetLiquidityUFP,
    fee_level: FeeLevel,
) -> FeeLiquidityUFP {
    let fee_rate_over_one_minus_fee_rate = one_over_one_minus_fee_rate(fee_level) - Float::one();
    // The conversion shall not fail, as long as `fee_level` is within the range.
    // See `conversion_one_minus_one_over_one_minus_fee_rate_to_fee_liquidity_ufp_never_fails_and_within_64_fract_bits`
    let fee_rate_over_one_minus_fee_rate =
        FeeLiquidityUFP::try_from(fee_rate_over_one_minus_fee_rate).unwrap();

    FeeLiquidityUFP::from(net_liqudity) * fee_rate_over_one_minus_fee_rate
}

#[cfg(test)]
#[test]
fn conversion_one_minus_one_over_one_minus_fee_rate_to_fee_liquidity_ufp_never_fails_and_within_64_fract_bits(
) {
    use super::*;
    for fee_level in 0..NUM_FEE_LEVELS {
        let fee_rate_over_one_minus_fee_rate =
            FeeLiquidityUFP::try_from(one_over_one_minus_fee_rate(fee_level) - Float::one())
                .unwrap();
        // Check that the value fits into 64 fractional bits.
        // As FeeLiquidityUFP has different size on different blockchains, we can't
        // index into underlaying array (as the indices vary).
        // Instead we check the remainder of division by 2^-64:
        assert_eq!(
            fee_rate_over_one_minus_fee_rate.0 % (FeeLiquidityUFP::one().0 >> 64),
            FeeLiquidityUFP::zero().0
        );
    }
}
