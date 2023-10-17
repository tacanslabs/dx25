// Some of the conversions are useless, because NEAR Amount is u128,
// which is not the same for other DEX's
#![allow(clippy::useless_conversion)]

use super::test_utils::{new_account_id, new_amount, new_token_id};
use super::Estimations;
use crate::chain::TokenId;
use crate::dex::pool::{one_over_one_minus_fee_rate, one_over_sqrt_one_minus_fee_rate};
use crate::dex::test_utils::Sandbox;
use crate::dex::tick::Tick;
use crate::dex::utils::swap_if;
use crate::dex::{
    Error, ErrorKind, FeeLevel, PoolInfo, PositionId, PositionInfo, PositionInit, Range, Result,
    Side, SwapKind,
};
use crate::{assert_eq_rel_tol, Amount, Float, Liquidity};
use assert_matches::assert_matches;
use num_traits::Zero;
use rstest::rstest;
use rug::ops::Pow;

struct SwapContext {
    state: Sandbox,
    tokens: (TokenId, TokenId),
}

impl SwapContext {
    fn open_position(
        &mut self,
        fee_level: FeeLevel,
        max_left: Amount,
        max_right: Amount,
        tick_low: Tick,
        tick_high: Tick,
    ) -> Result<(PositionId, Amount, Amount, Liquidity)> {
        let fee_rates = [1, 2, 4, 8, 16, 32, 64, 128];
        let fee_rate = fee_rates[fee_level as usize];

        self.state.call_mut(|dex| {
            dex.open_position(
                &self.tokens.0,
                &self.tokens.1,
                fee_rate,
                PositionInit {
                    amount_ranges: (
                        Range {
                            min: new_amount(0).into(),
                            max: max_left.into(),
                        },
                        Range {
                            min: new_amount(0).into(),
                            max: max_right.into(),
                        },
                    ),
                    ticks_range: (tick_low.to_opt_index(), tick_high.to_opt_index()),
                },
            )
        })
    }

    fn close_position(&mut self, position_id: PositionId) -> Result<()> {
        self.state.call_mut(|dex| dex.close_position(position_id))
    }

    fn get_pool_info(&self) -> Option<PoolInfo> {
        self.state
            .call(|dex| dex.get_pool_info(self.tokens.clone()).unwrap())
    }

    fn swap(&mut self, side: Side, swap_type: SwapKind, amount: Amount) -> Result<Amount> {
        let (token_in, token_out) = swap_if(side == Side::Right, self.tokens.clone());

        self.state.call_mut(|dex| {
            let result = dex.swap(&token_in, &token_out, swap_type, None, amount);

            match swap_type {
                SwapKind::ExactIn => result.map(|r| r.1),
                SwapKind::ExactOut => result.map(|r| r.0),
                SwapKind::ToPrice => unreachable!("Use swap_to_price"),
            }
        })
    }

    fn estimate_swap(&self, side: Side, swap_type: SwapKind, amount: Amount) -> Result<Amount> {
        let (token_in, token_out) = swap_if(side == Side::Right, self.tokens.clone());

        self.state.call(|dex| match swap_type {
            SwapKind::ExactIn => dex
                .estimate_swap_exact(true, token_in, token_out, amount, 10)
                .map(|r| r.result),
            SwapKind::ExactOut => dex
                .estimate_swap_exact(false, token_in, token_out, amount, 10)
                .map(|r| r.result),
            SwapKind::ToPrice => unreachable!("Use swap_to_price"),
        })
    }

    fn swap_to_price(
        &mut self,
        side: Side,
        amount: Amount,
        effective_price_limit: Float,
    ) -> Result<(Amount, Amount)> {
        let (token_in, token_out) = swap_if(side == Side::Right, self.tokens.clone());

        self.state.call_mut(|dex| {
            dex.swap(
                &token_in,
                &token_out,
                SwapKind::ToPrice,
                Some(effective_price_limit),
                amount,
            )
        })
    }

    #[allow(dead_code)]
    fn get_position_info(&self, position_id: PositionId) -> Result<PositionInfo> {
        self.state.call(|dex| dex.get_position_info(position_id))
    }

    fn withdraw_protocol_fee(&mut self) -> Result<(Amount, Amount)> {
        self.state
            .call_mut(|dex| dex.withdraw_protocol_fee(self.tokens.clone()))
    }
}

