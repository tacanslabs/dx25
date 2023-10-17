#![allow(unused)]
use crate::dex::traits::MapRemoveKey;
use crate::dex::traits::OrderedMap as _;
use crate::dex::v0::FeeLevelsArray;
use crate::dex::v0::NUM_FEE_LEVELS;
use crate::dex::Position;
use crate::dex::PositionId;
use crate::dex::TickState;
use crate::dex::TickStateV0;
use crate::dex::{ErrorKind, PoolV0, Tick};
use crate::ensure_here;
use crate::NetLiquiditySFP;
use crate::{
    dex, error_here, AmountSFP, AmountUFP, FeeLiquidityUFP, GrossLiquidityUFP,
    LPFeePerFeeLiquidity, Liquidity, NetLiquidityUFP,
};
use dex::utils::{next_down, next_up};
use dex::v0::{EffSqrtprices, RawFeeLevelsArray};
use dex::{traits, Amount, BasisPoints, EffTick, FeeLevel, Float, Result, Side, SwapKind, Types};
use num_traits::Zero;
use num_traits::{CheckedAdd, CheckedSub};
use std::ops::Neg;
use traits::Map;

pub(crate) trait PoolState<T: traits::Types> {
    /// Active swap direction. It is the direction in which the last swap was performed, or current swap is being performed.
    fn active_side(&self) -> Side;

    /// Set active swap direction.
    fn set_active_side(&mut self, side: Side);

    /// Topmost active fee level.
    fn top_active_level(&self) -> FeeLevel;

    /// Set top active level to zero.
    fn reset_top_active_level(&mut self);

    /// Increment top active level by one.
    /// Requires: current top active level is not the last one.
    fn inc_top_active_level(&mut self);

    /// Effective Tick that is sufficiently close to the current effective price.
    /// Pivot is used to evaluate the opposite effective sqrtprice.
    fn pivot(&self) -> EffTick;

    /// Set pivot tick.
    fn set_pivot(&mut self, pivot: EffTick);

    /// Sqrt of effective price at the given fee level, in the given swap direction.
    fn eff_sqrtprice(&self, level: FeeLevel, side: Side) -> Float;

    /// Sqrt-s of effective prices at the given fee level
    fn eff_sqrtprices_at(&self, level: FeeLevel) -> EffSqrtprices;

    /// Sqrt-s of effective prices per fee level
    fn eff_sqrtprices(&self) -> RawFeeLevelsArray<EffSqrtprices>;

    fn set_eff_sqrtprices_at(&mut self, level: FeeLevel, eff_sqrtprices: EffSqrtprices);

    fn reset_eff_sqrtprices(&mut self);

    /// Total amount of tokens in the pool, including positions and collected fees.
    fn total_reserves(&self) -> (Amount, Amount);

    /// Increase the total reserve of left or right token by `increment`
    /// Returns OK(()) on success, or Err(()) on overflow.
    fn inc_total_reserve(&mut self, side: Side, increment: Amount) -> Result<(), ()>;

    /// Decrease total reserve of left or right token by `decrement`
    /// Returns OK(()) on success, or Err(()) on underflow.
    fn dec_total_reserve(&mut self, side: Side, decrement: Amount) -> Result<(), ()>;

    /// Increase the total reserves by `increment`.
    /// Returns OK(()) on success, or Err(()) on overflow.
    fn inc_total_reserves(&mut self, increment: (Amount, Amount)) -> Result<(), ()> {
        self.inc_total_reserve(Side::Left, increment.0)?;
        self.inc_total_reserve(Side::Right, increment.1)?;
        Ok(())
    }

    /// Decrease total reserves by `decrement`.
    /// Returns OK(()) on success, or Err(()) on underflow.
    fn dec_total_reserves(&mut self, decrement: (Amount, Amount)) -> Result<(), ()> {
        self.dec_total_reserve(Side::Left, decrement.0)?;
        self.dec_total_reserve(Side::Right, decrement.1)?;
        Ok(())
    }

    /// Total amount of tokens locked in positions, per fee level.
    fn position_reserves(&self) -> RawFeeLevelsArray<(AmountUFP, AmountUFP)>;

