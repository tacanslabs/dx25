// Some of the conversions are useless, because NEAR Amount is u128,
// which is not the same for other DEX's
#![allow(clippy::useless_conversion)]

use super::dex;
use crate::chain::{AccountId, Amount, TokenId};
use crate::dex::SwapToPriceAction;
use crate::Float;
use assert_matches::assert_matches;
use dex::test_utils::{
    new_account_id, new_amount, new_token_id, BalanceTracker, Change, Sandbox, SwapTestContext,
};
use dex::utils::swap_if;
use dex::{
    Account, Dex, Error, ErrorKind, Map as _, Result, StateMembersMut, StateMut, SwapAction,
    SwapKind, Types,
};
use std::borrow::BorrowMut;

use rstest::rstest;

#[allow(clippy::too_many_arguments)]
fn dex_execute_swap_action<T: Types, S: StateMut<T>, SS: BorrowMut<S>>(
    dex: &mut Dex<T, S, SS>,
    account_id: &AccountId,
    prev_swap_result: &Option<(TokenId, SwapKind, Amount)>,
    exact: SwapKind,
    token_in: &TokenId,
    token_out: &TokenId,
    amount: Option<Amount>,
    amount_limit: Amount,
) -> Result<(TokenId, SwapKind, Amount)> {
    let StateMembersMut {
        contract, logger, ..
    } = dex.members_mut();
    let contract = contract.latest();
    contract
        .accounts
        .update(account_id, |Account::V0(ref mut account)| {
            Dex::<T, S, SS>::execute_swap_action(
                account_id,
                account,
                &mut contract.pools,
                logger,
                prev_swap_result,
                exact,
                SwapAction {
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    amount: amount.map(Into::into),
                    amount_limit: amount_limit.into(),
                },
                contract.protocol_fee_fraction,
            )
        })
        .unwrap() // Not intended for checking here
}

#[allow(clippy::too_many_arguments)]
fn dex_execute_swap_to_price_action<T: Types, S: StateMut<T>, SS: BorrowMut<S>>(
    dex: &mut Dex<T, S, SS>,
    account_id: &AccountId,
    prev_swap_result: &Option<(TokenId, SwapKind, Amount)>,
    token_in: &TokenId,
    token_out: &TokenId,
    amount: Option<Amount>,
    effective_price_limit: Float,
) -> Result<(TokenId, SwapKind, Amount)> {
    let StateMembersMut {
        contract, logger, ..
    } = dex.members_mut();
    let contract = contract.latest();
    contract
        .accounts
        .update(account_id, |Account::V0(ref mut account)| {
            Dex::<T, S, SS>::execute_swap_to_price_action(
                account_id,
                account,
                &mut contract.pools,
                logger,
                prev_swap_result,
                SwapToPriceAction {
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    amount: amount.map(Into::into),
                    effective_price_limit,
                },
                contract.protocol_fee_fraction,
            )
        })
        .unwrap() // Not intended for checking here
}

/// Ensure failure if input token isn't registered
#[rstest]
fn fail_token_in_not_registered(
    #[values(
        &None,
        &Some((new_token_id(), SwapKind::ExactIn, new_amount(10))),
        &Some((new_token_id(), SwapKind::ExactOut, new_amount(10))),
    )]
    prev_swap_result: &Option<(TokenId, SwapKind, Amount)>,
    #[values(SwapKind::ExactIn, SwapKind::ExactOut)] exact: SwapKind,
    #[values(None, Some(1_000))] amount: Option<u128>,
) {
    let mut sandbox = Sandbox::new_default(new_account_id());
    let account_id = sandbox.caller_id().clone();
    let token_in = new_token_id();
    let token_out = new_token_id();

    sandbox.call_mut(|dex| dex.register_account()).unwrap();
    // No tokens registered
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &account_id,
            prev_swap_result,
            exact,
            &token_in,
            &token_out,
            amount.map(Into::into),
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );

    sandbox
        .call_mut(|dex| dex.register_tokens(&account_id, [&token_out]))
        .unwrap();
    // `token_out` registered
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &account_id,
            prev_swap_result,
            exact,
            &token_in,
            &token_out,
            amount.map(Into::into),
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
}
/// Ensure failure if output token isn't registered
#[rstest]
fn fail_token_out_not_registered(
    #[values(
        &None,
        &Some((new_token_id(), SwapKind::ExactIn, new_amount(10))),
        &Some((new_token_id(), SwapKind::ExactOut, new_amount(10))),
    )]
    prev_swap_result: &Option<(TokenId, SwapKind, Amount)>,
    #[values(SwapKind::ExactIn, SwapKind::ExactOut)] exact: SwapKind,
    #[values(None, Some(1_000))] amount: Option<u128>,
) {
    let mut sandbox = Sandbox::new_default(new_account_id());
    let account_id = sandbox.caller_id().clone();
    let token_in = new_token_id();
    let token_out = new_token_id();

    sandbox.call_mut(|dex| dex.register_account()).unwrap();
    // No tokens registered
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &account_id,
            prev_swap_result,
            exact,
            &token_in,
            &token_out,
            amount.map(Into::into),
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );

    sandbox
        .call_mut(|dex| dex.register_tokens(&account_id, [&token_in]))
        .unwrap();
    // `token_in` registered
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &account_id,
            prev_swap_result,
            exact,
            &token_in,
            &token_out,
            amount.map(Into::into),
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
}

