// Some of the conversions are useless, because NEAR Amount is u128,
// which is not the same for other DEX's
#![allow(
    clippy::useless_conversion,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_arguments
)]

use super::test_utils::{new_account_id, new_amount, new_token_id};
use super::{BasisPoints, EstimateAddLiquidityResult, Estimations};
use crate::chain::TokenId;
use crate::dex::test_utils::Sandbox;
use crate::dex::tick::Tick;
use crate::dex::{PoolInfo, PositionId, PositionInit, Range, Result};
use crate::{Amount, Float, Liquidity};
use assert_matches::assert_matches;
use rstest::rstest;

struct TestContext {
    pub state: Sandbox,
    pub tokens: (TokenId, TokenId),
}

impl TestContext {
    fn new() -> Self {
        let acc = new_account_id();
        #[allow(clippy::clone_on_copy)]
        let sandbox = Sandbox::new_default(acc);

        let token_0 = new_token_id();
        let token_1 = new_token_id();

        TestContext {
            state: sandbox,
            tokens: (token_0, token_1),
        }
    }

    fn new_with_price(price: f64, fee_rate: BasisPoints, swap_token_ids: bool) -> Self {
        let acc = new_account_id();
        #[allow(clippy::clone_on_copy)]
        let mut sandbox = Sandbox::new_default(acc.clone());

        let amounts = (
            new_amount(500_000_000_000_000_000_000_000_u128),
            new_amount(500_000_000_000_000_000_000_000_u128),
        );

        sandbox.call_mut(|dex| dex.register_account()).unwrap();

        let token_with_lower_id = new_token_id();
        let token_with_higher_id = new_token_id();
        let (token_0, token_1) =
            swap_tokens(swap_token_ids, &(token_with_lower_id, token_with_higher_id));

        sandbox
            .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1]))
            .unwrap();

        sandbox
            .call_mut(|dex| dex.deposit(&acc, &token_0, amounts.0))
            .unwrap();
        sandbox
            .call_mut(|dex| dex.deposit(&acc, &token_1, amounts.1))
            .unwrap();

        let amount_a = 1_000_000_000_000_u128;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let amount_b = ((amount_a as f64 / price).max(1.)) as u128;

        let mut this = TestContext {
            state: sandbox,
            tokens: (token_0, token_1),
        };

        this.open_position(
            fee_rate,
            amount_a.into(),
            amount_b.into(),
            Tick::MIN,
            Tick::MAX,
        )
        .unwrap();

        this
    }

    fn open_position(
        &mut self,
        fee_rate: BasisPoints,
        max_left: Amount,
        max_right: Amount,
        tick_low: Tick,
        tick_high: Tick,
    ) -> Result<(PositionId, Amount, Amount, Liquidity)> {
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

    #[allow(unused)]
    fn close_position(&mut self, position_id: PositionId) -> Result<()> {
        self.state.call_mut(|dex| dex.close_position(position_id))
    }

    #[allow(unused)]
    fn get_pool_info(&self) -> Option<PoolInfo> {
        self.state
            .call(|dex| dex.get_pool_info(self.tokens.clone()).unwrap())
    }

    fn estimate_liq_add(
        &self,
        tokens: (TokenId, TokenId),
        fee_rate: BasisPoints,
        ticks_range: (Option<i32>, Option<i32>),
        amount_a: Option<Amount>,
        amount_b: Option<Amount>,
        user_price: Option<Float>,
        slippage_tolerance_bp: BasisPoints,
    ) -> Result<EstimateAddLiquidityResult> {
        self.state.call(|dex| {
            dex.estimate_liq_add(
                tokens,
                fee_rate,
                ticks_range,
                amount_a,
                amount_b,
                user_price,
                slippage_tolerance_bp,
            )
        })
    }
}

fn swap_tokens(swap: bool, (token1, token2): &(TokenId, TokenId)) -> (TokenId, TokenId) {
    if swap {
        (token2.clone(), token1.clone())
    } else {
        (token1.clone(), token2.clone())
    }
}

#[test]
fn basic_liquidity() {
    let context = TestContext::new();

    assert_matches!(
        context.estimate_liq_add(
            context.tokens.clone(),
            1,
            (None, None),
            Some(Amount::from(1_u128)),
            Some(Amount::from(9_172_981_341_412_974_812_937_u128)),
            None,
            3
        ),
        Ok(_)
    );
}