    /// Total amount of tokens locked in positions at the given fee level.
    fn position_reserves_at(&self, level: FeeLevel) -> (AmountUFP, AmountUFP);

    /// Increase position reserves of left or right (`side`) token on `level` by `increment`.
    /// Returns OK(()) on success, or Err(()) on overflow.
    fn inc_position_reserve_at(
        &mut self,
        level: FeeLevel,
        side: Side,
        increment: AmountUFP,
    ) -> Result<(), ()>;

    /// Decrease position reserves of left or right (`side`) token on `level` by `decrement`.
    /// Returns OK(()) on success, or Err(()) on underflow.
    fn dec_position_reserve_at(
        &mut self,
        level: FeeLevel,
        side: Side,
        decrement: AmountUFP,
    ) -> Result<(), ()>;

    fn net_liquidity_at(&self, level: FeeLevel) -> NetLiquidityUFP;

    fn inc_net_liquidity_at(&mut self, level: FeeLevel, net_liquidity_increment: NetLiquidityUFP);

    fn dec_net_liquidity_at(&mut self, level: FeeLevel, net_liquidity_decrement: NetLiquidityUFP);

    fn next_active_tick(&self, level: FeeLevel, side: Side) -> Option<Tick>;

    fn set_next_active_tick(&mut self, level: FeeLevel, side: Side, tick: Option<Tick>);

    fn acc_lp_fees(&self) -> (AmountUFP, AmountUFP);

    fn acc_lp_fee(&self, side: Side) -> AmountUFP;

    fn inc_acc_lp_fee(&mut self, side: Side, amount: AmountUFP);
    fn dec_acc_lp_fee(&mut self, side: Side, amount: AmountUFP);

    fn acc_lp_fee_per_fee_liquidity_at(&self, level: FeeLevel, side: Side) -> LPFeePerFeeLiquidity;

    fn acc_lp_fees_per_fee_liquidity_at(
        &self,
        level: FeeLevel,
    ) -> (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity);

    fn inc_acc_lp_fee_per_fee_liquidity(
        &mut self,
        side: Side,
        top_active_level: FeeLevel,
        lp_fee_per_fee_liquidity: LPFeePerFeeLiquidity,
    );

    /// Global accumulated LP fees (both sides) per net liquidity, since the very beginning of dex operation.
    fn acc_lp_fees_per_fee_liquidity(
        &self,
        fee_level: FeeLevel,
    ) -> (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity) {
        let mut acc_lp_fees_per_fee_liquidity =
            (LPFeePerFeeLiquidity::zero(), LPFeePerFeeLiquidity::zero());
        for level in fee_level..NUM_FEE_LEVELS {
            acc_lp_fees_per_fee_liquidity.0 +=
                self.acc_lp_fee_per_fee_liquidity_at(level, Side::Left);
            acc_lp_fees_per_fee_liquidity.1 +=
                self.acc_lp_fee_per_fee_liquidity_at(level, Side::Right);
        }
        acc_lp_fees_per_fee_liquidity
    }

    /// Reliable check if pool is not empty. Relies on that when last position is closed,
    /// no active ticks remain.
    fn contains_any_positions(&self) -> bool;

    fn find_next_active_tick_on_level(
        &self,
        begin_excluding: Tick,
        fee_level: FeeLevel,
        side: Side,
    ) -> Option<Tick>;

    /// Returns 0 if tick is not active
    fn get_tick_acc_lp_fees_per_fee_liquidity(
        &self,
        level: FeeLevel,
        tick: Tick,
    ) -> (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity);

    fn flip_tick_acc_lp_fees_per_fee_liquidity_and_update_net_liquidity(
        &mut self,
        level: FeeLevel,
        tick: Tick,
        side: Side,
    ) -> Result<()>;

