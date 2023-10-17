//! Depends on:
//! * [ ] `AccountLatest::register_tokens`, never fails
//! * [ ] `Dex::register_account_and_then`
//! * [x] `Dex::execute_swap_action`
//! * [ ] `Dex::deposit_impl`
//! * [ ] `Dex::withdraw_impl`
//! * [ ] `Dex::open_position_impl`
//! * [ ] `Dex::close_position_impl`
//! * [ ] `Dex::withdraw_fee_impl`
//!
//! Failures:
//! * [x] Fail if `RegisterAccount` is requested and callback fails
//! * [x] Fail if `RegisterAccount` isn't first in batch
//! * [x] Fail if account not registered
//! * [x] Fail if `SwapIn` or `SwapOut` fails, on swap-in or swap-out
//! * [x] Fail if `Deposit` action in batch but without data
//! * [x] Fail if `Deposit` data specified but action not in batch; includes empty batch
//! * [x] Fail if `Deposit` encountered more than once
//! * [x] Fail if `Withdraw` fails
//! * [x] Fail if `OpenPosition` fails
//! * [x] Fail if `ClosePosition` fails
//! * [x] Fail if `WithdrawFee` fails
//! * [x] Fail if swap chain breaks but next swap doesn't specify amount; this includes first swap
//!
//! Successes:
//! * [x] Empty batch, no deposit
//! * [x] Just deposit, with deposit data
//! * [x] Simple register account
//! * [x] Simple swap in/swap out
//! * [x] Two swap ins, in succession
//! * [x] Two swap outs, in succession
//! * [x] Base swap in and swap out scenarios
//! * [x] Base open position scenario; requires two executions
//! * [x] Base close position scenario
//! * [x] Base withdraw fee scenario
//!
//! For each success, check balances and ensure number of results equals number of actions
//!
//! Additional notes:
//! * Doesn't perform mutable API check or initiator check, these are performed by public methods
use super::super::ActionResult;
use super::dex;
use crate::chain::AccountId;
use crate::dex::{DepositPayment, State};
use crate::{assert_any_matches, error_here};
use assert_matches::assert_matches;
use dex::{
    test_utils, Action, Error, ErrorKind, PoolId, PoolUpdateReason, PositionInit, Range, SwapAction,
};
use rstest::rstest;
use test_utils::{
    new_account_id, new_amount, new_token_id, BalanceTracker, Change, Event, SwapTestContext,
};

#[allow(clippy::unnecessary_wraps)] // Expected - func is a stub for register account callback
fn its_ok<T: dex::Types>(
    _id: &AccountId,
    _acc: &mut dex::Account<T>,
    _ex: bool,
) -> dex::Result<()> {
    Ok(())
}

//
// Failures
//

#[test]
fn fail_register_account_callback_fails() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut |_, _, _| Err(error_here!(ErrorKind::InvalidParams)), // use InvalidParams as just stub error
            vec![Action::RegisterAccount, Action::Deposit]
        )),
        Err(Error {
            kind: ErrorKind::InvalidParams,
            ..
        })
    );
}

#[test]
fn fail_register_account_not_first() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut its_ok,
            vec![Action::Deposit, Action::RegisterAccount]
        )),
        Err(Error {
            kind: ErrorKind::UnexpectedRegisterAccount,
            ..
        })
    );
}

#[test]
#[cfg(feature = "near")]
fn fail_account_not_registered() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        ..
    } = SwapTestContext::new();

    let one = new_amount(1);
    let amount_in = new_amount(5_000);

    let acc2 = new_account_id();

    sandbox.set_initiator_id(acc2.clone());

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &acc2,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut its_ok,
            vec![Action::Deposit]
        )),
        Err(Error {
            kind: ErrorKind::AccountNotRegistered,
            ..
        })
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(&acc2, &[], &mut its_ok, vec![])),
        Err(Error {
            kind: ErrorKind::AccountNotRegistered,
            ..
        })
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &acc2,
            &[],
            &mut its_ok,
            vec![Action::SwapExactIn(SwapAction {
                token_in: token_ids.0.clone(),
                token_out: token_ids.1.clone(),
                amount: Some(amount_in.into()),
                amount_limit: one.into(),
            })]
        )),
        Err(Error {
            kind: ErrorKind::AccountNotRegistered,
            ..
        })
    );
}