#[rstest]
fn from_nones_fails(
    #[values(false, true)] swap_token_ids: bool,
    #[values(1, 2, 16, 128)] fee_rate: BasisPoints,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
) {
    let context = TestContext::new();

    let result = context.estimate_liq_add(
        swap_tokens(swap_token_ids, &context.tokens),
        fee_rate,
        (None, None),
        None,
        None,
        None,
        slippage_tolerance,
    );
    assert_matches!(result, Err(_));
}

#[rstest]
fn from_amount_a_fails(
    #[values(false, true)] swap_token_ids: bool,
    #[values(1, 2, 16, 128)] fee_rate: BasisPoints,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(1_u128.into(), 17_u128.into(), 27_267_623_u128.into(), 9_172_981_341_412_974_812_937_u128.into())]
    amount_a: Amount,
) {
    let context = TestContext::new();

    let result = context.estimate_liq_add(
        swap_tokens(swap_token_ids, &context.tokens),
        fee_rate,
        (None, None),
        Some(amount_a),
        None,
        None,
        slippage_tolerance,
    );
    assert_matches!(result, Err(_));
}

#[rstest]
fn from_amount_b_fails(
    #[values(false, true)] swap_token_ids: bool,
    #[values(1, 2, 16, 128)] fee_rate: BasisPoints,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(1_u128.into(), 17_u128.into(), 27_267_623_u128.into(), 9_172_981_341_412_974_812_937_u128.into())]
    amount_b: Amount,
) {
    let context = TestContext::new();

    let result = context.estimate_liq_add(
        swap_tokens(swap_token_ids, &context.tokens),
        fee_rate,
        (None, None),
        None,
        Some(amount_b),
        None,
        slippage_tolerance,
    );
    assert_matches!(result, Err(_));
}

#[rstest]
fn from_two_amounts_success(
    #[values(false, true)] swap_token_ids: bool,
    #[values(1, 2, 16, 128)] fee_rate: BasisPoints,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(1_u128.into(), 17_u128.into(), 27_267_623_u128.into(), 9_172_981_341_412_974_812_937_u128.into())]
    amount_a: Amount,
    #[values(1_u128.into(), 17_u128.into(), 27_267_623_u128.into(), 9_172_981_341_412_974_812_937_u128.into())]
    amount_b: Amount,
) {
    let context = TestContext::new();

    let result = context.estimate_liq_add(
        swap_tokens(swap_token_ids, &context.tokens),
        fee_rate,
        (None, None),
        Some(amount_a),
        Some(amount_b),
        None,
        slippage_tolerance,
    );
    assert_matches!(result, Ok(_));
}

#[rstest]
fn from_amount_a_and_price_success(
    #[values(false, true)] swap_token_ids: bool,
    #[values(1, 2, 16, 128)] fee_rate: BasisPoints,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values((1e-5, 1_u128.into()), (3e5, 372_982_934_u128.into()), (1e15, 917_213_414_974_812_937_u128.into()) )]
    spot_price_and_amount_a: (f64, Amount),
) {
    let context = TestContext::new();

    let (spot_price, amount_a) = spot_price_and_amount_a;
    let result = context.estimate_liq_add(
        swap_tokens(swap_token_ids, &context.tokens),
        fee_rate,
        (None, None),
        Some(amount_a),
        None,
        Some(spot_price.into()),
        slippage_tolerance,
    );
    assert_matches!(result, Ok(_));
}

#[rstest]
fn from_amount_b_and_price_success(
    #[values(false, true)] swap_token_ids: bool,
    #[values(1, 2, 16, 128)] fee_rate: BasisPoints,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values((1e5, 1_u128.into()), (3e-5, 372_982_934_u128.into()), (1e-15, 917_213_414_974_812_937_u128.into()) )]
    spot_price_and_amount_b: (f64, Amount),
) {
    let context = TestContext::new();

    let (spot_price, amount_b) = spot_price_and_amount_b;
    let result = context.estimate_liq_add(
        swap_tokens(swap_token_ids, &context.tokens),
        fee_rate,
        (None, None),
        None,
        Some(amount_b),
        Some(spot_price.into()),
        slippage_tolerance,
    );
    assert_matches!(result, Ok(_));
}

#[rstest]
fn from_two_amounts_and_price_success(
    #[values(false, true)] swap_token_ids: bool,
    #[values(1, 2, 16, 128)] fee_rate: BasisPoints,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(1_u128.into(), 17_u128.into(), 27_267_623_u128.into(), 9_172_981_341_412_974_812_937_u128.into())]
    amount_a: Amount,
    #[values(1_u128.into(), 17_u128.into(), 27_267_623_u128.into(), 9_172_981_341_412_974_812_937_u128.into())]
    amount_b: Amount,
    #[values(1.17712e-10, 0.22, 1e3)] spot_price: f64,
) {
    let context = TestContext::new();

    let result = context.estimate_liq_add(
        swap_tokens(swap_token_ids, &context.tokens),
        fee_rate,
        (None, None),
        Some(amount_a),
        Some(amount_b),
        Some(spot_price.into()),
        slippage_tolerance,
    );
    assert_matches!(result, Ok(_));
}