#[rstest]
fn fail_prev_swap_result(#[values(SwapKind::ExactIn, SwapKind::ExactOut)] exact: SwapKind) {
    let SwapTestContext {
        mut sandbox,
        owner,
        token_ids,
        ..
    } = SwapTestContext::new();
    // no result at all
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &owner,
            &None,
            exact,
            &token_ids.0,
            &token_ids.1,
            None,
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );
    // `exact` does not match
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &owner,
            &Some((token_ids.0.clone(), exact.opposite(), new_amount(1_000))),
            exact,
            &token_ids.0,
            &token_ids.1,
            None,
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &owner,
            &Some((token_ids.1.clone(), exact.opposite(), new_amount(1_000))),
            exact,
            &token_ids.0,
            &token_ids.1,
            None,
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );
    // `exact` matches but `prev_token_id` doesn't
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &owner,
            &Some((
                // if exact is in, prev token should be the same as current in,
                // so we do the opposite
                match exact {
                    SwapKind::ExactIn | SwapKind::ToPrice => &token_ids.1,
                    SwapKind::ExactOut => &token_ids.0,
                }
                .clone(),
                exact,
                new_amount(1_000)
            )),
            exact,
            &token_ids.0,
            &token_ids.1,
            None,
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );
    // `exact` matches but `prev_token_id` is some arbitrary
    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &owner,
            &Some((new_token_id(), exact, new_amount(1_000))),
            exact,
            &token_ids.0,
            &token_ids.1,
            None,
            new_amount(5_000),
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );
}

#[rstest]
fn fail_duplicate_tokens(#[values(SwapKind::ExactIn, SwapKind::ExactOut)] exact: SwapKind) {
    let SwapTestContext {
        mut sandbox,
        owner,
        token_ids,
        ..
    } = SwapTestContext::new();
    let other_token = new_token_id();

    sandbox
        .call_mut(|dex| dex.register_tokens(&owner, [&other_token]))
        .unwrap();
    for t in [&token_ids.0, &token_ids.1, &other_token] {
        assert_matches!(
            sandbox.call_mut(|dex| dex_execute_swap_action(
                dex,
                &owner,
                &None,
                exact,
                t,
                t,
                Some(new_amount(5_000)),
                new_amount(5_000),
            )),
            Err(Error {
                kind: ErrorKind::TokenDuplicates,
                ..
            })
        );
    }
}

#[test]
fn fail_pool_not_registered() {
    let SwapTestContext {
        mut sandbox,
        owner,
        token_ids,
        ..
    } = SwapTestContext::new();
    let other_tokens = (new_token_id(), new_token_id());

    sandbox
        .call_mut(|dex| dex.register_tokens(&owner, [&other_tokens.0, &other_tokens.1]))
        .unwrap();

    for (t0, t1) in [
        (&token_ids.0, &other_tokens.0),
        (&token_ids.0, &other_tokens.1),
        (&token_ids.1, &other_tokens.0),
        (&token_ids.1, &other_tokens.1),
        (&other_tokens.0, &other_tokens.1),
    ] {
        assert_matches!(
            sandbox.call_mut(|dex| dex_execute_swap_action(
                dex,
                &owner,
                &None,
                SwapKind::ExactIn,
                t0,
                t1,
                Some(new_amount(5_000)),
                new_amount(5_000),
            )),
            Err(Error {
                kind: ErrorKind::PoolNotRegistered,
                ..
            })
        );
        assert_matches!(
            sandbox.call_mut(|dex| dex_execute_swap_action(
                dex,
                &owner,
                &None,
                SwapKind::ExactIn,
                t1,
                t0,
                Some(new_amount(5_000)),
                new_amount(5_000),
            )),
            Err(Error {
                kind: ErrorKind::PoolNotRegistered,
                ..
            })
        );
    }
}