#[test]
#[cfg(feature = "near")]
fn fail_swap_in() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new_all_1g();
    // Prepare, drop token 1 from account
    sandbox
        .call_mut(|dex| dex.withdraw(&owner, &token_ids.1, new_amount(0), true, ()))
        .unwrap();
    // No deposit
    let bt = BalanceTracker::new(&sandbox, &owner, [&token_ids.0, &token_ids.1]);
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::SwapExactIn(SwapAction {
                token_in: token_ids.0.clone(),
                token_out: token_ids.1.clone(),
                amount: Some(new_amount(1_000).into()),
                amount_limit: new_amount(500).into(),
            })]
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
    bt.assert_changes(&sandbox, [Change::NoChangeExact, Change::NoChangeExact]);
    // With deposit
    let bt = BalanceTracker::new(&sandbox, &owner, [&token_ids.0, &token_ids.1]);
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000_000),
            }],
            &mut its_ok,
            vec![
                Action::SwapExactIn(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(new_amount(1_000).into()),
                    amount_limit: new_amount(500).into(),
                }),
                Action::Deposit
            ]
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
    bt.assert_changes(&sandbox, [Change::NoChangeExact, Change::NoChangeExact]);
}

#[test]
#[cfg(feature = "near")]
fn fail_swap_out() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new_all_1g();
    // Prepare, drop token 1 from account
    sandbox
        .call_mut(|dex| dex.withdraw(&owner, &token_ids.1, new_amount(0), true, ()))
        .unwrap();
    // No deposit
    let bt = BalanceTracker::new(&sandbox, &owner, [&token_ids.0, &token_ids.1]);
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::SwapExactOut(SwapAction {
                token_in: token_ids.0.clone(),
                token_out: token_ids.1.clone(),
                amount: Some(new_amount(1_000).into()),
                amount_limit: new_amount(500).into(),
            })]
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
    bt.assert_changes(&sandbox, [Change::NoChangeExact, Change::NoChangeExact]);
    // With deposit
    let bt = BalanceTracker::new(&sandbox, &owner, [&token_ids.0, &token_ids.1]);
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000_000),
            }],
            &mut its_ok,
            vec![
                Action::SwapExactOut(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(new_amount(1_000).into()),
                    amount_limit: new_amount(500).into(),
                }),
                Action::Deposit
            ]
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
    bt.assert_changes(&sandbox, [Change::NoChangeExact, Change::NoChangeExact]);
}

#[test]
fn fail_deposit_action_no_data() {
    let SwapTestContext {
        mut sandbox, owner, ..
    } = SwapTestContext::new_all_1g();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::Deposit]
        )),
        Err(Error {
            kind: ErrorKind::DepositNotAllowed,
            ..
        })
    );
}

#[test]
fn fail_deposit_data_no_action() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new_all_1g();
    // Empty batch
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000_000),
            }],
            &mut its_ok,
            vec![]
        )),
        Err(Error {
            kind: ErrorKind::DepositNotHandled,
            ..
        })
    );
    // Some actions but no deposit
    let tok2 = new_token_id();
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000_000),
            }],
            &mut its_ok,
            vec![Action::RegisterTokens(vec![tok2])]
        )),
        Err(Error {
            kind: ErrorKind::DepositNotHandled,
            ..
        })
    );
}

#[test]
fn fail_deposit_twice() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut its_ok,
            vec![Action::Deposit, Action::Deposit]
        )),
        Err(Error {
            kind: ErrorKind::DepositAlreadyHandled,
            ..
        })
    );
}

#[test]
fn success_zero_withdraw() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();
    let tok2 = new_token_id();
    // Succeeds even if token isn't registered
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::Withdraw(tok2.clone(), new_amount(0).into(), ())]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::Withdraw(None)
        ])
    );
    // Check zero withdraw for zero balance
    sandbox
        .call_mut(|dex| dex.register_tokens(&owner, [&tok2]))
        .unwrap();
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::Withdraw(tok2.clone(), new_amount(0).into(), ())]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::Withdraw(None)
        ])
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut its_ok,
            vec![
                Action::Deposit,
                Action::Withdraw(tok2, new_amount(0).into(), ())
            ]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::Deposit,
            ActionResult::Withdraw(None)
        ])
    );
}

#[test]
fn fail_withdraw() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();
    let tok2 = new_token_id();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut its_ok,
            vec![
                Action::Deposit,
                Action::Withdraw(tok2, new_amount(1_000).into(), ())
            ]
        )),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
}

