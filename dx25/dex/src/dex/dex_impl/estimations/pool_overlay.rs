use std::{collections::HashMap, ops::Neg};

use array_init::array_init;

use crate::{
    dex::{
        self,
        pool::{as_fee_level, one_over_sqrt_one_minus_fee_rate, PoolState},
        traits,
        v0::{EffSqrtprices, RawFeeLevelsArray},
        EffTick, ErrorKind, FeeLevel, PoolV0, Position, PositionId, Result, Side, Tick, TickState,
    },
    ensure_here, error_here, Amount, AmountUFP, Float, LPFeePerFeeLiquidity, Liquidity,
    NetLiquiditySFP, NetLiquidityUFP,
};

use num_traits::{CheckedAdd, CheckedSub, Zero};
use traits::{Map, MapRemoveKey, OrderedMap};

use super::overlay_map::OrderedOverlayMap;

pub struct PoolStateOverlay<'a, T: traits::Types>
where
    <T::PoolPositionsMap as Map>::Value: Clone,
    <T::TickStatesMap as Map>::Value: Clone,
{
    /// Liquidity positions of this pool
    pub positions: OrderedOverlayMap<'a, T::PoolPositionsMap>,
    // /// Tick states per fee level
    pub tick_states: RawFeeLevelsArray<OrderedOverlayMap<'a, T::TickStatesMap>>,
    /// Tick states per fee level
    // pub tick_states: v0::FeeLevelsArray<TickStatesMap<T>>,
    /// Total amounts of tokens, including the positions and collected fees (LP and protocol)
    pub total_reserves: (Amount, Amount),
    /// Amounts of tokens locked in positions.
    pub position_reserves: RawFeeLevelsArray<(AmountUFP, AmountUFP)>,
    /// Total amount of LP fee reward to be paid out to all LPs (in case all pasitions are closed)
    pub acc_lp_fee: (AmountUFP, AmountUFP),
    /// Global sqrtprice shift accumulators per top-active-level and for each swap direction.
    /// These are sums of price shifts, performed in swaps with top active level equal to
    /// the index of the array. Hence, to get the total price shift on level `k`
    /// one has to sum up the values from index k to NUM_FEE_LEVELS.
    pub acc_lp_fees_per_fee_liquidity:
        RawFeeLevelsArray<(LPFeePerFeeLiquidity, LPFeePerFeeLiquidity)>,
    /// Effective price on each of the levels
    pub eff_sqrtprices: RawFeeLevelsArray<EffSqrtprices>,
    /// next active ticks for swaps in left direction
    pub next_active_ticks_left: RawFeeLevelsArray<Option<Tick>>,
    /// next active ticks for swaps in right direction
    pub next_active_ticks_right: RawFeeLevelsArray<Option<Tick>>,
    /// Current effective net liquidity. Equal to: liquidity * sqrt(1-fee_rate)
    pub net_liquidities: RawFeeLevelsArray<Liquidity>,
    /// Current top active level
    pub top_active_level: FeeLevel,
    pub active_side: Side,
    /// A tick which spot price is sufficiently close (less than 1 tick away) to the
    /// current effective sqrtprice in the active direction. It is used to evaluate the
    /// effective sqrtprice in the opposite direction.
    /// See `eff_sqrtprice_opposite_side` for details.
    pub pivot: EffTick,
}

impl<'a, T: traits::Types> Default for PoolStateOverlay<'a, T> {
    fn default() -> Self {
        Self {
            positions: OrderedOverlayMap::default(),
            tick_states: RawFeeLevelsArray::default(),
            total_reserves: (Amount::default(), Amount::default()),
            position_reserves: RawFeeLevelsArray::default(),
            acc_lp_fee: (AmountUFP::default(), AmountUFP::default()),
            acc_lp_fees_per_fee_liquidity: RawFeeLevelsArray::default(),
            eff_sqrtprices: RawFeeLevelsArray::default(),
            next_active_ticks_left: RawFeeLevelsArray::default(),
            next_active_ticks_right: RawFeeLevelsArray::default(),
            net_liquidities: RawFeeLevelsArray::default(),
            top_active_level: FeeLevel::default(),
            active_side: Side::default(),
            pivot: EffTick::default(),
        }
    }
}

impl<'a, T: traits::Types> From<&'a PoolV0<T>> for PoolStateOverlay<'a, T> {
    fn from(pool: &'a PoolV0<T>) -> Self {
        let positions: &T::PoolPositionsMap = &pool.positions;
        let tick_states_refs: RawFeeLevelsArray<&T::TickStatesMap> = [
            &pool.tick_states[0],
            &pool.tick_states[1],
            &pool.tick_states[2],
            &pool.tick_states[3],
            &pool.tick_states[4],
            &pool.tick_states[5],
            &pool.tick_states[6],
            &pool.tick_states[7],
        ];

        Self {
            positions: OrderedOverlayMap::new(positions),
            tick_states: tick_states_refs.map(OrderedOverlayMap::new),
            acc_lp_fee: pool.acc_lp_fee,
            total_reserves: pool.total_reserves,
            acc_lp_fees_per_fee_liquidity: pool.acc_lp_fees_per_fee_liquidity.into(),
            active_side: pool.active_side,
            eff_sqrtprices: pool.eff_sqrtprices.into(),
            net_liquidities: pool.net_liquidities.into(),
            next_active_ticks_left: pool.next_active_ticks_left.into(),
            next_active_ticks_right: pool.next_active_ticks_right.into(),
            pivot: pool.pivot,
            position_reserves: pool.position_reserves.into(),
            top_active_level: pool.top_active_level,
        }
    }
}