fn new_swap_context() -> SwapContext {
    let acc = new_account_id();
    #[allow(clippy::clone_on_copy)]
    let mut sandbox = Sandbox::new_default(acc.clone());

    let amounts = (
        new_amount(500_000_000_000_000_000_000_000_u128),
        new_amount(500_000_000_000_000_000_000_000_u128),
    );

    sandbox.call_mut(|dex| dex.register_account()).unwrap();

    let token_0 = new_token_id();
    let token_1 = new_token_id();

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1]))
        .unwrap();

    sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_0, amounts.0))
        .unwrap();
    sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_1, amounts.1))
        .unwrap();

    SwapContext {
        state: sandbox,
        tokens: (token_0, token_1),
    }
}

#[test]
fn swap_exact_in_failure() {
    let SwapContext {
        mut state,
        tokens: (token_0, token_1),
        ..
    } = new_swap_context();

    assert_matches!(
        state.call_mut(|dex| dex.swap_exact_in(&[token_0, token_1], new_amount(1), new_amount(20))),
        Err(_)
    );
}

#[test]
fn swap_exact_out_failure() {
    let SwapContext {
        mut state,
        tokens: (token_0, token_1),
        ..
    } = new_swap_context();

    assert_matches!(
        state.call_mut(|dex| dex.swap_exact_out(
            &[token_0, token_1],
            new_amount(100),
            new_amount(1),
        )),
        Err(_)
    );
}

/// A swap without crossing active ticks
#[test]
fn test_swap_simple() -> Result<()> {
    let mut ctx = new_swap_context();
    let fee_level: FeeLevel = 3;
    let position_amounts = 1_000_000_u128;

    let tick_halfrange: i32 = 10000;

    ctx.open_position(
        fee_level,
        position_amounts.into(),
        position_amounts.into(),
        Tick::new(-tick_halfrange).unwrap(),
        Tick::new(tick_halfrange).unwrap(),
    )?;

    // Position is symmetric => all prices must be equal 1.0
    let expected_spot_price = 1f64;
    let acutal_spot_prices = ctx.get_pool_info().unwrap().spot_sqrtprices;

    for actual_spot_price in &acutal_spot_prices {
        assert_eq_rel_tol!(*actual_spot_price, expected_spot_price, 5);
    }

    let expected_sqrtprice_low_tick = rug::Float::with_val(500, 1.0001)
        .pow(-tick_halfrange / 2)
        .to_f64();
    assert_eq_rel_tol!(
        f64::from(Tick::new(-tick_halfrange).unwrap().spot_sqrtprice()),
        expected_sqrtprice_low_tick,
        10
    );
    let expected_sqrtprice_high_tick = rug::Float::with_val(500, 1.0001)
        .pow(tick_halfrange / 2)
        .to_f64();
    assert_eq_rel_tol!(
        f64::from(Tick::new(tick_halfrange).unwrap().spot_sqrtprice()),
        expected_sqrtprice_high_tick,
        10
    );

    #[allow(clippy::cast_precision_loss)]
    let expected_liquidity = position_amounts as f64 / (1.0 - expected_sqrtprice_low_tick);

    let amount_out = 10_000_u128;

    #[allow(clippy::cast_precision_loss)]
    let expected_in_wo_fee = (amount_out as f64) * expected_liquidity * expected_spot_price
        / (expected_liquidity / expected_spot_price - (amount_out as f64));

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let expected_in: Amount =
        ((expected_in_wo_fee * f64::from(one_over_one_minus_fee_rate(fee_level))).ceil() as u128)
            .into();

    let in_amount = ctx
        .swap(Side::Left, SwapKind::ExactOut, amount_out.into())
        .unwrap();

    assert_eq!(in_amount, expected_in);

    Ok(())
}