#[test]
fn fail_open_position() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new_all_1g();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::OpenPosition {
                tokens: (token_ids.0.clone(), token_ids.0.clone()),
                fee_rate: 1,
                position: PositionInit {
                    amount_ranges: (
                        Range {
                            min: new_amount(1).into(),
                            max: new_amount(1_000_000).into()
                        },
                        Range {
                            min: new_amount(1).into(),
                            max: new_amount(1_000_000).into()
                        },
                    ),
                    ticks_range: (None, None)
                }
            }]
        )),
        Err(Error {
            kind: ErrorKind::TokenDuplicates,
            ..
        })
    );
}

#[test]
fn fail_close_position() {
    let SwapTestContext {
        mut sandbox, owner, ..
    } = SwapTestContext::new_all_1g();

    assert_matches!(
        sandbox.call_mut(|dex| {
            let invalid_id = dex.contract().as_ref().next_free_position_id + 1;
            dex.execute_actions_impl(
                &owner,
                &[],
                &mut its_ok,
                vec![Action::ClosePosition(invalid_id)],
            )
        }),
        Err(Error {
            kind: ErrorKind::PositionDoesNotExist,
            ..
        })
    );
}

#[test]
fn fail_withdraw_fee() {
    let SwapTestContext {
        mut sandbox, owner, ..
    } = SwapTestContext::new_all_1g();

    assert_matches!(
        sandbox.call_mut(|dex| {
            let invalid_id = dex.contract().as_ref().next_free_position_id + 1;
            dex.execute_actions_impl(
                &owner,
                &[],
                &mut its_ok,
                vec![Action::WithdrawFee(invalid_id)],
            )
        }),
        Err(Error {
            kind: ErrorKind::PositionDoesNotExist,
            ..
        })
    );
}

#[test]
fn fail_no_start_amount() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new_all_1g();
    // Single action with no amount
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::SwapExactIn(SwapAction {
                token_in: token_ids.0.clone(),
                token_out: token_ids.1.clone(),
                amount: None,
                amount_limit: new_amount(5_000).into(),
            })]
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![Action::SwapExactOut(SwapAction {
                token_in: token_ids.0.clone(),
                token_out: token_ids.1.clone(),
                amount: None,
                amount_limit: new_amount(5_000).into(),
            })]
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );
    // Two actions, second one doesn't properly continue first one
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![
                Action::SwapExactIn(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(new_amount(10_000).into()),
                    amount_limit: new_amount(5_000).into(),
                }),
                Action::SwapExactIn(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: None,
                    amount_limit: new_amount(5_000).into(),
                })
            ]
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions(
            &mut its_ok,
            vec![
                Action::SwapExactOut(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(new_amount(2_500).into()),
                    amount_limit: new_amount(5_000).into(),
                }),
                Action::SwapExactOut(SwapAction {
                    token_in: token_ids.0,
                    token_out: token_ids.1,
                    amount: None,
                    amount_limit: new_amount(5_000).into(),
                })
            ]
        )),
        Err(Error {
            kind: ErrorKind::WrongActionResult,
            ..
        })
    );
}

//
// Successes
//

#[test]
fn success_no_deposit_empty_batch() {
    let SwapTestContext {
        mut sandbox,
        token_ids: _,
        owner,
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![]
        )),
        Ok(v) if matches!(&v[..], &[])
    );
}

#[test]
fn success_just_deposit() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();

    let amount = new_amount(1_000);
    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_ids.0, &token_ids.1]);

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount,
            }],
            &mut its_ok,
            vec![Action::Deposit]
        )),
        Ok(v) if matches!(&v[..], &[ActionResult::Deposit])
    );

    bal_track.assert_changes(&sandbox, [Change::FromLogs, Change::NoChange]);
}

#[test]
fn success_multiple_deposit() {
    let SwapTestContext {
        mut sandbox,
        token_ids: (token_id1, token_id2),
        owner,
        ..
    } = SwapTestContext::new();

    let amount1 = new_amount(1_000);
    let amount2 = new_amount(1_001);
    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_id1, &token_id2]);

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_id1.clone(),
                amount: amount1,
            },
            DepositPayment {
                token_id: token_id2.clone(),
                amount: amount2,
            }],
            &mut its_ok,
            vec![Action::Deposit]
        )),
        Ok(v) if matches!(&v[..], &[ActionResult::Deposit])
    );

    #[allow(clippy::useless_conversion)] // Clippy complains sometimes on VEAX
    bal_track.assert_changes(
        &sandbox,
        [Change::Inc(1000u128.into()), Change::Inc(1001u128.into())],
    );
}

