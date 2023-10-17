#[cfg(test)]
mod tests {
    #![allow(
        clippy::needless_pass_by_value,
        clippy::too_many_lines,
        clippy::useless_conversion,
        clippy::cast_possible_truncation
    )]

    use crate::dex::latest::{EffSqrtprices, FeeLevelsArray, NUM_FEE_LEVELS};

    use crate::dex::pool::Pool as _;
    use crate::dex::pool::{
        fee_liquidity_from_net_liquidity, fee_rate, gross_liquidity_from_net_liquidity,
        one_over_sqrt_one_minus_fee_rate,
    };
    use crate::dex::test_utils::{ItemFactory, Types};
    use crate::dex::traits::ItemFactory as _;
    use crate::dex::{
        errors, BasisPoints, FeeLevel, Float, PairExt, Pool, PoolId, PoolV0, PositionClosedInfo,
        PositionInit, PositionOpenedInfo, Range, Side, Tick, BASIS_POINT_DIVISOR,
    };
    use crate::test_utils::{new_amount, new_token_id};
    use crate::{assert_eq_rel_tol, Amount, NetLiquidityUFP};
    use assert_matches::assert_matches;
    #[cfg(feature = "near")]
    use num_traits::Zero;
    use rstest::{fixture, rstest};

    const TOLERANCE: u32 = 5;

    #[fixture]
    fn factory() -> ItemFactory {
        ItemFactory::new()
    }

    #[fixture]
    fn pool_id() -> PoolId {
        let (pool_id, _swapped) = PoolId::try_from_pair((new_token_id(), new_token_id())).unwrap();
        pool_id
    }

    #[fixture]
    fn empty_pool(#[default(&mut factory())] factory: &mut ItemFactory) -> PoolV0<Types> {
        let Pool::V0(pool) = factory.new_pool().unwrap();
        pool
    }

    #[rstest]
    fn test_price_is_zero_at_all_levels_in_empty_pool(
        empty_pool: PoolV0<Types>,
        #[values(Side::Left, Side::Right)] side: Side,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        let actual_spot_price = empty_pool.spot_sqrtprice(side, fee_level);

        assert_eq!(actual_spot_price, Float::zero());

        assert_eq!(empty_pool.top_active_level, 0);
        assert_eq!(empty_pool.active_side, Side::Left);
    }

    const N: usize = NUM_FEE_LEVELS as usize;

    #[fixture]
    fn fee_rates(
        #[default([1, 2, 4, 8, 16, 32, 64, 128])] rates: [BasisPoints; N],
    ) -> FeeLevelsArray<BasisPoints> {
        rates.into()
    }

    #[fixture]
    fn protocol_fee_fraction() -> BasisPoints {
        1300
    }

    #[rstest]
    fn test_open_position_in_empty_pool(
        mut factory: ItemFactory,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        let mut empty_pool = empty_pool(&mut factory);

        let x_min = 0u128;
        let x_max = 3_000_000_000u128;
        let y_min = 0u128;
        let y_max = 7_000_000_000u128;

        let tick_low = Tick::new(-1700).unwrap();
        let tick_high = Tick::new(6800).unwrap();

        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let result = empty_pool.open_position(position, fee_level, 0, &mut factory);

        assert_matches!(result, Ok(_));

        let x = Float::from(x_max);
        let y = Float::from(y_max);
        let expected_x_amount: Amount = x_max.into();
        let expected_y_amount: Amount = y_max.into();
        let expected_net_liquidity = {
            let p_low_right = tick_high.eff_sqrtprice(fee_level, Side::Right);
            let p_low_left = tick_low.eff_sqrtprice(fee_level, Side::Left);
            let p_left = empty_pool.eff_sqrtprice(fee_level, Side::Left);
            let p_right = empty_pool.eff_sqrtprice(fee_level, Side::Right);
            let liquidity_left = x / (p_left - p_low_left);
            let liquidity_right = y / (p_right - p_low_right);
            assert_eq_rel_tol!(liquidity_left, liquidity_right, TOLERANCE + 2);
            NetLiquidityUFP::try_from(liquidity_left).unwrap()
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity: actual_net_liquidity,
            low_tick_liquidity_change,
            high_tick_liquidity_change,
        } = result.unwrap();

        let (actual_x_amount, actual_y_amount) = deposited_amounts;

        assert_eq!(expected_x_amount, actual_x_amount);
        assert_eq!(expected_y_amount, actual_y_amount);
        assert_eq_rel_tol!(expected_net_liquidity, actual_net_liquidity, TOLERANCE);

        let (tick, net_liquidity_change) = low_tick_liquidity_change;
        assert_eq!(tick_low, tick);
        assert_eq_rel_tol!(expected_net_liquidity, net_liquidity_change, TOLERANCE);

        let (tick, net_liquidity_change) = high_tick_liquidity_change;
        assert_eq!(tick_high, tick,);
        assert_eq_rel_tol!(
            Float::from(expected_net_liquidity),
            -net_liquidity_change,
            TOLERANCE
        );

        let actual_accounted_deposit = empty_pool.position_reserves()[fee_level as usize];
        let expected_deposit = (
            actual_accounted_deposit
                .0
                .ceil()
                .min(Amount::from(x_max).into()),
            actual_accounted_deposit
                .1
                .ceil()
                .min(Amount::from(y_max).into()),
        );
        assert_eq!(
            expected_deposit,
            (actual_x_amount.into(), actual_y_amount.into())
        );
        assert_eq_rel_tol!(empty_pool.total_reserves.0, actual_x_amount, TOLERANCE);
        assert_eq_rel_tol!(empty_pool.total_reserves.1, actual_y_amount, TOLERANCE);

        assert_eq_rel_tol!(
            empty_pool.net_liquidities[fee_level],
            expected_net_liquidity,
            TOLERANCE
        );
        assert_eq!(empty_pool.top_active_level, 0);
        assert_eq!(empty_pool.active_side, Side::Left);

        let p_low = tick_low.spot_sqrtprice();
        let p_high = tick_high.spot_sqrtprice();

        let expected_spot_price = {
            (((x / y - p_low * p_high).powi(2) + (Float::from(4) * x / y * p_high.powi(2))).sqrt()
                - (x / y - p_low * p_high))
                / (Float::from(2) * p_high)
        };

        let expected_gross_liquidity =
            gross_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);
        let expected_fee_liquidity =
            fee_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);

        assert_eq_rel_tol!(
            empty_pool.gross_liquidity(fee_level),
            expected_gross_liquidity,
            TOLERANCE
        );
        assert_eq_rel_tol!(
            empty_pool.fee_liquidity(fee_level),
            expected_fee_liquidity,
            TOLERANCE
        );

        let expected_eff_sqrtprices = FeeLevelsArray::<EffSqrtprices>::from_fn(|fee_level| {
            EffSqrtprices::from_value(
                expected_spot_price * one_over_sqrt_one_minus_fee_rate(fee_level as FeeLevel),
                Side::Left,
                fee_level as FeeLevel,
                None,
            )
            .unwrap()
        });

        for level in 0..NUM_FEE_LEVELS {
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Left,),
                expected_eff_sqrtprices[level].0,
                TOLERANCE
            );
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Right),
                expected_eff_sqrtprices[level].1,
                TOLERANCE
            );
        }

        let actual_spot_price = empty_pool.spot_sqrtprice(Side::Left, fee_level);

        assert_eq_rel_tol!(expected_spot_price, actual_spot_price, TOLERANCE);
    }

    #[rstest]
    fn test_open_position_in_pool_with_liquidity_when_spot_price_within_position_range(
        mut factory: ItemFactory,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        let mut empty_pool = empty_pool(&mut factory);

        let x_min = 0u128;
        let x_max = 700_000_000_000u128;
        let y_min = 0u128;
        let y_max = 80_000_000_000_000_u128;

        let tick_low = Tick::new(-120_000).unwrap();
        let tick_high = Tick::new(87_000).unwrap();

        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let result = empty_pool
            .open_position(position.clone(), fee_level, 0, &mut factory)
            .unwrap();

        let expected_x_amount: Amount = x_max.into();
        let expected_y_amount: Amount = y_max.into();
        let expected_net_liquidity = {
            let x = Float::from(x_max);
            let y = Float::from(y_max);
            let p_low_right = tick_high.eff_sqrtprice(fee_level, Side::Right);
            let p_low_left = tick_low.eff_sqrtprice(fee_level, Side::Left);
            let p_left = empty_pool.eff_sqrtprice(fee_level, Side::Left);
            let p_right = empty_pool.eff_sqrtprice(fee_level, Side::Right);
            let liquidity_left = x / (p_left - p_low_left);
            let liquidity_right = y / (p_right - p_low_right);
            assert_eq_rel_tol!(liquidity_left, liquidity_right, TOLERANCE);
            NetLiquidityUFP::try_from(liquidity_left).unwrap()
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity: actual_net_liquidity,
            low_tick_liquidity_change,
            high_tick_liquidity_change,
        } = result;

        let (actual_x_amount, actual_y_amount) = deposited_amounts;

        assert_eq_rel_tol!(expected_x_amount, actual_x_amount, TOLERANCE);
        assert_eq_rel_tol!(expected_y_amount, actual_y_amount, TOLERANCE);
        assert_eq_rel_tol!(expected_net_liquidity, actual_net_liquidity, TOLERANCE);

        let (tick, net_liquidity_change) = low_tick_liquidity_change;
        assert_eq!(tick_low, tick);
        assert_eq_rel_tol!(expected_net_liquidity, net_liquidity_change, TOLERANCE);

        let (tick, net_liquidity_change) = high_tick_liquidity_change;
        assert_eq!(tick_high, tick);
        assert_eq_rel_tol!(
            Float::from(expected_net_liquidity),
            -net_liquidity_change,
            TOLERANCE
        );

        let actual_accounted_deposit = empty_pool.position_reserves()[fee_level as usize];
        let expected_deposit = (
            actual_accounted_deposit
                .0
                .ceil()
                .min(Amount::from(x_max).into()),
            actual_accounted_deposit
                .1
                .ceil()
                .min(Amount::from(y_max).into()),
        );
        assert_eq!(
            expected_deposit,
            (actual_x_amount.into(), actual_y_amount.into())
        );
        assert_eq_rel_tol!(empty_pool.total_reserves.0, actual_x_amount, 1);
        assert_eq_rel_tol!(empty_pool.total_reserves.1, actual_y_amount, 1);

        assert_eq_rel_tol!(
            empty_pool.net_liquidities[fee_level],
            expected_net_liquidity,
            6
        );

        assert_eq!(empty_pool.top_active_level, 0);
        assert_eq!(empty_pool.active_side, Side::Left);

        let expected_spot_price = {
            let x = Float::from(x_max);
            let y = Float::from(y_max);
            let p_low = tick_low.spot_sqrtprice();
            let p_high = tick_high.spot_sqrtprice();

            (((x / y - p_low * p_high).powi(2) + (Float::from(4) * x / y * p_high.powi(2))).sqrt()
                - (x / y - p_low * p_high))
                / (Float::from(2) * p_high)
        };

        let expected_gross_liquidity =
            gross_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);
        let expected_fee_liquidity =
            fee_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);

        assert_eq_rel_tol!(
            empty_pool.gross_liquidity(fee_level),
            expected_gross_liquidity,
            TOLERANCE
        );
        assert_eq_rel_tol!(
            empty_pool.fee_liquidity(fee_level),
            expected_fee_liquidity,
            TOLERANCE
        );

        let expected_eff_sqrtprices = FeeLevelsArray::<EffSqrtprices>::from_fn(|fee_level| {
            EffSqrtprices::from_value(
                expected_spot_price * one_over_sqrt_one_minus_fee_rate(fee_level as FeeLevel),
                Side::Left,
                fee_level as FeeLevel,
                None,
            )
            .unwrap()
        });

        for level in 0..NUM_FEE_LEVELS {
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Left),
                expected_eff_sqrtprices[level].0,
                TOLERANCE
            );
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Right,),
                expected_eff_sqrtprices[level].1,
                TOLERANCE
            );
        }

        let actual_spot_price = empty_pool.spot_sqrtprice(Side::Left, fee_level);

        assert_eq_rel_tol!(expected_spot_price, actual_spot_price, 6);

        let last_net_liquidity = empty_pool.net_liquidities.raw[fee_level as usize];
        let last_gross_liquidity = empty_pool.gross_liquidity(fee_level);
        let last_fee_liquidity = empty_pool.fee_liquidity(fee_level);
        let (last_total_reserves_x, last_total_reserves_y) = empty_pool.total_reserves;

        let result = empty_pool.open_position(position, fee_level, 1, &mut factory);

        assert_matches!(result, Ok(_));

        let x = Float::from(x_max);
        let y = Float::from(y_max);
        let expected_x_amount: Amount = x_max.into();
        let expected_y_amount: Amount = y_max.into();

        let expected_net_liquidity = {
            let p_high = tick_high.eff_sqrtprice(fee_level, Side::Right);
            let p_low = tick_low.eff_sqrtprice(fee_level, Side::Left);
            let p_x = expected_eff_sqrtprices.raw[fee_level as usize].0;
            let p_y = expected_eff_sqrtprices.raw[fee_level as usize].1;

            let net_liquidity_x = x / (p_x - p_low);
            let net_liquidity_y = y / (p_y - p_high);

            NetLiquidityUFP::try_from(net_liquidity_x.min(net_liquidity_y)).unwrap()
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity: actual_net_liquidity,
            ..
        } = result.unwrap();

        let (actual_x_amount, actual_y_amount) = deposited_amounts;

        assert_eq_rel_tol!(expected_x_amount, actual_x_amount, TOLERANCE);
        assert_eq_rel_tol!(expected_y_amount, actual_y_amount, TOLERANCE);
        assert_eq_rel_tol!(expected_net_liquidity, actual_net_liquidity, TOLERANCE);

        // assert_eq!(
        //     empty_pool.position_reserves()[fee_level as usize],
        //     (actual_x_amount.into(), actual_y_amount.into())
        // );
        assert_eq!(
            empty_pool.total_reserves,
            (
                last_total_reserves_x + actual_x_amount,
                last_total_reserves_y + actual_y_amount
            )
        );
        assert_eq_rel_tol!(
            empty_pool.net_liquidities[fee_level],
            last_net_liquidity + expected_net_liquidity,
            TOLERANCE
        );
        assert_eq!(empty_pool.top_active_level, 0);
        assert_eq!(empty_pool.active_side, Side::Left);

        let expected_spot_price = {
            let x = Float::from(x_max);
            let y = Float::from(y_max);
            let p_low = tick_low.spot_sqrtprice();
            let p_high = tick_high.spot_sqrtprice();

            (((x / y - p_low * p_high).powi(2) + (Float::from(4) * x / y * p_high.powi(2))).sqrt()
                - (x / y - p_low * p_high))
                / (Float::from(2) * p_high)
        };

        let expected_gross_liquidity =
            gross_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);
        let expected_fee_liquidity =
            fee_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);

        assert_eq_rel_tol!(
            empty_pool.gross_liquidity(fee_level),
            last_gross_liquidity + expected_gross_liquidity,
            TOLERANCE
        );
        assert_eq_rel_tol!(
            empty_pool.fee_liquidity(fee_level),
            last_fee_liquidity + expected_fee_liquidity,
            TOLERANCE
        );

        for level in 0..NUM_FEE_LEVELS {
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Left,),
                expected_eff_sqrtprices[level].0,
                TOLERANCE
            );
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Right),
                expected_eff_sqrtprices[level].1,
                TOLERANCE
            );
        }

        let actual_spot_price = empty_pool.spot_sqrtprice(Side::Left, fee_level);

        assert_eq_rel_tol!(expected_spot_price, actual_spot_price, TOLERANCE);
    }

    #[rstest]
    fn test_open_position_in_pool_with_liquidity_when_spot_price_below_position_range(
        mut factory: ItemFactory,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        let mut empty_pool = empty_pool(&mut factory);

        let x_min = 0u128;
        let x_max = 700_000_000_000_000u128;
        let y_min = 0u128;
        let y_max = 80_000_000_000_000u128;

        let tick_low = Tick::new(-1700).unwrap();
        let tick_high = Tick::new(6800).unwrap();

        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let result = empty_pool.open_position(position, fee_level, 0, &mut factory);

        assert_matches!(result, Ok(_));

        let expected_x_amount: Amount = x_max.into();
        let expected_y_amount: Amount = y_max.into();
        let expected_net_liquidity = {
            let x = Float::from(x_max);
            let y = Float::from(y_max);
            let p_low_left = tick_low.eff_sqrtprice(fee_level, Side::Left);
            let p_low_right = tick_high.eff_sqrtprice(fee_level, Side::Right);
            let p_left = empty_pool.eff_sqrtprice(fee_level, Side::Left);
            let p_right = empty_pool.eff_sqrtprice(fee_level, Side::Right);
            let liquidity_left = x / (p_left - p_low_left);
            let liquidity_right = y / (p_right - p_low_right);
            assert_eq_rel_tol!(liquidity_left, liquidity_right, TOLERANCE + 2);
            NetLiquidityUFP::try_from(liquidity_left).unwrap()
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity: actual_net_liquidity,
            low_tick_liquidity_change,
            high_tick_liquidity_change,
        } = result.unwrap();

        let (actual_x_amount, actual_y_amount) = deposited_amounts;

        assert_eq_rel_tol!(expected_x_amount, actual_x_amount, TOLERANCE + 2);
        assert_eq_rel_tol!(expected_y_amount, actual_y_amount, TOLERANCE + 2);
        assert_eq_rel_tol!(expected_net_liquidity, actual_net_liquidity, TOLERANCE + 2);

        let (tick, net_liquidity_change) = low_tick_liquidity_change;
        assert_eq!(tick_low, tick);
        assert_eq_rel_tol!(expected_net_liquidity, net_liquidity_change, TOLERANCE + 2);

        let (tick, net_liquidity_change) = high_tick_liquidity_change;
        assert_eq!(tick_high, tick);
        assert_eq_rel_tol!(
            Float::from(expected_net_liquidity),
            -net_liquidity_change,
            TOLERANCE + 2
        );

        let actual_accounted_deposit = empty_pool.position_reserves()[fee_level as usize];
        let expected_deposit = (
            actual_accounted_deposit
                .0
                .ceil()
                .min(Amount::from(x_max).into()),
            actual_accounted_deposit
                .1
                .ceil()
                .min(Amount::from(y_max).into()),
        );
        assert_eq!(
            expected_deposit,
            (actual_x_amount.into(), actual_y_amount.into())
        );

        assert_eq_rel_tol!(empty_pool.total_reserves.0, actual_x_amount, TOLERANCE);
        assert_eq_rel_tol!(empty_pool.total_reserves.1, actual_y_amount, TOLERANCE);

        assert_eq_rel_tol!(
            empty_pool.net_liquidities[fee_level],
            expected_net_liquidity,
            TOLERANCE + 2
        );

        assert_eq!(empty_pool.top_active_level, 0);

        let expected_spot_price = {
            let x = Float::from(x_max);
            let y = Float::from(y_max);
            let p_low = tick_low.spot_sqrtprice();
            let p_high = tick_high.spot_sqrtprice();

            (((x / y - p_low * p_high).powi(2) + (Float::from(4) * x / y * p_high.powi(2))).sqrt()
                - (x / y - p_low * p_high))
                / (Float::from(2) * p_high)
        };

        let expected_gross_liquidity =
            gross_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);
        let expected_fee_liquidity =
            fee_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);

        assert_eq_rel_tol!(
            empty_pool.gross_liquidity(fee_level),
            expected_gross_liquidity,
            TOLERANCE + 2
        );
        assert_eq_rel_tol!(
            empty_pool.fee_liquidity(fee_level),
            expected_fee_liquidity,
            TOLERANCE + 2
        );

        let expected_eff_sqrtprices = FeeLevelsArray::<EffSqrtprices>::from_fn(|fee_level| {
            EffSqrtprices::from_value(
                expected_spot_price * one_over_sqrt_one_minus_fee_rate(fee_level as FeeLevel),
                Side::Left,
                fee_level as FeeLevel,
                None,
            )
            .unwrap()
        });

        for level in 0..NUM_FEE_LEVELS {
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Left,),
                expected_eff_sqrtprices[level].0,
                TOLERANCE
            );
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Right),
                expected_eff_sqrtprices[level].1,
                TOLERANCE
            );
        }

        let actual_spot_price = empty_pool.spot_sqrtprice(Side::Left, fee_level);

        assert_eq_rel_tol!(expected_spot_price, actual_spot_price, 6);

        let x_min = 0u128;
        let x_max = 700_000_000_000u128;
        let y_min = 0u128;
        let y_max = 80_000_000_000_000u128;

        let tick_low = Tick::new(6800).unwrap();
        let tick_high = Tick::new(16800).unwrap();

        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let last_net_liquidity = empty_pool.net_liquidities.raw[fee_level as usize];
        let last_gross_liquidity = empty_pool.gross_liquidity(fee_level);
        let last_fee_liquidity = empty_pool.fee_liquidity(fee_level);
        let (last_total_reserves_x, last_total_reserves_y) = empty_pool.total_reserves;

        let result = empty_pool.open_position(position, fee_level, 1, &mut factory);

        assert_matches!(result, Ok(_));

        let expected_x_amount = Amount::zero();
        let expected_y_amount: Amount = y_max.into();
        let expected_net_liquidity = {
            let y = Float::from(y_max);
            let p_high = tick_high.eff_sqrtprice(fee_level, Side::Right);
            let p_low = tick_low.eff_sqrtprice(fee_level, Side::Right);

            let liquidity = y / (p_low - p_high);

            NetLiquidityUFP::try_from(liquidity).unwrap()
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity: actual_net_liquidity,
            ..
        } = result.unwrap();

        let (actual_x_amount, actual_y_amount) = deposited_amounts;

        assert_eq!(expected_x_amount, actual_x_amount);
        assert_eq!(expected_y_amount, actual_y_amount);
        assert_eq_rel_tol!(expected_net_liquidity, actual_net_liquidity, TOLERANCE);
        // assert_eq!(
        //     empty_pool.position_reserves()[fee_level as usize],
        //     (actual_x_amount.into(), actual_y_amount.into())
        // );
        assert_eq!(
            empty_pool.total_reserves,
            (
                last_total_reserves_x + actual_x_amount,
                last_total_reserves_y + actual_y_amount
            )
        );
        assert_eq!(empty_pool.net_liquidities[fee_level], last_net_liquidity);
        assert_eq_rel_tol!(
            empty_pool.gross_liquidity(fee_level),
            last_gross_liquidity,
            TOLERANCE
        );

        assert_eq!(empty_pool.fee_liquidity(fee_level), last_fee_liquidity);

        assert_eq!(empty_pool.top_active_level, 0);

        for level in 0..NUM_FEE_LEVELS {
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Left,),
                expected_eff_sqrtprices[level].0,
                TOLERANCE
            );
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Right),
                expected_eff_sqrtprices[level].1,
                TOLERANCE
            );
        }

        let actual_spot_price = empty_pool.spot_sqrtprice(Side::Left, fee_level);

        assert_eq_rel_tol!(expected_spot_price, actual_spot_price, TOLERANCE);
    }

    #[rstest]
    fn test_open_position_in_pool_with_liquidity_when_spot_price_above_position_range(
        mut factory: ItemFactory,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        let mut empty_pool = empty_pool(&mut factory);

        let x_min = 0u128;
        let x_max = 700_000_000_000u128;
        let y_min = 0u128;
        let y_max = 80_000_000_000_000u128;

        let tick_low = Tick::new(-17000).unwrap();
        let tick_high = Tick::new(68000).unwrap();

        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let result = empty_pool.open_position(position, fee_level, 0, &mut factory);

        assert_matches!(result, Ok(_));

        let expected_x_amount: Amount = x_max.into();
        let expected_y_amount: Amount = y_max.into();
        let expected_net_liquidity = {
            let x = Float::from(x_max);
            let y = Float::from(y_max);
            let p_low_right = tick_high.eff_sqrtprice(fee_level, Side::Right);
            let p_low_left = tick_low.eff_sqrtprice(fee_level, Side::Left);
            let p_left = empty_pool.eff_sqrtprice(fee_level, Side::Left);
            let p_right = empty_pool.eff_sqrtprice(fee_level, Side::Right);
            let liquidity_left = x / (p_left - p_low_left);
            let liquidity_right = y / (p_right - p_low_right);
            assert_eq_rel_tol!(liquidity_left, liquidity_right, TOLERANCE + 2);
            NetLiquidityUFP::try_from(liquidity_left).unwrap()
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity: actual_net_liquidity,
            low_tick_liquidity_change,
            high_tick_liquidity_change,
        } = result.unwrap();

        let (actual_x_amount, actual_y_amount) = deposited_amounts;

        assert_eq!(expected_x_amount, actual_x_amount);
        assert_eq!(expected_y_amount, actual_y_amount);
        assert_eq_rel_tol!(expected_net_liquidity, actual_net_liquidity, TOLERANCE + 1);

        let (tick, net_liquidity_change) = low_tick_liquidity_change;
        assert_eq!(tick_low, tick);
        assert_eq_rel_tol!(expected_net_liquidity, net_liquidity_change, TOLERANCE + 1);

        let (tick, net_liquidity_change) = high_tick_liquidity_change;
        assert_eq!(tick_high, tick);
        assert_eq_rel_tol!(
            Float::from(expected_net_liquidity),
            -net_liquidity_change,
            TOLERANCE + 1
        );

        let actual_accounted_deposit = empty_pool.position_reserves()[fee_level as usize];
        let expected_deposit = (
            actual_accounted_deposit
                .0
                .ceil()
                .min(Amount::from(x_max).into()),
            actual_accounted_deposit
                .1
                .ceil()
                .min(Amount::from(y_max).into()),
        );
        assert_eq!(
            expected_deposit,
            (actual_x_amount.into(), actual_y_amount.into())
        );
        assert_eq!(
            empty_pool.total_reserves,
            (actual_x_amount, actual_y_amount)
        );

        let expected_spot_price = {
            let x = Float::from(x_max);
            let y = Float::from(y_max);
            let p_low = tick_low.spot_sqrtprice();
            let p_high = tick_high.spot_sqrtprice();

            (((x / y - p_low * p_high).powi(2) + (Float::from(4) * x / y * p_high.powi(2))).sqrt()
                - (x / y - p_low * p_high))
                / (Float::from(2) * p_high)
        };

        let expected_gross_liquidity =
            gross_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);
        let expected_fee_liquidity =
            fee_liquidity_from_net_liquidity(expected_net_liquidity, fee_level);

        assert_eq_rel_tol!(
            empty_pool.net_liquidities[fee_level],
            expected_net_liquidity,
            TOLERANCE + 1
        );
        assert_eq_rel_tol!(
            empty_pool.gross_liquidity(fee_level),
            expected_gross_liquidity,
            TOLERANCE + 1
        );
        assert_eq_rel_tol!(
            empty_pool.fee_liquidity(fee_level),
            expected_fee_liquidity,
            TOLERANCE + 1
        );

        assert_eq!(empty_pool.top_active_level, 0);
        assert_eq!(empty_pool.active_side, Side::Left);

        let expected_eff_sqrtprices = FeeLevelsArray::<EffSqrtprices>::from_fn(|fee_level| {
            EffSqrtprices::from_value(
                expected_spot_price * one_over_sqrt_one_minus_fee_rate(fee_level as FeeLevel),
                Side::Left,
                fee_level as FeeLevel,
                None,
            )
            .unwrap()
        });

        for level in 0..NUM_FEE_LEVELS {
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Left),
                expected_eff_sqrtprices[level].0,
                TOLERANCE
            );
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Right),
                expected_eff_sqrtprices[level].1,
                TOLERANCE
            );
        }

        let actual_spot_price = empty_pool.spot_sqrtprice(Side::Left, fee_level);

        assert_eq_rel_tol!(expected_spot_price, actual_spot_price, TOLERANCE);

        let x_min = 0u128;
        let x_max = 700_000_000_000u128;
        let y_min = 0u128;
        let y_max = 80_000_000_000_000u128;

        let tick_low = Tick::new(-33000).unwrap();
        let tick_high = Tick::new(-17000).unwrap();

        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let last_net_liquidity = empty_pool.net_liquidities.raw[fee_level as usize];
        let last_gross_liquidity = empty_pool.gross_liquidity(fee_level);
        let last_fee_liquidity = empty_pool.fee_liquidity(fee_level);
        let (last_total_reserves_x, last_total_reserves_y) = empty_pool.total_reserves;

        let result = empty_pool.open_position(position, fee_level, 1, &mut factory);

        assert_matches!(result, Ok(_));

        let expected_x_amount: Amount = x_max.into();
        let expected_y_amount = Amount::zero();
        let expected_net_liquidity = {
            let x = Float::from(x_max);
            let p_high = tick_high.eff_sqrtprice(fee_level, Side::Left);
            let p_low = tick_low.eff_sqrtprice(fee_level, Side::Left);

            let liquidity = x / (p_high - p_low);

            NetLiquidityUFP::try_from(liquidity).unwrap()
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity: actual_net_liquidity,
            ..
        } = result.unwrap();

        let (actual_x_amount, actual_y_amount) = deposited_amounts;

        assert_eq!(expected_x_amount, actual_x_amount);
        assert_eq!(expected_y_amount, actual_y_amount);
        assert_eq_rel_tol!(expected_net_liquidity, actual_net_liquidity, TOLERANCE);
        // assert_eq!(
        //     empty_pool.position_reserves()[fee_level as usize],
        //     (actual_x_amount.into(), actual_y_amount.into())
        // );
        assert_eq!(
            empty_pool.total_reserves,
            (
                last_total_reserves_x + actual_x_amount,
                last_total_reserves_y + actual_y_amount
            )
        );
        assert_eq!(empty_pool.net_liquidities[fee_level], last_net_liquidity);
        assert_eq_rel_tol!(
            empty_pool.gross_liquidity(fee_level),
            last_gross_liquidity,
            TOLERANCE
        );
        assert_eq_rel_tol!(
            empty_pool.fee_liquidity(fee_level),
            last_fee_liquidity,
            TOLERANCE
        );

        assert_eq!(empty_pool.top_active_level, 0);
        assert_eq!(empty_pool.active_side, Side::Left);

        for level in 0..NUM_FEE_LEVELS {
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Left),
                expected_eff_sqrtprices[level].0,
                TOLERANCE
            );
            assert_eq_rel_tol!(
                empty_pool.eff_sqrtprice(level, Side::Right),
                expected_eff_sqrtprices[level].1,
                TOLERANCE
            );
        }

        let actual_spot_price = empty_pool.spot_sqrtprice(Side::Left, fee_level);

        assert_eq_rel_tol!(expected_spot_price, actual_spot_price, TOLERANCE);
    }

    #[rstest]
    fn test_close_position_for_non_existed_position(mut empty_pool: PoolV0<Types>) {
        let non_existed_position_id = 0_u64;

        let result = empty_pool.withdraw_fee_and_close_position(non_existed_position_id);

        assert_matches!(
            result,
            Err(errors::Error {
                kind: errors::ErrorKind::PositionDoesNotExist,
                ..
            })
        );
    }

    #[rstest]
    fn test_close_position_right_after_open_position(
        mut empty_pool: PoolV0<Types>,
        #[values(Side::Left, Side::Right)] side: Side,
        #[values(0)] fee_level: FeeLevel,
        mut factory: ItemFactory,
    ) {
        let x_min = 100_000_u128;
        let x_max = 100_000_u128;
        let y_min = 100_000_u128;
        let y_max = 100_000_u128;

        let tick_low = Tick::new(-100).unwrap();
        let tick_high = Tick::new(100).unwrap();

        let position_id = 0;
        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let PositionOpenedInfo {
            deposited_amounts, ..
        } = empty_pool
            .open_position(position, fee_level, position_id, &mut factory)
            .unwrap();

        let (x, y) = deposited_amounts;

        assert_eq_rel_tol!(
            empty_pool.spot_sqrtprice(side, fee_level),
            Float::one(),
            TOLERANCE
        );

        let PositionClosedInfo {
            fees,
            balance,
            fee_level: _,
            low_tick_liquidity_change,
            high_tick_liquidity_change,
        } = empty_pool
            .withdraw_fee_and_close_position(position_id)
            .unwrap();

        assert_eq!(fees, (new_amount(0), new_amount(0)));
        assert_eq!(balance, (x - 1, y - 1));

        let (tick, net_liquidity_change) = low_tick_liquidity_change;
        assert_eq!(tick_low, tick);
        assert_eq!(Float::from(0), net_liquidity_change);

        let (tick, net_liquidity_change) = high_tick_liquidity_change;
        assert_eq!(tick_high, tick);
        assert_eq!(Float::from(0), net_liquidity_change);
    }

    #[rstest]
    fn test_withdraw_fee_with_non_existed_position_id(mut empty_pool: PoolV0<Types>) {
        let position_id = 0_u64;
        let result = empty_pool.withdraw_fee(position_id);

        assert_matches!(
            result,
            Err(errors::Error {
                kind: errors::ErrorKind::PositionDoesNotExist,
                ..
            })
        );
    }

    #[rstest]
    fn test_close_position_after_one_swap(
        mut empty_pool: PoolV0<Types>,
        #[values(Side::Left, Side::Right)] side: Side,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
        #[values(1235)] protocol_fee_fraction: BasisPoints,
        mut factory: ItemFactory,
    ) {
        let amount = new_amount(1_u128 << 70);

        let x_min = new_amount(0_u128);
        let x_max = amount;
        let y_min = new_amount(0_u128);
        let y_max = amount;

        let tick_low = Tick::new(-100_000).unwrap();
        let tick_high = Tick::new(100_000).unwrap();

        let position_id = 0;
        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: x_min.into(),
                    max: x_max.into(),
                },
                Range {
                    min: y_min.into(),
                    max: y_max.into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        empty_pool
            .open_position(position, fee_level, position_id, &mut factory)
            .unwrap();

        assert_eq_rel_tol!(
            empty_pool.spot_sqrtprice(side, fee_level),
            Float::one(),
            TOLERANCE
        );

        let amount_in = amount;
        empty_pool
            .swap_exact_in(side, amount_in, protocol_fee_fraction)
            .unwrap();

        let PositionClosedInfo {
            fees: actual_fee,
            balance: _,
            fee_level: _,
            low_tick_liquidity_change,
            high_tick_liquidity_change,
        } = empty_pool
            .withdraw_fee_and_close_position(position_id)
            .unwrap();

        let (tick, net_liquidity_change) = low_tick_liquidity_change;
        assert_eq!(tick_low, tick);
        assert_eq!(Float::from(0), net_liquidity_change);

        let (tick, net_liquidity_change) = high_tick_liquidity_change;
        assert_eq!(tick_high, tick);
        assert_eq!(Float::from(0), net_liquidity_change);

        let fee_rate_on_current_level = fee_rate(fee_level);

        let lp_fee_fraction = Float::from(BASIS_POINT_DIVISOR - protocol_fee_fraction)
            / Float::from(BASIS_POINT_DIVISOR);

        let expected_fee = match side {
            Side::Left => {
                let expected_fee_left =
                    Float::from(amount_in) * fee_rate_on_current_level * lp_fee_fraction;
                let expected_fee_right = Float::zero();
                (
                    Amount::try_from(expected_fee_left).unwrap(),
                    Amount::try_from(expected_fee_right).unwrap(),
                )
            }
            Side::Right => {
                let expected_fee_left = Float::zero();
                let expected_fee_right =
                    Float::from(amount_in) * fee_rate_on_current_level * lp_fee_fraction;
                (
                    Amount::try_from(expected_fee_left).unwrap(),
                    Amount::try_from(expected_fee_right).unwrap(),
                )
            }
        };
        assert_eq_rel_tol!(actual_fee.0, expected_fee.0, TOLERANCE);
        assert_eq_rel_tol!(actual_fee.1, expected_fee.1, TOLERANCE);
    }

    #[rstest]
    fn test_get_position_info_for_non_existed_position(pool_id: PoolId, empty_pool: PoolV0<Types>) {
        let position_id = 0;
        let result = empty_pool.get_position_info(&pool_id, position_id);
        assert_matches!(
            result,
            Err(errors::Error {
                kind: errors::ErrorKind::PositionDoesNotExist,
                ..
            })
        );
    }

    #[rstest]
    fn test_get_position_info_for_existed_position(
        pool_id: PoolId,
        mut empty_pool: PoolV0<Types>,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
        #[values(Side::Left, Side::Right)] side: Side,
        mut factory: ItemFactory,
    ) {
        let amount = 1_u128 << 60;

        let x_min = 0_u128;
        let x_max = amount;
        let y_min = 0_u128;
        let y_max = amount;

        let tick_low = Tick::new(-100).unwrap();
        let tick_high = Tick::new(100).unwrap();

        let position_id = 0;
        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: new_amount(x_min).into(),
                    max: new_amount(x_max).into(),
                },
                Range {
                    min: new_amount(y_min).into(),
                    max: new_amount(y_max).into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let PositionOpenedInfo {
            deposited_amounts,
            net_liquidity,
            ..
        } = empty_pool
            .open_position(position, fee_level, position_id, &mut factory)
            .unwrap();

        let (x, y) = deposited_amounts;

        assert_eq_rel_tol!(
            empty_pool.spot_sqrtprice(side, fee_level),
            Float::one(),
            TOLERANCE
        );

        let position_info = empty_pool.get_position_info(&pool_id, position_id).unwrap();

        assert_eq!(
            position_info.tokens_ids,
            pool_id.as_refs().map(Clone::clone)
        );
        assert_eq_rel_tol!(position_info.balance.0, x, TOLERANCE);
        assert_eq_rel_tol!(position_info.balance.1, y, TOLERANCE);
        assert_eq_rel_tol!(position_info.init_sqrtprice, Float::one(), TOLERANCE);
        assert_eq!(position_info.range_ticks, (tick_low, tick_high));
        assert_eq_rel_tol!(
            position_info.reward_since_last_withdraw.0,
            Float::zero(),
            TOLERANCE
        );
        assert_eq_rel_tol!(
            position_info.reward_since_last_withdraw.1,
            Float::zero(),
            TOLERANCE
        );
        assert_eq_rel_tol!(
            position_info.reward_since_creation.0,
            Float::zero(),
            TOLERANCE
        );
        assert_eq_rel_tol!(
            position_info.reward_since_creation.1,
            Float::zero(),
            TOLERANCE
        );

        assert_eq_rel_tol!(
            Float::from(net_liquidity),
            position_info.net_liquidity,
            TOLERANCE
        );
    }

    /// Perform two swaps with different protocol fee fraction
    /// and check the accumulated protocol fee.
    #[rstest]
    fn test_change_protocol_fee_fraction(
        mut empty_pool: PoolV0<Types>,
        #[values(Side::Left, Side::Right)] side: Side,
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
        mut factory: ItemFactory,
    ) {
        let x_min = new_amount(0_u128);
        let x_max = new_amount(17_u128 << 70);
        let y_min = new_amount(0_u128);
        let y_max = new_amount(17_u128 << 70);
        let tick_low = Tick::new(-1000).unwrap();
        let tick_high = Tick::new(1000).unwrap();

        let position = PositionInit {
            amount_ranges: (
                Range {
                    min: x_min.into(),
                    max: x_max.into(),
                },
                Range {
                    min: y_min.into(),
                    max: y_max.into(),
                },
            ),
            ticks_range: (Some(tick_low.index()), Some(tick_high.index())),
        };

        let position_id = 0;
        empty_pool
            .open_position(position, fee_level, position_id, &mut factory)
            .unwrap();

        let swap1_amount = new_amount(11_u128 << 70);
        let swap1_protocol_fee_fraction = 2000;

        empty_pool
            .swap_exact_in(side, swap1_amount, swap1_protocol_fee_fraction)
            .unwrap();

        let swap2_amount = new_amount(3_u128 << 70);
        let swap2_protocol_fee_fraction = 3000;

        empty_pool
            .swap_exact_in(side, swap2_amount, swap2_protocol_fee_fraction)
            .unwrap();

        let protocol_fee = empty_pool.withdraw_protocol_fee().unwrap()[side];

        let expected_protocol_fee = Float::from(swap1_amount)
            * fee_rate(fee_level)
            * Float::from(swap1_protocol_fee_fraction)
            / Float::from(BASIS_POINT_DIVISOR)
            + Float::from(swap2_amount)
                * fee_rate(fee_level)
                * Float::from(swap2_protocol_fee_fraction)
                / Float::from(BASIS_POINT_DIVISOR);

        // Protocol fee accomodates rounding errors, so it can significantly deviate from the "expected" value.
        // Protocol fee may not be much lower than the "expected" value, but in marginal cases can exceed it
        // multiple times.
        assert!(Float::from(protocol_fee) > Float::from(1.0 - 1e-10) * expected_protocol_fee);
        assert!(Float::from(protocol_fee) < Float::from(1.0 + 1e-6) * expected_protocol_fee);
    }
}