/// A swap with crossing active tick
///
/// Position ranges: from `tick_low` to `tick_mid`, and from `tick_mid` to `tick_high`
#[test]
fn test_swap_with_tick_crossing() -> Result<()> {
    let mut ctx = new_swap_context();
    let fee_level: FeeLevel = 3;
    let tick_low = Tick::new(-10i32).unwrap();
    let tick_mid = Tick::new(10i32).unwrap();
    let tick_high = Tick::new(40i32).unwrap();
    let position_amounts = 100_000u128.into();
    let (pos1_id, pos1_amount_left, pos1_amount_right, _pos1_liquidity) = ctx.open_position(
        fee_level,
        position_amounts,
        position_amounts,
        tick_low,
        tick_mid,
    )?;

    // First position in the pool => max left and max right amounts are deposited
    assert_eq!(pos1_amount_left, position_amounts);
    assert_eq!(pos1_amount_right, position_amounts);
    // Position is symmetric, so spot price must be 1.0
    assert_eq_rel_tol!(
        ctx.get_pool_info().unwrap().spot_sqrtprices[fee_level as usize],
        Float::one(),
        5
    );
    let expected_liquidity_pos1 =
        Float::from(position_amounts) / (Float::one() - tick_low.spot_sqrtprice());
    assert_eq_rel_tol!(
        expected_liquidity_pos1,
        ctx.get_pool_info().unwrap().liquidities[fee_level as usize],
        13
    );

    // let expected_net_liquidity_pos1 = expected_liquidity_pos1 / one_over_sqrt_one_minus_fee_rate(fee_level);
    // let actual_net_liquidity_pos1 = pool.net_liquidities[fee_level];

    // assert_eq_rel_tol!(expected_net_liquidity_pos1, actual_net_liquidity_pos1, 12);
    // let actual_gross_liquidity_pos1 = Float::from(pool.gross_liquidities[fee_level]);
    let expected_gross_liquidity_pos1 =
        expected_liquidity_pos1 * one_over_sqrt_one_minus_fee_rate(fee_level);

    // assert_eq_rel_tol!(
    //     expected_gross_liquidity_pos1,
    //     actual_gross_liquidity_pos1,
    //     12
    // );

    // Open second position. Position is single-sided.
    let zero_amount: Amount = 0u128.into();
    let (pos2_id, pos2_amount_left, pos2_amount_right, _pos2_liquidity) = ctx.open_position(
        fee_level,
        zero_amount,
        position_amounts,
        tick_mid,
        tick_high,
    )?;

    assert_eq!(pos2_amount_left, zero_amount);
    assert_eq!(pos2_amount_right, position_amounts);
    let expected_liquidity_pos2 = Float::from(position_amounts)
        / (tick_mid.spot_sqrtprice().recip() - tick_high.spot_sqrtprice().recip());

    // let expected_net_liquidity_pos2 = expected_liquidity_pos2 / one_over_sqrt_one_minus_fee_rate(fee_level);
    let expected_gross_liquidity_pos2 =
        expected_liquidity_pos2 * one_over_sqrt_one_minus_fee_rate(fee_level);

    // let actual_net_liquidity_pos2 = pos2.1;
    // assert_eq_rel_tol!(expected_net_liquidity_pos2, actual_net_liquidity_pos2, 12);
    //
    let amount_out = position_amounts + position_amounts / 4;

    let actual_amount_in = ctx
        .swap(Side::Left, SwapKind::ExactOut, amount_out)
        .unwrap();

    assert_eq_rel_tol!(
        expected_liquidity_pos2,
        ctx.get_pool_info().unwrap().liquidities[fee_level as usize],
        12
    );

    let eff_price_shift_pos1 = tick_mid.eff_sqrtprice(fee_level, Side::Left)
        - Tick::new(0).unwrap().eff_sqrtprice(fee_level, Side::Left);
    let expected_amount_in_pos1 = eff_price_shift_pos1 * expected_gross_liquidity_pos1;
    let expected_amount_out_pos1 = Float::from(position_amounts);

    let remaining_amount_out = Float::from(amount_out) - expected_amount_out_pos1;
    let new_eff_price_pos2 = (tick_mid.eff_sqrtprice(fee_level, Side::Left).recip()
        - remaining_amount_out / expected_gross_liquidity_pos2)
        .recip();
    let eff_price_shift_pos2 = new_eff_price_pos2 - tick_mid.eff_sqrtprice(fee_level, Side::Left);
    let expected_amount_in_pos2 = expected_gross_liquidity_pos2 * eff_price_shift_pos2;

    let expected_amount_in =
        Amount::try_from((expected_amount_in_pos1 + expected_amount_in_pos2).ceil()).unwrap();

    assert_eq!(expected_amount_in, actual_amount_in);

    ctx.close_position(pos1_id)?;
    ctx.close_position(pos2_id)?;
    assert!(ctx
        .get_pool_info()
        .unwrap()
        .spot_sqrtprices
        .iter()
        .all(|spot_sqrtprice| spot_sqrtprice.is_zero()));
    Ok(())
}