#[test]
fn success_register_account() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();

    let amount = new_amount(1_000);
    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_ids.0, &token_ids.1]);

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount,
            }],
            &mut its_ok,
            vec![
                Action::RegisterAccount,
                Action::Deposit,
            ]
        )),
        Ok(v) if matches!(&v[..], &[ActionResult::RegisterAccount, ActionResult::Deposit])
    );

    bal_track.assert_changes(&sandbox, [Change::FromLogs, Change::NoChange]);
}

#[rstest]
fn success_single_swap_in(#[values(200, 1_000, 5_000)] amount: u128) {
    let SwapTestContext {
        mut sandbox,
        owner,
        token_ids,
        ..
    } = SwapTestContext::new_all_1g();

    let amount_limit = new_amount(amount / 2);
    let amount = new_amount(amount);

    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_ids.0, &token_ids.1]);

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount,
            }],
            &mut its_ok,
            vec![
            Action::Deposit,
            Action::SwapExactIn(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(amount.into()),
                    amount_limit: amount_limit.into(),
                }
            )])),
        Ok(v) if matches!(&v[..], &[ActionResult::Deposit, ActionResult::SwapExactIn(_)])
    );

    bal_track.assert_changes(&sandbox, [Change::NoChange, Change::FromLogs]);

    assert_any_matches!(
        sandbox.latest_logs(),
        Event::Swap { user, tokens, .. }
        if user == &owner
            && tokens == &token_ids
    );

    let pool_id = PoolId::try_from_pair(token_ids).unwrap().0;

    assert_any_matches!(
        sandbox.latest_logs(),
        Event::UpdatePoolState {
            reason,
            pool,
            ..
        }
        if reason == &PoolUpdateReason::Swap
            && PoolId::try_from_pair(pool.clone()).unwrap().0
                == pool_id
    );
}

#[rstest]
fn success_single_swap_out(#[values(200, 1_000, 5_000)] amount: u128) {
    let SwapTestContext {
        mut sandbox,
        owner,
        token_ids,
        ..
    } = SwapTestContext::new_all_1g();

    let amount_limit = new_amount(amount * 2);
    let amount = new_amount(amount);

    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_ids.0, &token_ids.1]);

    assert_matches!(
    sandbox.call_mut(|dex| dex.execute_actions_impl(
        &owner,
        &[DepositPayment {
            token_id: token_ids.0.clone(),
            amount,
        }],
        &mut its_ok,
        vec![
            Action::Deposit,
            Action::SwapExactOut(SwapAction {
                token_in: token_ids.0.clone(),
                token_out: token_ids.1.clone(),
                amount: Some(amount.into()),
                amount_limit: amount_limit.into(),
            })
        ])),
        Ok(v) if matches!(&v[..], &[
            ActionResult::Deposit, ActionResult::SwapExactOut(_)
        ])
    );

    bal_track.assert_changes(&sandbox, [Change::FromLogs, Change::FromLogs]);

    assert_any_matches!(
        sandbox.latest_logs(),
        Event::Swap { user, tokens, .. }
        if user == &owner
            && tokens == &token_ids
    );

    let pool_id = PoolId::try_from_pair(token_ids).unwrap().0;

    assert_any_matches!(
        sandbox.latest_logs(),
        Event::UpdatePoolState {
            reason,
            pool,
            ..
        }
        if reason == &PoolUpdateReason::Swap
            && PoolId::try_from_pair(pool.clone()).unwrap().0
                == pool_id
    );
}

#[rstest]
fn success_two_swap_ins_chain(#[values(200, 1_000, 5_000)] amount: u128) {
    let mut ctxt = SwapTestContext::new_all_1g();
    let token_2 = new_token_id();
    ctxt.open_position_1g((&ctxt.token_ids.1.clone(), &token_2));

    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        owner,
        ..
    } = ctxt;

    let amount_limit = new_amount(amount / 2);
    let amount = new_amount(amount);

    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_0, &token_1, &token_2]);

    assert_matches!(sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![
            Action::SwapExactIn(SwapAction {
                token_in: token_0.clone(),
                token_out: token_1.clone(),
                amount: Some(amount.into()),
                amount_limit: amount_limit.into(),
            }),
            Action::SwapExactIn(SwapAction {
                token_in: token_1.clone(),
                token_out: token_2.clone(),
                amount: None,
                amount_limit: amount_limit.into(),
            }),
        ])),
        Ok(v) if matches!(&v[..], &[
            ActionResult::SwapExactIn(_),
            ActionResult::SwapExactIn(_)
        ])
    );

    bal_track.assert_changes(
        &sandbox,
        [Change::FromLogs, Change::NoChangeExact, Change::FromLogs],
    );
}