    fn flip_tick_acc_lp_fees_per_fee_liquidity(
        tick_state: &mut TickStateV0<T>,
        global_acc_lp_fees_per_fee_liquidity: (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity),
    ) -> Result<NetLiquiditySFP> {
        tick_state.acc_lp_fees_per_fee_liquidity_outside.0 = global_acc_lp_fees_per_fee_liquidity.0
            - tick_state.acc_lp_fees_per_fee_liquidity_outside.0;
        tick_state.acc_lp_fees_per_fee_liquidity_outside.1 = global_acc_lp_fees_per_fee_liquidity.1
            - tick_state.acc_lp_fees_per_fee_liquidity_outside.1;

        Ok(tick_state.net_liquidity_change)
    }

    fn tick_add_liquidity(
        &mut self,
        factory: &mut dyn dex::ItemFactory<T>,
        level: FeeLevel,
        tick: Tick,
        net_liquidity_change: NetLiquiditySFP,
    ) -> Result<NetLiquiditySFP>;

    fn tick_remove_liquidity(
        &mut self,
        level: FeeLevel,
        tick: Tick,
        net_liquidity_change: NetLiquiditySFP,
    ) -> Result<NetLiquiditySFP>;

    fn get_position(&self, position_id: PositionId) -> Option<Position<T>>;

    fn insert_position(&mut self, position_id: PositionId, position: Position<T>);

    fn remove_position(&mut self, position_id: PositionId);
}

impl<T: Types> PoolState<T> for crate::dex::PoolV0<T> {
    fn active_side(&self) -> Side {
        self.active_side
    }

    fn set_active_side(&mut self, side: Side) {
        self.active_side = side;
    }

    fn reset_top_active_level(&mut self) {
        self.top_active_level = 0;
    }

    fn inc_top_active_level(&mut self) {
        self.top_active_level += 1;
    }

    fn top_active_level(&self) -> FeeLevel {
        if self.top_active_level >= NUM_FEE_LEVELS {
            unsafe { std::hint::unreachable_unchecked() }
        }
        self.top_active_level
    }

    fn eff_sqrtprice(&self, level: FeeLevel, side: Side) -> Float {
        self.eff_sqrtprices[level].value(side)
    }

    fn eff_sqrtprices_at(&self, level: FeeLevel) -> EffSqrtprices {
        self.eff_sqrtprices[level]
    }

    fn eff_sqrtprices(&self) -> RawFeeLevelsArray<EffSqrtprices> {
        self.eff_sqrtprices.into()
    }

    fn set_eff_sqrtprices_at(&mut self, level: FeeLevel, eff_sqrtprices: EffSqrtprices) {
        self.eff_sqrtprices[level] = eff_sqrtprices;
    }

    fn reset_eff_sqrtprices(&mut self) {
        self.eff_sqrtprices = FeeLevelsArray::default();
    }

    fn pivot(&self) -> EffTick {
        self.pivot
    }

    fn set_pivot(&mut self, pivot: EffTick) {
        self.pivot = pivot;
    }

    fn total_reserves(&self) -> (Amount, Amount) {
        self.total_reserves
    }

    fn inc_total_reserve(&mut self, side: Side, increment: Amount) -> Result<(), ()> {
        let total_reserve = &mut self.total_reserves[side];
        match total_reserve.checked_add(increment) {
            Some(new_total_reserve) => {
                *total_reserve = new_total_reserve;
                Ok(())
            }
            None => Err(()),
        }
    }

    fn dec_total_reserve(&mut self, side: Side, decrement: Amount) -> Result<(), ()> {
        let total_reserve = &mut self.total_reserves[side];
        match total_reserve.checked_sub(decrement) {
            Some(new_total_reserve) => {
                *total_reserve = new_total_reserve;
                Ok(())
            }
            None => Err(()),
        }
    }

    fn position_reserves(&self) -> RawFeeLevelsArray<(AmountUFP, AmountUFP)> {
        self.position_reserves.into()
    }

    fn position_reserves_at(&self, level: FeeLevel) -> (AmountUFP, AmountUFP) {
        self.position_reserves[level]
    }