#[test]
fn test_estimate_swap_with_tick_crossing() -> Result<()> {
    let mut ctx = new_swap_context();
    let fee_level: FeeLevel = 3;
    let tick_low = Tick::new(-10i32).unwrap();
    let tick_mid = Tick::new(10i32).unwrap();
    let tick_high = Tick::new(40i32).unwrap();
    let position_amounts = 100_000u128.into();
    ctx.open_position(
        fee_level,
        position_amounts,
        position_amounts,
        tick_low,
        tick_mid,
    )?;

    // Open second position. Position is single-sided.
    let zero_amount: Amount = 0u128.into();
    ctx.open_position(
        fee_level,
        zero_amount,
        position_amounts,
        tick_mid,
        tick_high,
    )?;

    let amount_out = position_amounts + position_amounts / 4;

    let estimated_amount_in = ctx
        .estimate_swap(Side::Left, SwapKind::ExactOut, amount_out)
        .unwrap();

    let actual_amount_in = ctx
        .estimate_swap(Side::Left, SwapKind::ExactOut, amount_out)
        .unwrap();

    assert_eq!(estimated_amount_in, actual_amount_in);

    Ok(())
}

fn new_swap_context_in_inactive_region() -> SwapContext {
    let mut ctx = new_swap_context();
    let (pos0_id, _, _, _) = ctx
        .open_position(
            3,
            100_000u128.into(),
            200_000u128.into(),
            Tick::new(20_000).unwrap(),
            Tick::new(21_000).unwrap(),
        )
        .unwrap();
    let (_pos1_id, _, _, _) = ctx
        .open_position(
            3,
            100_000u128.into(),
            0u128.into(),
            Tick::new(19_000).unwrap(),
            Tick::new(20_000).unwrap(),
        )
        .unwrap();
    ctx.close_position(pos0_id).unwrap();
    ctx
}

#[test]
fn test_swap_within_inactive_region_success() {
    let mut ctx = new_swap_context_in_inactive_region();
    let res = ctx.swap(Side::Right, SwapKind::ExactOut, 20_000u128.into());
    assert_matches!(res, Ok(_));
}

#[test]
fn test_swap_within_inactive_region_fail_wrong_direction() {
    let mut ctx = new_swap_context_in_inactive_region();
    let res = ctx.swap(Side::Left, SwapKind::ExactOut, 20_000u128.into());
    assert_matches!(
        res,
        Err(Error {
            kind: ErrorKind::InsufficientLiquidity { .. },
            ..
        })
    );
}

#[test]
fn test_swap_almost_to_position_end_success() {
    let mut ctx = new_swap_context();
    let amount_x = 100_000_u128;
    let amount_y = 200_000_u128;
    ctx.open_position(
        3,
        amount_x.into(),
        amount_y.into(),
        Tick::new(20_000).unwrap(),
        Tick::new(30_000).unwrap(),
    )
    .unwrap();

    let res = ctx.swap(Side::Left, SwapKind::ExactOut, (amount_y - 1u128).into());
    assert_matches!(res, Ok(_));
}

#[test]
fn test_swap_beyond_position_end_fails() {
    let mut ctx = new_swap_context();
    let amount_x = 100_000_u128;
    let amount_y = 200_000_u128;
    ctx.open_position(
        3,
        amount_x.into(),
        amount_y.into(),
        Tick::new(20_000).unwrap(),
        Tick::new(30_000).unwrap(),
    )
    .unwrap();

    let res = ctx.swap(Side::Left, SwapKind::ExactOut, (amount_y + 1u128).into());
    assert_matches!(
        res,
        Err(Error {
            kind: ErrorKind::InsufficientLiquidity { .. },
            ..
        })
    );
}

#[test]
fn test_swap_two_overlapping_positions() {
    let mut ctx = new_swap_context();
    ctx.open_position(
        3,
        100_000_u128.into(),
        200_000_u128.into(),
        Tick::new(20_000).unwrap(),
        Tick::new(30_000).unwrap(),
    )
    .unwrap();

    ctx.open_position(
        5,
        0_u128.into(),
        200_000_u128.into(),
        Tick::new(28_000).unwrap(),
        Tick::new(40_000).unwrap(),
    )
    .unwrap();

    let res = ctx.swap(Side::Left, SwapKind::ExactOut, 399_999_u128.into());
    assert_matches!(res, Ok(_));
}

#[test]
fn test_swap_to_price_too_low() {
    let mut ctx = new_swap_context();
    ctx.open_position(
        3,
        100_000_u128.into(),
        200_000_u128.into(),
        Tick::new(20_000).unwrap(),
        Tick::new(30_000).unwrap(),
    )
    .unwrap();

    let res = ctx
        .swap_to_price(Side::Left, 399_999_u128.into(), 1.0.into())
        .unwrap();
    assert_eq!(res, (Amount::zero(), Amount::zero()));
}