#[rstest]
fn success_two_swap_outs_chain(#[values(200, 1_000, 5_000)] amount: u128) {
    let mut ctxt = SwapTestContext::new_all_1g();
    let token_2 = new_token_id();
    ctxt.open_position_1g((&ctxt.token_ids.1.clone(), &token_2));

    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        owner,
        ..
    } = ctxt;

    let amount_limit = new_amount(amount * 2);
    let amount = new_amount(amount);

    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_0, &token_1, &token_2]);

    assert_matches!(sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok, vec![
            Action::SwapExactOut(SwapAction {
                token_in: token_1.clone(),
                token_out: token_2.clone(),
                amount: Some(amount.into()),
                amount_limit: amount_limit.into(),
            }),
            Action::SwapExactOut(SwapAction {
                token_in: token_0.clone(),
                token_out: token_1.clone(),
                amount: None,
                amount_limit: amount_limit.into(),
            }),
        ])),
        Ok(v) if matches!(&v[..], &[ActionResult::SwapExactOut(_), ActionResult::SwapExactOut(_)])
    );

    bal_track.assert_changes(
        &sandbox,
        [Change::FromLogs, Change::NoChangeExact, Change::FromLogs],
    );
}

#[test]
fn success_base_scenario_in() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        ..
    } = SwapTestContext::new();
    let acc = new_account_id();

    let amount_in = 1_000_000;
    let min_amount_out = new_amount(amount_in / 2);
    let amount_in = new_amount(amount_in);

    let bal_track = BalanceTracker::new(&sandbox, &acc, [&token_ids.0, &token_ids.1]);

    sandbox.set_initiator_id(acc.clone());
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &acc,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: amount_in,
            }],
            &mut its_ok,
            vec![
                Action::RegisterAccount,
                Action::RegisterTokens(vec![token_ids.0.clone(), token_ids.1.clone()]),
                Action::Deposit,
                Action::SwapExactIn(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(amount_in.into()),
                    amount_limit: min_amount_out.into(),
                }),
                Action::Withdraw(token_ids.0.clone(), new_amount(0).into(), ()),
                Action::Withdraw(token_ids.1.clone(), new_amount(0).into(), ()),
            ]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::RegisterAccount,
            ActionResult::RegisterTokens,
            ActionResult::Deposit,
            ActionResult::SwapExactIn(_),
            ActionResult::Withdraw(_),
            ActionResult::Withdraw(_),
        ])
    );

    bal_track.assert_changes(
        &sandbox,
        // No changes - because all remnants are withdrawn
        [Change::NoChange, Change::NoChange],
    );
}

#[test]
fn success_base_scenario_out() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        ..
    } = SwapTestContext::new();
    let acc = new_account_id();

    let deposit_amount = new_amount(2_000_000);
    let amount_out = 1_000_000;
    let max_amount_in = new_amount(amount_out * 2);
    let amount_out = new_amount(amount_out);

    let bal_track = BalanceTracker::new(&sandbox, &acc, [&token_ids.0, &token_ids.1]);

    sandbox.set_initiator_id(acc.clone());
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &acc,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: deposit_amount,
            }],
            &mut its_ok,
            vec![
                Action::RegisterAccount,
                Action::RegisterTokens(vec![token_ids.0.clone(), token_ids.1.clone()]),
                Action::Deposit,
                Action::SwapExactOut(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(amount_out.into()),
                    amount_limit: max_amount_in.into(),
                }),
                Action::Withdraw(token_ids.0.clone(), new_amount(0).into(), ()),
                Action::Withdraw(token_ids.1.clone(), new_amount(0).into(), ()),
            ]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::RegisterAccount,
            ActionResult::RegisterTokens,
            ActionResult::Deposit,
            ActionResult::SwapExactOut(_),
            ActionResult::Withdraw(_),
            ActionResult::Withdraw(_),
        ])
    );

    bal_track.assert_changes(
        &sandbox,
        // No changes - because all remnants are withdrawn
        [Change::NoChange, Change::NoChange],
    );
}