    fn inc_position_reserve_at(
        &mut self,
        level: FeeLevel,
        side: Side,
        increment: AmountUFP,
    ) -> Result<(), ()> {
        let position_reserve = &mut self.position_reserves[level][side];
        match position_reserve.checked_add(&increment) {
            Some(new_position_reserve) => {
                *position_reserve = new_position_reserve;
                Ok(())
            }
            None => Err(()),
        }
    }

    fn dec_position_reserve_at(
        &mut self,
        level: FeeLevel,
        side: Side,
        decrement: AmountUFP,
    ) -> Result<(), ()> {
        let position_reserve = &mut self.position_reserves[level][side];
        match position_reserve.checked_sub(&decrement) {
            Some(new_position_reserve) => {
                *position_reserve = new_position_reserve;
                Ok(())
            }
            None => Err(()),
        }
    }

    fn net_liquidity_at(&self, level: FeeLevel) -> NetLiquidityUFP {
        self.net_liquidities[level]
    }

    fn inc_net_liquidity_at(&mut self, level: FeeLevel, net_liquidity_increment: NetLiquidityUFP) {
        self.net_liquidities[level] += net_liquidity_increment;
    }

    fn dec_net_liquidity_at(&mut self, level: FeeLevel, net_liquidity_decrement: NetLiquidityUFP) {
        self.net_liquidities[level] -= net_liquidity_decrement;
    }

    fn next_active_tick(&self, level: FeeLevel, side: Side) -> Option<Tick> {
        match side {
            Side::Left => self.next_active_ticks_left[level],
            Side::Right => self.next_active_ticks_right[level],
        }
    }

    fn set_next_active_tick(&mut self, level: FeeLevel, side: Side, tick: Option<Tick>) {
        match side {
            Side::Left => self.next_active_ticks_left[level] = tick,
            Side::Right => self.next_active_ticks_right[level] = tick,
        }
    }

    fn acc_lp_fees(&self) -> (AmountUFP, AmountUFP) {
        self.acc_lp_fee
    }
    fn acc_lp_fee(&self, side: Side) -> AmountUFP {
        self.acc_lp_fee[side]
    }

    fn inc_acc_lp_fee(&mut self, side: Side, amount: AmountUFP) {
        self.acc_lp_fee[side] += amount;
    }

    fn dec_acc_lp_fee(&mut self, side: Side, amount: AmountUFP) {
        self.acc_lp_fee[side] -= amount;
    }

    fn acc_lp_fee_per_fee_liquidity_at(&self, level: FeeLevel, side: Side) -> LPFeePerFeeLiquidity {
        self.acc_lp_fees_per_fee_liquidity[level][side]
    }

    fn acc_lp_fees_per_fee_liquidity_at(
        &self,
        level: FeeLevel,
    ) -> (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity) {
        self.acc_lp_fees_per_fee_liquidity[level]
    }

    fn inc_acc_lp_fee_per_fee_liquidity(
        &mut self,
        side: Side,
        top_active_level: FeeLevel,
        lp_fee_per_fee_liquidity: LPFeePerFeeLiquidity,
    ) {
        self.acc_lp_fees_per_fee_liquidity[top_active_level][side] += lp_fee_per_fee_liquidity;
    }

    fn contains_any_positions(&self) -> bool {
        self.tick_states
            .iter()
            .any(|tick_states| tick_states.inspect_min(|_, _| ()).is_some())
    }

    fn find_next_active_tick_on_level(
        &self,
        begin_excluding: Tick,
        fee_level: FeeLevel,
        side: Side,
    ) -> Option<Tick> {
        match side {
            Side::Left => {
                self.tick_states[fee_level].inspect_above(&begin_excluding, |tick: &Tick, _| *tick)
            }
            Side::Right => {
                self.tick_states[fee_level].inspect_below(&begin_excluding, |tick: &Tick, _| *tick)
            }
        }
    }

    /// `level` must be valid
    /// Returns 0 if tick is not active
    fn get_tick_acc_lp_fees_per_fee_liquidity(
        &self,
        level: FeeLevel,
        tick: Tick,
    ) -> (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity) {
        self.tick_states[level]
            .inspect(&tick, |TickState::V0(tick_state)| {
                tick_state.acc_lp_fees_per_fee_liquidity_outside
            })
            .unwrap_or_default()
    }