#[rstest]
fn from_left_amount_success(
    #[values(false, true)] swap_token_ids: bool,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(
    (1_u128.into(), 3.2e-10),
    (231_u128.into(), 0.283),
    (32723_u128.into(), 5.22e12),
    (3_272_372_314_662_134_612_u128.into(), 5.22e-3),
    (917_213_414_974_812_937_u128.into(), 7.3e12),
    )]
    amount_a_and_price: (Amount, f64),
) {
    let (amount_a, price) = amount_a_and_price;
    let context = TestContext::new_with_price(price, 16, swap_token_ids);

    let result = context.estimate_liq_add(
        context.tokens.clone(),
        16,
        (None, None),
        Some(amount_a),
        None,
        None,
        slippage_tolerance,
    );
    assert_matches!(result, Ok(_));
}

#[rstest]
fn from_right_amount_success(
    #[values(false, true)] swap_token_ids: bool,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(
    (1_u128.into(), 3.8e10),
    (231_u128.into(), 283.),
    (2347_u128.into(), 7.33e-12),
    (917_213_414_974_812_937_u128.into(), 7.11e-12)
    )]
    amount_b_and_price: (Amount, f64),
) {
    let (amount_b, price) = amount_b_and_price;
    let context = TestContext::new_with_price(price, 16, swap_token_ids);

    let result = context.estimate_liq_add(
        context.tokens.clone(),
        16,
        (None, None),
        None,
        Some(amount_b),
        None,
        slippage_tolerance,
    );
    assert_matches!(result, Ok(_));
}

#[rstest]
fn from_left_amount_and_price_fails(
    #[values(false, true)] swap_token_ids: bool,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(
    (1_u128.into(), 3e-10),
    (231_u128.into(), 0.283),
    (917_213_414_974_812_937_u128.into(), 7e12)
    )]
    amount_a_and_price: (Amount, f64),
) {
    let (amount_a, price) = amount_a_and_price;
    let context = TestContext::new_with_price(price, 16, swap_token_ids);

    let result = context.estimate_liq_add(
        context.tokens.clone(),
        16,
        (None, None),
        Some(amount_a),
        None,
        Some(price.into()),
        slippage_tolerance,
    );
    assert_matches!(result, Err(_));
}

#[rstest]
fn from_right_amount_and_price_fails(
    #[values(false, true)] swap_token_ids: bool,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(false, true)] position_price_same_as_initial: bool,
) {
    let initial_price = 1000.;
    let position_price = if position_price_same_as_initial {
        initial_price
    } else {
        initial_price * 0.001
    };
    let context = TestContext::new_with_price(initial_price, 16, swap_token_ids);

    let result = context.estimate_liq_add(
        context.tokens.clone(),
        16,
        (None, None),
        None,
        Some(1_000_000u128.into()),
        Some(position_price.into()),
        slippage_tolerance,
    );
    assert_matches!(result, Err(_));
}

#[rstest]
fn from_two_amounts_success_existing(
    #[values(false, true)] swap_token_ids: bool,
    #[values(0, 3, 20, 99)] slippage_tolerance: BasisPoints,
    #[values(
    (1.727e-10, 1.232),
    (0.0003, 0.0001),
    (0.0003, 0.0003),
    (0.0003, 0.007),
    (237_324., 21347.),
    (237_324., 2_231_347.),
    (7e12, 7e3),
    (7e12, 7e11),
    (7e12, 7e13),
    )]
    initial_price_and_position_amount_ratio: (f64, f64),
    #[values(237_412_364_128_769_213_478_u128, 10000_u128)] amount_a: u128,
) {
    let (initial_price, position_amount_ratio) = initial_price_and_position_amount_ratio;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let amount_b = (amount_a as f64 / position_amount_ratio).max(1.);
    let context = TestContext::new_with_price(initial_price, 16, swap_token_ids);

    let result = context.estimate_liq_add(
        context.tokens.clone(),
        16,
        (None, None),
        Some(amount_a.into()),
        Some((amount_b as u128).into()),
        None,
        slippage_tolerance,
    );
    assert_matches!(result, Ok(_));
}
