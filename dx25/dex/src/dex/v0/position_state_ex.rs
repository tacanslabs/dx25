use itertools::multizip;
#[allow(unused)]
use num_traits::Zero;

use crate::dex::pool::{fee_liquidity_from_net_liquidity, gross_liquidity_from_net_liquidity};
use crate::dex::{Error, FeeLevel, PositionV0, Side, Tick, Types};
use crate::{
    error_here, fp, AmountUFP, FeeLiquidityUFP, GrossLiquidityUFP, LongestUFP, NetLiquidityUFP,
};

use super::EffSqrtprices;

impl<T: Types> PositionV0<T> {
    pub fn net_liquidity(&self) -> NetLiquidityUFP {
        self.net_liquidity
    }

    pub fn gross_liquidity(&self) -> GrossLiquidityUFP {
        gross_liquidity_from_net_liquidity(self.net_liquidity, self.fee_level)
    }

    pub fn fee_liquidity(&self) -> FeeLiquidityUFP {
        fee_liquidity_from_net_liquidity(self.net_liquidity, self.fee_level)
    }

    pub fn eval_position_balance_ufp(
        &self,
        eff_sqrtprices: EffSqrtprices,
    ) -> Result<(AmountUFP, AmountUFP), Error> {
        eval_position_balance_ufp(
            self.net_liquidity,
            self.tick_bounds.0,
            self.tick_bounds.1,
            eff_sqrtprices,
            self.fee_level,
        )
    }
}

#[allow(clippy::useless_conversion)]
pub fn eval_position_balance_ufp(
    net_liquidity: NetLiquidityUFP,
    tick_low: Tick,
    tick_high: Tick,
    eff_sqrtprices: EffSqrtprices,
    fee_level: FeeLevel,
) -> Result<(AmountUFP, AmountUFP), Error> {
    let lower_bounds = [
        tick_low.eff_sqrtprice(fee_level, Side::Left),
        tick_high.eff_sqrtprice(fee_level, Side::Right),
    ];
    let upper_bounds = [
        tick_high.eff_sqrtprice(fee_level, Side::Left),
        tick_low.eff_sqrtprice(fee_level, Side::Right),
    ];

    let mut balances_ufp = [AmountUFP::zero(), AmountUFP::zero()];

    for (eff_sqrtprice, lower_bound, upper_bound, balance_ufp) in multizip((
        eff_sqrtprices.as_array(),
        lower_bounds,
        upper_bounds,
        &mut balances_ufp,
    )) {
        if eff_sqrtprice <= lower_bound {
            *balance_ufp = AmountUFP::zero();
        } else if eff_sqrtprice < upper_bound {
            let lower_bound =
                LongestUFP::try_from(lower_bound).map_err(|e: fp::Error| error_here!(e))?;
            let eff_sqrtprice =
                LongestUFP::try_from(eff_sqrtprice).map_err(|e: fp::Error| error_here!(e))?;
            let net_liquidity = LongestUFP::from(net_liquidity);
            #[cfg_attr(feature = "near", allow(clippy::useless_conversion))]
            {
                *balance_ufp = AmountUFP::try_from(net_liquidity * (eff_sqrtprice - lower_bound))
                    .map_err(|e| error_here!(e))?;
            }
        } else {
            let lower_bound =
                LongestUFP::try_from(lower_bound).map_err(|e: fp::Error| error_here!(e))?;
            let upper_bound =
                LongestUFP::try_from(upper_bound).map_err(|e: fp::Error| error_here!(e))?;
            let net_liquidity = LongestUFP::from(net_liquidity);
            #[cfg_attr(feature = "near", allow(clippy::useless_conversion))]
            {
                *balance_ufp = AmountUFP::try_from(net_liquidity * (upper_bound - lower_bound))
                    .map_err(|e| error_here!(e))?;
            }
        }
    }

    Ok((balances_ufp[0], balances_ufp[1]))
}

#[cfg(test)]
mod tests {
    use super::PositionV0;
    use crate::dex::tick::Tick;
    use crate::dex::FeeLevel;
    use crate::{assert_eq_rel_tol, Float, LPFeePerFeeLiquidity, Liquidity, TestTypes};
    use assert_matches::assert_matches;
    use num_traits::Zero;
    use rstest::rstest;
    use std::marker::PhantomData;

    #[test]
    fn test_gross_liquidity() {
        let fee_level: FeeLevel = 5;
        let one_over_sqrt_one_minus_fee_rate = Tick::BASE.powi(2_i32.pow(u32::from(fee_level)));
        let liquidity = Float::from(50);
        let eff_net_liqiudity = liquidity / one_over_sqrt_one_minus_fee_rate;

        let position = PositionV0::<TestTypes> {
            fee_level,
            net_liquidity: eff_net_liqiudity.try_into().unwrap(),
            init_acc_lp_fees_per_fee_liquidity: (
                LPFeePerFeeLiquidity::zero(),
                LPFeePerFeeLiquidity::zero(),
            ),
            tick_bounds: (Tick::MIN, Tick::MAX),
            init_sqrtprice: 0f64.into(),
            unwithdrawn_acc_lp_fees_per_fee_liquidity: (
                LPFeePerFeeLiquidity::zero(),
                LPFeePerFeeLiquidity::zero(),
            ),
            phantom_t: PhantomData,
        };

        let expected_gross_liquidity = liquidity * one_over_sqrt_one_minus_fee_rate;
        let actual_gross_liquidity = Float::from(position.gross_liquidity());

        assert_eq_rel_tol!(
            f64::from(actual_gross_liquidity),
            f64::from(expected_gross_liquidity),
            7
        );
    }

    #[rstest]
    fn test_cast_one_over_sqrt_one_minus_fee_rate_from_float_to_liquidity_is_always_successful(
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] fee_level: FeeLevel,
    ) {
        let one_over_sqrt_one_minus_fee_rate_tick = Tick::new(2_i32.pow(u32::from(fee_level) + 1));

        assert_matches!(one_over_sqrt_one_minus_fee_rate_tick, Ok(_));

        let one_over_sqrt_one_minus_fee_rate = one_over_sqrt_one_minus_fee_rate_tick
            .unwrap()
            .spot_sqrtprice();

        let one_over_sqrt_one_minus_fee_rate =
            Liquidity::try_from(one_over_sqrt_one_minus_fee_rate);

        assert_matches!(one_over_sqrt_one_minus_fee_rate, Ok(_));
    }
}