    fn flip_tick_acc_lp_fees_per_fee_liquidity_and_update_net_liquidity(
        &mut self,
        level: FeeLevel,
        tick: Tick,
        side: Side,
    ) -> Result<()> {
        let global_acc_lp_fees_per_fee_liquidity = self.acc_lp_fees_per_fee_liquidity(level);
        let net_liquidity_change = self.tick_states[level]
            .update(&tick, |TickState::V0(tick_state)| {
                Self::flip_tick_acc_lp_fees_per_fee_liquidity(
                    tick_state,
                    global_acc_lp_fees_per_fee_liquidity,
                )
            })
            .ok_or(error_here!(ErrorKind::InternalTickNotFound))??;

        let net_liquidity_change = match side {
            Side::Left => net_liquidity_change,
            Side::Right => net_liquidity_change.neg(),
        };

        if net_liquidity_change.non_negative {
            self.net_liquidities[level] += net_liquidity_change.value;
        } else {
            self.net_liquidities[level] -= net_liquidity_change.value;
        };
        Ok(())
    }

    fn tick_add_liquidity(
        &mut self,
        factory: &mut dyn dex::ItemFactory<T>,
        level: FeeLevel,
        tick: Tick,
        net_liquidity_change_increment: NetLiquiditySFP,
    ) -> Result<NetLiquiditySFP> {
        let mut existing_tick_state =
            self.tick_states[level].inspect(&tick, |tick_state| tick_state.clone());
        let mut tick_state = if let Some(tick_state) = existing_tick_state {
            tick_state
        } else {
            factory.new_default_tick()?
        };
        let new_net_liquidity_change = match tick_state {
            TickState::V0(ref mut tick_state) => {
                tick_state.net_liquidity_change += net_liquidity_change_increment;
                tick_state.reference_counter += 1;
                tick_state.net_liquidity_change
            }
        };
        self.tick_states[level].insert(tick, tick_state);
        Ok(new_net_liquidity_change)
    }

    fn tick_remove_liquidity(
        &mut self,
        level: FeeLevel,
        tick: Tick,
        net_liquidity_change_decrement: NetLiquiditySFP,
    ) -> Result<NetLiquiditySFP> {
        let mut tick_state = self.tick_states[level]
            .inspect(&tick, |tick_state| tick_state.clone())
            .ok_or(error_here!(ErrorKind::InternalTickNotFound))?;
        let (new_net_liquidity_change, tick_remains_active) = match tick_state {
            TickState::V0(ref mut tick_state) => {
                tick_state.net_liquidity_change -= net_liquidity_change_decrement;
                ensure_here!(
                    tick_state.reference_counter > 0,
                    ErrorKind::InternalLogicError
                );
                tick_state.reference_counter -= 1;
                (
                    tick_state.net_liquidity_change,
                    tick_state.reference_counter > 0,
                )
            }
        };

        if tick_remains_active {
            self.tick_states[level].insert(tick, tick_state);
        } else {
            ensure_here!(
                new_net_liquidity_change.is_zero(),
                ErrorKind::InternalLogicError
            );

            self.tick_states[level].remove(&tick);

            if self.next_active_tick(level, Side::Left) == Some(tick) {
                self.set_next_active_tick(
                    level,
                    Side::Left,
                    self.find_next_active_tick_on_level(tick, level, Side::Left),
                );
            }
            if self.next_active_tick(level, Side::Right) == Some(tick) {
                self.set_next_active_tick(
                    level,
                    Side::Right,
                    self.find_next_active_tick_on_level(tick, level, Side::Right),
                );
            }
        }

        Ok(new_net_liquidity_change)
    }

    fn get_position(&self, position_id: PositionId) -> Option<Position<T>> {
        self.positions
            .inspect(&position_id, |position| position.clone())
    }

    fn insert_position(&mut self, position_id: PositionId, position: Position<T>) {
        self.positions.insert(position_id, position);
    }

    fn remove_position(&mut self, position_id: PositionId) {
        self.positions.remove(&position_id);
    }
}