#[rstest]
fn fail_slippage(
    #[values(false, true)] use_prev_result: bool,
    #[values(SwapKind::ExactIn, SwapKind::ExactOut)] exact: SwapKind,
    #[values(200, 1_000, 5_000)] amount: u128, // turned into `Amount` on spot
) {
    let SwapTestContext {
        mut sandbox,
        owner,
        token_ids: (token_0, token_1),
        ..
    } = SwapTestContext::new_all_1g();

    let amount = new_amount(amount);
    let prev_token = if exact == SwapKind::ExactIn {
        token_0.clone()
    } else {
        token_1.clone()
    };

    assert_matches!(
        sandbox.call_mut(|dex| dex_execute_swap_action(
            dex,
            &owner,
            &if use_prev_result {
                Some((prev_token.clone(), exact, amount))
            } else {
                None
            },
            exact,
            &token_0,
            &token_1,
            if use_prev_result { None } else { Some(amount) },
            amount, // Output amount cannot be equal input one on equal pool
        )),
        Err(Error {
            kind: ErrorKind::Slippage,
            ..
        })
    );
}

#[rstest]
fn success(
    #[values(SwapKind::ExactIn, SwapKind::ExactOut)] exact: SwapKind,
    #[values(200, 1_000, 5_000)] amount: u128, // turned into `Amount` on spot
) {
    let do_swap_check = |use_prev_result: bool| -> Amount {
        let SwapTestContext {
            mut sandbox,
            owner,
            token_ids: (token_0, token_1),
            ..
        } = SwapTestContext::new_all_1g();

        let (prev_token, next_token, amount, amount_limit, amount_range) =
            if exact == SwapKind::ExactIn {
                let amount_limit = new_amount(amount / 2);
                let amount = new_amount(amount);
                (
                    token_0.clone(),
                    token_1.clone(),
                    amount,
                    amount_limit,
                    amount_limit..=amount,
                )
            } else {
                let amount_limit = new_amount(amount * 2);
                let amount = new_amount(amount);
                (
                    token_1.clone(),
                    token_0.clone(),
                    amount,
                    amount_limit,
                    amount..=amount_limit,
                )
            };

        let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_0, &token_1]);

        let (out_token_id, out_exact, out_amount) = assert_matches!(sandbox.call_mut(|dex| dex_execute_swap_action(
                dex,
                &owner,
                &if use_prev_result {
                    Some((prev_token.clone(), exact, amount))
                }
                else {
                    None
                },
                exact,
                &token_0,
                &token_1,
                if use_prev_result { None } else { Some(amount) },
                amount_limit,
            )),
            Ok((out_token_id, out_exact, out_amount)) => (out_token_id, out_exact, out_amount)
        );

        assert_eq!(next_token, out_token_id);
        assert_eq!(exact, out_exact);
        assert!(amount_range.contains(&out_amount));

        let (amount_0, amount_1) = swap_if(exact == SwapKind::ExactOut, (amount, out_amount));

        bal_track.assert_changes(&sandbox, [Change::Dec(amount_0), Change::Inc(amount_1)]);

        out_amount
    };

    let direct_amount = do_swap_check(false);
    let prev_result_amount = do_swap_check(true);

    assert_eq!(direct_amount, prev_result_amount);
}

#[rstest]
fn success_to_price(
    #[values(true, false)] direction: bool,
    #[values(200, 10_000, 500_000)] amount: u128, // limit, result
) {
    let do_swap_check = |use_prev_result: bool| -> Amount {
        let SwapTestContext {
            mut sandbox,
            owner,
            token_ids: (token_0, token_1),
            ..
        } = SwapTestContext::new_all_1g();

        let amount: Amount = amount.into();
        let (first_token, second_token) = if direction {
            (token_0, token_1)
        } else {
            (token_1, token_0)
        };

        let bal_track = BalanceTracker::new_with_caller(&sandbox, [&first_token, &second_token]);

        let (out_token_id, _, out_amount) = assert_matches!(sandbox.call_mut(|dex| dex_execute_swap_to_price_action(
                dex,
                &owner,
                &if use_prev_result {
                    Some((second_token.clone(), SwapKind::ExactIn, amount))
                }
                else {
                    None
                },
                &first_token.clone(),
                &second_token.clone(),
                Some(amount),
                1.5.into()
            )),
            Ok((out_token_id, out_exact, out_amount)) => (out_token_id, out_exact, out_amount)
        );

        assert_eq!(second_token, out_token_id);

        bal_track.assert_changes(&sandbox, [Change::Dec(amount), Change::Inc(out_amount)]);

        out_amount
    };

    let _: Amount = do_swap_check(false);
}