#[test]
fn test_swap_to_price_all() {
    let mut ctx = new_swap_context();
    ctx.open_position(
        3,
        200_000_u128.into(),
        200_000_u128.into(),
        Tick::new(-30_000).unwrap(),
        Tick::new(30_000).unwrap(),
    )
    .unwrap();

    let res = ctx
        .swap_to_price(Side::Left, 100_000_u128.into(), 5.0.into())
        .unwrap();
    assert_eq!(res, (100_000_u128.into(), 71_982_u128.into()));
}

#[test]
fn test_swap_to_price_limit() {
    let get_pool_price = |ctx: &SwapContext| {
        ctx.get_pool_info()
            .map(|info| {
                Float::from(info.position_reserves.0) / Float::from(info.position_reserves.1)
            })
            .unwrap()
    };

    let validate_price_limit = |price_limit: Float, current_price: Float| {
        let tolerance: Float = 0.01.into();

        assert!(current_price <= price_limit);
        assert!((price_limit - current_price) / price_limit < tolerance);
    };

    let mut ctx = new_swap_context();
    ctx.open_position(
        3,
        200_u128.into(),
        400_000_u128.into(),
        Tick::MIN,
        Tick::MAX,
    )
    .unwrap();

    let price_limit = get_pool_price(&ctx) * 1.1.into();
    let res = ctx
        .swap_to_price(Side::Left, 100_000_000_u128.into(), price_limit)
        .unwrap();

    assert_eq!(res, (10_u128.into(), 17_505_u128.into()));

    let new_pool_price = get_pool_price(&ctx);
    validate_price_limit(price_limit, new_pool_price);

    // Check if sligtly changed price limit results in more tokens
    let price_limit = price_limit * 1.1.into();
    let res = ctx
        .swap_to_price(Side::Left, 100_000_000_u128.into(), price_limit)
        .unwrap();
    assert_eq!(res, (11_u128.into(), 17_800_u128.into()));

    validate_price_limit(price_limit, get_pool_price(&ctx));
}

#[rstest]
fn test_swap_two_cl_positions(
    #[values(
    (-15000, -12000),
    (-15000, -10000),
    (-15000, -8000),
    (-15000, 0),
    (-15000, 2000),
    (-15000, 10000),
    (-15000, 14000),
    (-5000, -3000),
    (-5000, 0),
    (-5000, 3000),
    (-5000, 10000),
    (-5000, 12000),
    (0, 5000),
    (0, 10000),
    (0, 12000),
    (2000, 4000),
    (2000, 10000),
    (2000, 16000),
    (10000, 16000),
    )]
    pos1_range: (i32, i32),
    #[values(Side::Left, Side::Right)] swap_side: Side,
) {
    let arbitrary_amount: u128 = 100_000;
    let fee_level: FeeLevel = 3;
    let mut ctx = new_swap_context();
    let (_, pos0_amount_a, pos0_amount_b, _) = ctx
        .open_position(
            fee_level,
            arbitrary_amount.into(),
            arbitrary_amount.into(),
            Tick::new(-10_000).unwrap(),
            Tick::new(10_000).unwrap(),
        )
        .unwrap();

    let (_, pos1_amount_a, pos1_amount_b, _) = ctx
        .open_position(
            fee_level,
            arbitrary_amount.into(),
            arbitrary_amount.into(),
            Tick::new(pos1_range.0).unwrap(),
            Tick::new(pos1_range.1).unwrap(),
        )
        .unwrap();

    let amount_out = match swap_side {
        Side::Left => pos0_amount_b + pos1_amount_b - 100,
        Side::Right => pos0_amount_a + pos1_amount_a - 100,
    };

    ctx.swap(swap_side, SwapKind::ExactOut, amount_out.into())
        .unwrap();
    ctx.close_position(0).unwrap();
    ctx.close_position(1).unwrap();
    ctx.withdraw_protocol_fee().unwrap();

    let pool_info = ctx.get_pool_info().unwrap();
    let is_all_liquidities_zero = pool_info
        .liquidities
        .iter()
        .all(|liquidity| liquidity.is_zero());
    assert!(is_all_liquidities_zero);
    let is_position_reserves_zero =
        pool_info.position_reserves.0.is_zero() && pool_info.position_reserves.1.is_zero();
    assert!(is_position_reserves_zero);
    let is_total_reserves_zero =
        pool_info.total_reserves.0.is_zero() && pool_info.total_reserves.1.is_zero();
    assert!(is_total_reserves_zero);
}