#[test]
fn success_base_open_position() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        ..
    } = SwapTestContext::new_all_1g();
    let acc = new_account_id();

    let amounts = (1_000_000, 1_000_000);

    let bal_track = BalanceTracker::new(&sandbox, &acc, [&token_ids.0, &token_ids.1]);

    sandbox.set_initiator_id(acc.clone());
    // NB: we can do only 1 deposit per invocation, so we do two calls
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &acc,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(amounts.0),
            }],
            &mut its_ok,
            vec![
                Action::RegisterAccount,
                Action::RegisterTokens(vec![token_ids.0.clone(), token_ids.1.clone()]),
                Action::Deposit,
            ]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::RegisterAccount,
            ActionResult::RegisterTokens,
            ActionResult::Deposit,
        ])
    );
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &acc,
            &[DepositPayment {
                token_id: token_ids.1.clone(),
                amount: new_amount(amounts.1),
            }],
            &mut its_ok,
            vec![
                Action::Deposit,
                Action::OpenPosition {
                    tokens: token_ids.clone(),
                    fee_rate: 1,
                    position: PositionInit {
                        amount_ranges: (
                            Range {
                                min: new_amount(1).into(),
                                max: new_amount(amounts.0).into(),
                            },
                            Range {
                                min: new_amount(1).into(),
                                max: new_amount(amounts.1).into(),
                            },
                        ),
                        ticks_range: (None, None)
                    }
                },
                Action::Withdraw(token_ids.0.clone(), new_amount(0).into(), ()),
                Action::Withdraw(token_ids.1.clone(), new_amount(0).into(), ()),
            ]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::Deposit,
            ActionResult::OpenPosition,
            ActionResult::Withdraw(_),
            ActionResult::Withdraw(_),
        ])
    );

    bal_track.assert_changes(
        &sandbox,
        // No changes - because all remnants are withdrawn
        [Change::NoChange, Change::NoChange],
    );
}

#[test]
fn success_base_close_position() {
    let SwapTestContext {
        mut sandbox,
        owner,
        token_ids,
        position_id,
    } = SwapTestContext::new_all_1g();

    let bt = BalanceTracker::new(&sandbox, &owner, [&token_ids.0, &token_ids.1]);
    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![
                Action::ClosePosition(position_id),
                Action::Withdraw(token_ids.0, new_amount(0).into(), ()),
                Action::Withdraw(token_ids.1, new_amount(0).into(), ()),
            ]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::ClosePosition,
            ActionResult::Withdraw(Some(())),
            ActionResult::Withdraw(Some(())),
        ])
    );
    bt.assert_changes(
        &sandbox,
        [Change::Exact(new_amount(0)), Change::Exact(new_amount(0))],
    );
}

#[test]
fn success_base_withdraw_fee() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        position_id,
    } = SwapTestContext::new_all_1g();
    // Register another account
    let acc2 = new_account_id();
    sandbox
        .call_mut(|dex| {
            dex.register_account_and_then(Some(acc2.clone()), its_ok)?;
            dex.register_tokens(&acc2, [&token_ids.0, &token_ids.1])?;
            dex.deposit(&acc2, &token_ids.0, new_amount(1_000_000_000))?;
            dex.deposit(&acc2, &token_ids.1, new_amount(1_000_000_000))?;
            Ok(())
        })
        .unwrap();
    // Perform swap for `acc2`
    sandbox
        .call_mut(|dex| {
            dex.execute_actions_impl(
                &acc2,
                &[],
                &mut its_ok,
                vec![Action::SwapExactIn(SwapAction {
                    token_in: token_ids.0.clone(),
                    token_out: token_ids.1.clone(),
                    amount: Some(new_amount(1_000_000).into()),
                    amount_limit: new_amount(500_000).into(),
                })],
            )
        })
        .unwrap();

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions_impl(
            &owner,
            &[],
            &mut its_ok,
            vec![
                Action::WithdrawFee(position_id),
                Action::Withdraw(token_ids.0.clone(), new_amount(0).into(), ()),
                Action::Withdraw(token_ids.1.clone(), new_amount(0).into(), ()),
            ]
        )),
        Ok(v) if matches!(&v[..], &[
            ActionResult::WithdrawFee,
            ActionResult::Withdraw(_),
            ActionResult::Withdraw(_),
        ])
    );
}