impl<'a, T: traits::Types> PoolStateOverlay<'a, T> {
    fn spot_sqrtprice(&self, side: Side, level: FeeLevel) -> Float {
        self.eff_sqrtprice(level, side) / one_over_sqrt_one_minus_fee_rate(level)
    }

    pub fn spot_sqrtprices(&self, side: Side) -> RawFeeLevelsArray<Float> {
        array_init(|level| self.spot_sqrtprice(side, as_fee_level(level)))
    }
}

impl<'a, T: traits::Types> PoolState<T> for PoolStateOverlay<'a, T> {
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
        self.top_active_level
    }

    fn eff_sqrtprice(&self, level: FeeLevel, side: Side) -> Float {
        self.eff_sqrtprices[level as usize].value(side)
    }

    fn eff_sqrtprices_at(&self, level: FeeLevel) -> EffSqrtprices {
        self.eff_sqrtprices[level as usize]
    }

    fn eff_sqrtprices(&self) -> RawFeeLevelsArray<EffSqrtprices> {
        self.eff_sqrtprices
    }

    fn reset_eff_sqrtprices(&mut self) {
        self.eff_sqrtprices = RawFeeLevelsArray::default();
    }

    fn set_eff_sqrtprices_at(&mut self, level: FeeLevel, eff_sqrtprices: EffSqrtprices) {
        self.eff_sqrtprices[level as usize] = eff_sqrtprices;
    }

    fn net_liquidity_at(&self, level: FeeLevel) -> NetLiquidityUFP {
        self.net_liquidities[level as usize]
    }

    fn inc_net_liquidity_at(&mut self, level: FeeLevel, net_liquidity_increment: NetLiquidityUFP) {
        self.net_liquidities[level as usize] += net_liquidity_increment;
    }

    fn dec_net_liquidity_at(&mut self, level: FeeLevel, net_liquidity_decrement: NetLiquidityUFP) {
        self.net_liquidities[level as usize] -= net_liquidity_decrement;
    }

    fn pivot(&self) -> EffTick {
        self.pivot
    }

    fn set_pivot(&mut self, pivot: EffTick) {
        self.pivot = pivot;
    }

    fn position_reserves(&self) -> RawFeeLevelsArray<(AmountUFP, AmountUFP)> {
        self.position_reserves
    }

    fn position_reserves_at(&self, level: FeeLevel) -> (AmountUFP, AmountUFP) {
        self.position_reserves[level as usize]
    }

    fn inc_position_reserve_at(
        &mut self,
        level: FeeLevel,
        side: Side,
        increment: AmountUFP,
    ) -> Result<(), ()> {
        let position_reserve = &mut self.position_reserves[level as usize][side];
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
        let position_reserve = &mut self.position_reserves[level as usize][side];
        match position_reserve.checked_sub(&decrement) {
            Some(new_position_reserve) => {
                *position_reserve = new_position_reserve;
                Ok(())
            }
            None => Err(()),
        }
    }

    fn next_active_tick(&self, level: FeeLevel, side: Side) -> Option<Tick> {
        match side {
            Side::Left => self.next_active_ticks_left[level as usize],
            Side::Right => self.next_active_ticks_right[level as usize],
        }
    }
    fn set_next_active_tick(&mut self, level: FeeLevel, side: Side, tick: Option<Tick>) {
        match side {
            Side::Left => self.next_active_ticks_left[level as usize] = tick,
            Side::Right => self.next_active_ticks_right[level as usize] = tick,
        }
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
        self.acc_lp_fees_per_fee_liquidity[level as usize][side]
    }

    fn acc_lp_fees_per_fee_liquidity_at(
        &self,
        level: FeeLevel,
    ) -> (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity) {
        self.acc_lp_fees_per_fee_liquidity[level as usize]
    }

    fn inc_acc_lp_fee_per_fee_liquidity(
        &mut self,
        side: Side,
        top_active_level: FeeLevel,
        lp_fee_per_fee_liquidity: LPFeePerFeeLiquidity,
    ) {
        self.acc_lp_fees_per_fee_liquidity[top_active_level as usize][side] +=
            lp_fee_per_fee_liquidity;
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
            Side::Left => self.tick_states[fee_level as usize]
                .inspect_above(&begin_excluding, |tick: &Tick, _| *tick),
            Side::Right => self.tick_states[fee_level as usize]
                .inspect_below(&begin_excluding, |tick: &Tick, _| *tick),
        }
    }

    /// `level` must be valid
    /// Returns 0 if tick is not active
    fn get_tick_acc_lp_fees_per_fee_liquidity(
        &self,
        level: FeeLevel,
        tick: Tick,
    ) -> (LPFeePerFeeLiquidity, LPFeePerFeeLiquidity) {
        self.tick_states[level as usize]
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
        let net_liquidity_change = self.tick_states[level as usize]
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
            self.net_liquidities[level as usize] += net_liquidity_change.value;
        } else {
            self.net_liquidities[level as usize] -= net_liquidity_change.value;
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
        let existing_tick_state =
            self.tick_states[level as usize].inspect(&tick, |tick_state| tick_state.clone());
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
        self.tick_states[level as usize].insert(tick, tick_state);
        Ok(new_net_liquidity_change)
    }

    fn tick_remove_liquidity(
        &mut self,
        level: FeeLevel,
        tick: Tick,
        net_liquidity_change_decrement: NetLiquiditySFP,
    ) -> Result<NetLiquiditySFP> {
        let mut tick_state = self.tick_states[level as usize]
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
            self.tick_states[level as usize].insert(tick, tick_state);
        } else {
            ensure_here!(
                new_net_liquidity_change.is_zero(),
                ErrorKind::InternalLogicError
            );

            self.tick_states[level as usize].remove(&tick);

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
