//! Check:
//! * Payable API enabled
//! * Number of outcomes corresponds to number of withdrawals
//! * Check token amounts

use super::dex;
use assert_matches::assert_matches;
use dex::test_utils::{new_amount, new_token_id, BalanceTracker, Change, SwapTestContext};
use dex::{Action, Error, ErrorKind, SwapAction};
use rstest::rstest;

#[allow(clippy::unnecessary_wraps)] // Expected - func is a stub for register account constructor
fn its_ok<T: dex::Types>(
    _id: &crate::chain::AccountId,
    _acc: &mut dex::Account<T>,
    _ex: bool,
) -> dex::Result<()> {
    Ok(())
}

#[test]
fn empty() {
    let SwapTestContext { mut sandbox, .. } = SwapTestContext::new();

    assert_matches!(sandbox.call_mut(|dex| dex.execute_actions(&mut its_ok, vec![])), Ok((v, None)) if v.is_empty());
    assert_eq!(sandbox.latest_logs().len(), 0);
}

#[test]
fn fail_mutable_api_stopped() {
    let SwapTestContext { mut sandbox, .. } = SwapTestContext::new();

    assert_matches!(sandbox.call_mut(|dex| dex.execute_actions(&mut its_ok, vec![])), Ok((v, None)) if v.is_empty());
    assert_matches!(sandbox.call_mut(|dex| dex.suspend_payable_api()), Ok(_));

    assert_matches!(
        sandbox.call_mut(|dex| dex.execute_actions(&mut its_ok, vec![])),
        Err(Error {
            kind: ErrorKind::PayableAPISuspended,
            ..
        })
    );

    assert_matches!(sandbox.call_mut(|dex| dex.resume_payable_api()), Ok(_));
    assert_matches!(sandbox.call_mut(|dex| dex.execute_actions(&mut its_ok, vec![])), Ok((v, None)) if v.is_empty());
}

#[rstest]
fn success_two_swap_ins_chain(#[values(200, 1_000, 5_000)] amount: u128) {
    let mut ctxt = SwapTestContext::new_all_1g();
    let token_2 = new_token_id();
    ctxt.open_position_1g((&ctxt.token_ids.1.clone(), &token_2));

    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        ..
    } = ctxt;

    let amount_limit = new_amount(amount / 2);
    let amount = new_amount(amount);

    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_0, &token_1, &token_2]);

    let amount_out = assert_matches!(sandbox.call_mut(|dex| dex.execute_actions(&mut its_ok, vec![
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
        Ok((outs, Some(a))) if outs.is_empty() => a
    );

    bal_track.assert_changes(
        &sandbox,
        [Change::FromLogs, Change::NoChangeExact, Change::FromLogs],
    );

    assert!((amount_limit..=amount).contains(&amount_out));
}

#[rstest]
fn success_two_swap_outs_chain(#[values(200, 1_000, 5_000)] amount: u128) {
    let mut ctxt = SwapTestContext::new_all_1g();
    let token_2 = new_token_id();
    ctxt.open_position_1g((&ctxt.token_ids.1.clone(), &token_2));

    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        ..
    } = ctxt;

    let amount_limit = new_amount(amount * 2);
    let amount = new_amount(amount);

    let bal_track = BalanceTracker::new_with_caller(&sandbox, [&token_0, &token_1, &token_2]);

    let amount_out = assert_matches!(sandbox.call_mut(|dex| dex.execute_actions(&mut its_ok, vec![
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
        Ok((outs, Some(a))) if outs.is_empty() => a
    );

    bal_track.assert_changes(
        &sandbox,
        [Change::FromLogs, Change::NoChangeExact, Change::FromLogs],
    );

    assert!((amount..=amount_limit).contains(&amount_out));
}
