//! Check:
//! * Payable API is enabled
//! * Caller is initiator
//! * Number of outcomes corresponds to number of withdrawals
//! * Check token amounts
use crate::dex::DepositPayment;

use super::dex;
use assert_matches::assert_matches;
use dex::test_utils::{new_account_id, new_amount, BalanceTracker, Change, SwapTestContext};
use dex::{Action, Error, ErrorKind};

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
        sandbox.call_mut(|dex| dex.deposit_execute_actions(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount,
            }],
            &mut |_, _, _| Ok(()),
            vec![Action::Deposit]
        )),
        Ok(v) if v.is_empty()
    );

    bal_track.assert_changes(&sandbox, [Change::FromLogs, Change::NoChange]);
}

#[test]
fn fail_mutable_api_stopped() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();

    let amount = new_amount(1_000);

    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_ids.0, &token_ids.1]);
    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit_execute_actions(
            &owner,
            &[DepositPayment{
                token_id: token_ids.0.clone(),
                amount
            }],
            &mut |_, _, _| Ok(()),
            vec![Action::Deposit]
        )),
        Ok(o) if o.is_empty()
    );
    bal_track.assert_changes(&sandbox, [Change::FromLogs, Change::NoChange]);
    assert_matches!(sandbox.call_mut(|dex| dex.suspend_payable_api()), Ok(_));

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit_execute_actions(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount
            }],
            &mut |_, _, _| Ok(()),
            vec![Action::Deposit]
        )),
        Err(Error {
            kind: ErrorKind::PayableAPISuspended,
            ..
        })
    );

    assert_matches!(sandbox.call_mut(|dex| dex.resume_payable_api()), Ok(_));
    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_ids.0, &token_ids.1]);
    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit_execute_actions(
            &owner,
            &[DepositPayment{
                token_id: token_ids.0.clone(),
                amount
            }],
            &mut |_, _, _| Ok(()),
            vec![Action::Deposit]
        )),
        Ok(o) if o.is_empty()
    );
    bal_track.assert_changes(&sandbox, [Change::FromLogs, Change::NoChange]);
}

#[test]
fn fail_caller_not_initiator() {
    let SwapTestContext {
        mut sandbox,
        token_ids,
        owner,
        ..
    } = SwapTestContext::new();

    let from = new_account_id();
    // some other account as sender
    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit_execute_actions(
            &from,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut |_, _, _| Ok(()),
            vec![Action::Deposit]
        )),
        Err(Error {
            kind: ErrorKind::DepositSenderMustBeSigner,
            ..
        })
    );

    // some other account as initiator
    sandbox.set_initiator_id(from);

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit_execute_actions(
            &owner,
            &[DepositPayment {
                token_id: token_ids.0.clone(),
                amount: new_amount(1_000),
            }],
            &mut |_, _, _| Ok(()),
            vec![Action::Deposit]
        )),
        Err(Error {
            kind: ErrorKind::DepositSenderMustBeSigner,
            ..
        })
    );
}
