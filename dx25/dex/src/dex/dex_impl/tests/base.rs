// Some of the conversions are useless, because NEAR Amount is u128,
// which is not the same as for other DEX's
#![allow(clippy::useless_conversion)]
// FIXME: concordium's address is Copy, but NEAR's one is not
#![allow(clippy::clone_on_copy)]
// Won't be fixed - `|x| x.do_something()` is usually more readable
#![allow(clippy::redundant_closure_for_method_calls)]

use crate::chain::{Amount, TokenId};
use crate::dex::test_utils::{
    amount_as_u128, new_account_id, new_amount, new_token_id, Event, Sandbox, SwapTestContext,
};
use crate::dex::tick::Tick;
use crate::dex::{
    BasisPoints, Error, ErrorKind, PairExt, PoolId, PositionInit, Range, Side, State as _,
};
use crate::Float;
use crate::{assert_any_matches, assert_eq_rel_tol};
use assert_matches::assert_matches;
use rand::Rng;
use rstest::rstest;

#[test]
fn create_instance() {
    const FEE_RATES: [u16; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
    let acc = new_account_id();
    let sandbox = Sandbox::new(acc.clone(), 1, FEE_RATES);
    sandbox.call(|dex| {
        let contract = dex.contract().as_ref();

        assert_eq!(contract.owner_id, &acc);
        assert_eq!(contract.protocol_fee_fraction, 1);
    });
}

#[test]
fn add_remove_guards() {
    let acc = new_account_id();

    // Spawn contract
    let mut sandbox = Sandbox::new_default(acc.clone());

    // Guards accounts
    let account_0 = new_account_id();
    let account_1 = new_account_id();
    let account_2 = new_account_id();

    assert_ne!(account_0, account_1);
    assert_ne!(account_0, account_2);
    assert_ne!(account_1, account_2);

    // Add guards
    sandbox
        .call_mut(|dex| dex.add_guard_accounts([account_0.clone(), account_1.clone()]))
        .unwrap();

    // Suspend payable API from owner
    sandbox.call_mut(|dex| dex.suspend_payable_api()).unwrap();

    // Try to suspend again (fail)
    assert_matches!(
        sandbox.call_mut(|dex| dex.suspend_payable_api()),
        Err(Error {
            kind: ErrorKind::GuardChangeStateDenied,
            ..
        })
    );

    // Resume payable API from first guard
    sandbox.set_initiator_caller_ids(account_0.clone());
    sandbox.call_mut(|dex| dex.resume_payable_api()).unwrap();

    // Try to resume again (fail)
    assert_matches!(
        sandbox.call_mut(|dex| dex.resume_payable_api()),
        Err(Error {
            kind: ErrorKind::GuardChangeStateDenied,
            ..
        })
    );

    // Try to suspend with non registred account
    sandbox.set_initiator_caller_ids(account_2.clone());
    assert_matches!(
        sandbox.call_mut(|dex| dex.suspend_payable_api()),
        Err(Error {
            kind: ErrorKind::PermissionDenied,
            ..
        })
    );

    // Remove one account from guards
    sandbox.set_initiator_caller_ids(acc.clone());
    sandbox
        .call_mut(|dex| dex.remove_guard_accounts([account_1.clone()]))
        .unwrap();

    // Try to suspend with removed account (fail)
    sandbox.set_initiator_caller_ids(account_1.clone());
    assert_matches!(
        sandbox.call_mut(|dex| dex.suspend_payable_api()),
        Err(Error {
            kind: ErrorKind::PermissionDenied,
            ..
        })
    );

    // Suspend payable API from guard
    sandbox.set_initiator_caller_ids(account_0.clone());
    sandbox.call_mut(|dex| dex.suspend_payable_api()).unwrap();

    // Try to register account (fail)
    sandbox.set_initiator_caller_ids(acc);
    assert_matches!(
        sandbox.call_mut(|dex| dex.register_account()),
        Err(Error {
            kind: ErrorKind::PayableAPISuspended,
            ..
        })
    );

    // Resume payable API from owner
    sandbox.call_mut(|dex| dex.resume_payable_api()).unwrap();

    // Register account (success)
    sandbox.call_mut(|dex| dex.register_account()).unwrap();

    // Try to add accounts not from owner (fail)
    sandbox.set_initiator_caller_ids(account_2);
    assert_matches!(
        sandbox.call_mut(|dex| { dex.add_guard_accounts([account_1.clone()]) }),
        Err(Error {
            kind: ErrorKind::PermissionDenied,
            ..
        })
    );
}

#[test]
fn open_close_position() {
    let acc = new_account_id();
    //
    // Spawn contract
    //
    let mut sandbox = Sandbox::new_default(acc.clone());
    //
    // Register account
    //
    sandbox.call_mut(|dex| dex.register_account()).unwrap();
    //
    // Register tokens for account
    //
    let token_0 = new_token_id();
    let token_1 = new_token_id();

    assert_ne!(token_0, token_1);

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1]))
        .unwrap();
    //
    // Deposit tokens
    //
    let initial_balance = (new_amount(500_011), new_amount(5_000_110));

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_0, initial_balance.0))
        .unwrap();
    assert_eq!(initial_balance.0, balance);

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_1, initial_balance.1))
        .unwrap();
    assert_eq!(initial_balance.1, balance);

    sandbox.call(|dex| dex.get_deposit(&acc, &token_0)).unwrap();
    sandbox.call(|dex| dex.get_deposit(&acc, &token_1)).unwrap();

    //
    // Open position
    //
    let pool_id = (&token_0, &token_1);
    let position_amounts_requested = (new_amount(500_000), new_amount(5_000_000));

    let (pos_id, balance_0, balance_1, net_liquidity) = sandbox
        .call_mut(|dex| {
            dex.open_position_full(
                &pool_id.0.clone(),
                &pool_id.1.clone(),
                1,
                position_amounts_requested.0,
                position_amounts_requested.1,
            )
        })
        .unwrap();
    let position_amounts_actual = (balance_0, balance_1);
    assert_eq!(position_amounts_requested, position_amounts_actual);
    //
    // Close position
    //
    sandbox.call_mut(|dex| dex.close_position(pos_id)).unwrap();

    let final_balance_0 = sandbox.call(|dex| dex.get_deposit(&acc, &token_0)).unwrap();
    let final_balance_1 = sandbox.call(|dex| dex.get_deposit(&acc, &token_1)).unwrap();

    let amount_one: Amount = 1u128.into();
    assert!(initial_balance.0 - final_balance_0 <= amount_one); // TODO: FIX: REQUIRE EXACTLY THE SAME AMOUNT
    assert!(initial_balance.1 - final_balance_1 <= amount_one);

    let actual_tick_update_events: Vec<_> = sandbox
        .logs()
        .iter()
        .filter(|event| matches!(event, Event::TickUpdate { .. }))
        .map(|event| match event {
            Event::TickUpdate {
                pool,
                fee_level,
                tick,
                liquidity_change,
            } => (pool, *fee_level, *tick, *liquidity_change),
            _ => unreachable!(),
        })
        .collect();

    let expected_pool_id = PoolId::try_from_pair((token_0, token_1)).unwrap().0.into();
    assert_eq!(
        actual_tick_update_events,
        vec![
            (
                &expected_pool_id,
                0,
                Tick::MIN.index(),
                f64::from(Float::from(net_liquidity))
            ),
            (
                &expected_pool_id,
                0,
                Tick::MAX.index(),
                -f64::from(Float::from(net_liquidity))
            ),
            (&expected_pool_id, 0, Tick::MIN.index(), 0f64),
            (&expected_pool_id, 0, Tick::MAX.index(), 0f64),
        ]
    );
}

#[test]
fn open_two_positions() {
    let acc = new_account_id();
    //
    // Spawn contract
    //
    let mut sandbox = Sandbox::new_default(acc.clone());
    //
    // Register account
    //
    sandbox.call_mut(|dex| dex.register_account()).unwrap();
    //
    // Register tokens for account
    //
    let token_0 = new_token_id();
    let token_1 = new_token_id();

    assert_ne!(token_0, token_1);

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1]))
        .unwrap();
    //
    // Deposit tokens
    //
    let amounts = (new_amount(5_000_000), new_amount(5_000_000));

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_0, amounts.0))
        .unwrap();
    assert_eq!(amounts.0, balance);

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_1, amounts.1))
        .unwrap();
    assert_eq!(amounts.1, balance);
    //
    // Open position #1
    //
    let pool_id = (token_0, token_1);
    let amounts = (new_amount(100_000), new_amount(100_000));

    let (pos_id, balance_0, balance_1, _) = sandbox
        .call_mut(|dex| {
            dex.open_position_full(
                &pool_id.0.clone(),
                &pool_id.1.clone(),
                1,
                amounts.0,
                amounts.1,
            )
        })
        .unwrap();
    let pos_amounts = (balance_0, balance_1);
    assert_eq!(amounts, pos_amounts);
    assert_eq!(pos_id, 0);
    //
    // Open position #2
    //
    let amounts = (new_amount(50_000), new_amount(50_000));

    let (pos_id, balance_0, balance_1, _) = sandbox
        .call_mut(|dex| {
            dex.open_position_full(
                &pool_id.0.clone(),
                &pool_id.1.clone(),
                1,
                amounts.0,
                amounts.1,
            )
        })
        .unwrap();
    let pos_amounts = (balance_0, balance_1);
    assert_eq!(amounts, pos_amounts);
    assert_eq!(pos_id, 1);
}

#[test]
fn get_positions_infos() {
    let acc = new_account_id();
    //
    // Spawn contract
    //
    let mut sandbox = Sandbox::new_default(acc.clone());
    //
    // Register account
    //
    sandbox.call_mut(|dex| dex.register_account()).unwrap();
    //
    // Register tokens for account
    //
    let token_0 = new_token_id();
    let token_1 = new_token_id();

    assert_ne!(token_0, token_1);

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1]))
        .unwrap();
    //
    // Deposit tokens
    //
    let amounts = (new_amount(5_000_000), new_amount(5_000_000));

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_0, amounts.0))
        .unwrap();
    assert_eq!(amounts.0, balance);

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_1, amounts.1))
        .unwrap();
    assert_eq!(amounts.1, balance);
    //
    // Open position #1
    //
    let pool_id = (token_0, token_1);
    let amounts = (new_amount(100_000), new_amount(100_000));

    let (pos_id1, balance_0, balance_1, _) = sandbox
        .call_mut(|dex| {
            dex.open_position_full(
                &pool_id.0.clone(),
                &pool_id.1.clone(),
                1,
                amounts.0,
                amounts.1,
            )
        })
        .unwrap();
    let pos_amounts = (balance_0, balance_1);
    assert_eq!(amounts, pos_amounts);
    assert_eq!(pos_id1, 0);
    //
    // Open position #2
    //
    let amounts = (new_amount(50_000), new_amount(50_000));

    let (pos_id2, balance_0, balance_1, _) = sandbox
        .call_mut(|dex| {
            dex.open_position_full(
                &pool_id.0.clone(),
                &pool_id.1.clone(),
                1,
                amounts.0,
                amounts.1,
            )
        })
        .unwrap();
    let pos_amounts = (balance_0, balance_1);
    assert_eq!(amounts, pos_amounts);
    assert_eq!(pos_id2, 1);

    // Get position info for each opened positions
    let pos_info1 = sandbox
        .call_mut(|dex| dex.get_position_info(pos_id1))
        .unwrap();

    assert_eq!(pos_info1.tokens_ids, (pool_id.1.clone(), pool_id.0.clone()));
    assert_eq!(pos_info1.balance, (99_999u128.into(), 99_999u128.into()));

    let pos_info2 = sandbox
        .call_mut(|dex| dex.get_position_info(pos_id2))
        .unwrap();

    assert_eq!(pos_info2.tokens_ids, (pool_id.1.clone(), pool_id.0.clone()));
    assert_eq!(pos_info2.balance, (49_999u128.into(), 49_999u128.into()));

    // Get infos for both positions
    let pos_infos = sandbox
        .call_mut(|dex| Ok(dex.get_positions_info(&[pos_id1, 10, pos_id2, 25, 345])))
        .unwrap();

    assert_eq!(pos_infos.len(), 5);

    assert_matches!(
        &pos_infos[0],
        Some(info) if info.tokens_ids == (pool_id.1.clone(), pool_id.0.clone()) &&
                      info.balance == (99_999u128.into(), 99_999u128.into())
    );
    assert_matches!(&pos_infos[1], None);
    assert_matches!(
        &pos_infos[2],
        Some(info) if info.tokens_ids == (pool_id.1.clone(), pool_id.0.clone()) &&
                      info.balance == (49_999u128.into(), 49_999u128.into())
    );
    assert_matches!(&pos_infos[3], None);
    assert_matches!(&pos_infos[4], None);
}

#[test]
fn open_first_position_signle_sided_succeeds() {
    let acc = new_account_id();
    //
    // Spawn contract
    //
    let mut sandbox = Sandbox::new_default(acc.clone());
    //
    // Register account
    //
    sandbox.call_mut(|dex| dex.register_account()).unwrap();
    //
    // Register tokens for account
    //
    let token_0 = new_token_id();
    let token_1 = new_token_id();

    assert_ne!(token_0, token_1);

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1]))
        .unwrap();
    //
    // Deposit tokens
    //
    let amounts = (new_amount(500_011 << 32), new_amount(5_000_110 << 32));

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_0, amounts.0))
        .unwrap();
    assert_eq!(amounts.0, balance);

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_1, amounts.1))
        .unwrap();
    assert_eq!(amounts.1, balance);
    //
    // Open position
    //
    let pool_id = (token_0, token_1);

    let tick_high = Tick::new(750i32).unwrap();

    let open_position_result = sandbox.call_mut(|dex| {
        dex.open_position(
            &pool_id.0.clone(),
            &pool_id.1.clone(),
            16,
            PositionInit {
                amount_ranges: (
                    Range {
                        min: new_amount(100).into(),
                        max: new_amount(1000_u128 << 32).into(),
                    },
                    Range {
                        min: new_amount(0).into(),
                        max: new_amount(0).into(),
                    },
                ),
                ticks_range: (None, tick_high.to_opt_index()),
            },
        )
    });

    assert_matches!(open_position_result, Ok(_));

    sandbox
        .call_mut(|dex| {
            let actual_price = dex
                .get_pool_info((pool_id.0.clone(), pool_id.1.clone()))
                .unwrap()
                .unwrap()
                .spot_sqrtprices[3];
            let expected_price = tick_high.spot_sqrtprice();
            assert_eq_rel_tol!(actual_price, expected_price, 2);
            Ok(())
        })
        .unwrap();
}

#[test]
fn open_non_first_position_signle_sided_fails() {
    let acc = new_account_id();
    //
    // Spawn contract
    //
    let mut sandbox = Sandbox::new_default(acc.clone());
    //
    // Register account
    //
    sandbox.call_mut(|dex| dex.register_account()).unwrap();
    //
    // Register tokens for account
    //
    let token_0 = new_token_id();
    let token_1 = new_token_id();

    assert_ne!(token_0, token_1);

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1]))
        .unwrap();
    //
    // Deposit tokens
    //
    let amounts = (new_amount(500_011), new_amount(5_000_110));

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_0, amounts.0))
        .unwrap();
    assert_eq!(amounts.0, balance);

    let balance = sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_1, amounts.1))
        .unwrap();
    assert_eq!(amounts.1, balance);
    //
    // Open position
    //
    let pool_id = (token_0, token_1);

    let (_pos_id, amount_left, amount_right, _liquidity) = sandbox
        .call_mut(|dex| {
            dex.open_position(
                &pool_id.0.clone(),
                &pool_id.1.clone(),
                16,
                PositionInit {
                    amount_ranges: (
                        Range {
                            min: new_amount(100).into(),
                            max: new_amount(1000).into(),
                        },
                        Range {
                            min: new_amount(500).into(),
                            max: new_amount(5000).into(),
                        },
                    ),
                    ticks_range: (None, None),
                },
            )
        })
        .unwrap();
    assert_eq!(amount_left, Amount::from(1000_u64));
    assert_eq!(amount_right, Amount::from(5000_u64));

    let open_position_result = sandbox.call_mut(|dex| {
        dex.open_position(
            &pool_id.0.clone(),
            &pool_id.1.clone(),
            16,
            PositionInit {
                amount_ranges: (
                    Range {
                        min: new_amount(100).into(),
                        max: new_amount(1000).into(),
                    },
                    Range {
                        min: new_amount(0).into(),
                        max: new_amount(0).into(),
                    },
                ),
                ticks_range: (None, None),
            },
        )
    });
    assert_matches!(
        open_position_result,
        Err(Error {
            kind: ErrorKind::Slippage,
            ..
        })
    );
}

#[test]
fn deposit_successful() {
    const DEPOSIT_AMOUNT: u128 = 2_000;

    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());

    assert_matches!(sandbox.call_mut(|dex| dex.register_account()), Ok(()));

    assert_matches!(
        sandbox.call_mut(|dex| dex.register_tokens(&acc, [&token_id])),
        Ok(())
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, new_amount(DEPOSIT_AMOUNT))),
        Ok(amount) if amount == new_amount(DEPOSIT_AMOUNT)
    );

    let new_balance = assert_matches!(
        sandbox.call(|dex| dex.get_deposit(&acc, &token_id)),
        Ok(balance) if balance == new_amount(DEPOSIT_AMOUNT) => balance
    );

    assert_any_matches!(
        sandbox.latest_logs(),
        Event::Deposit {
            user,
            token,
            amount,
            balance,
        } if
            user == &acc
            && token == &token_id
            && amount == &new_amount(DEPOSIT_AMOUNT)
            && balance == &new_balance
    );

    assert_eq!(new_balance, new_amount(DEPOSIT_AMOUNT));
}

// Test deposit for an account which differs from the caller
#[test]
#[cfg(not(feature = "near"))]
fn deposit_other_account_successful() {
    const DEPOSIT_AMOUNT: u128 = 2_000;

    let acc = new_account_id();
    let acc_to_deposit = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc);

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc_to_deposit, &token_id, new_amount(DEPOSIT_AMOUNT))),
        Ok(amount) if amount == new_amount(DEPOSIT_AMOUNT)
    );

    let new_balance = assert_matches!(
        sandbox.call(|dex| dex.get_deposit(&acc_to_deposit, &token_id)),
        Ok(balance) if balance == new_amount(DEPOSIT_AMOUNT) => balance
    );

    assert_any_matches!(
        sandbox.latest_logs(),
        Event::Deposit {
            user,
            token,
            amount,
            balance,
        } if
            user == &acc_to_deposit
            && token == &token_id
            && amount == &new_amount(DEPOSIT_AMOUNT)
            && balance == &new_balance
    );

    assert_eq!(new_balance, new_amount(DEPOSIT_AMOUNT));
}

#[test]
#[cfg(feature = "near")]
fn deposit_fails_account_not_registered() {
    const DEPOSIT_AMOUNT: u128 = 2_000;
    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, new_amount(DEPOSIT_AMOUNT))),
        Err(Error {
            kind: ErrorKind::AccountNotRegistered,
            ..
        })
    );
}

#[test]
#[cfg(feature = "near")]
fn deposit_fails_token_not_registered() {
    const DEPOSIT_AMOUNT: u128 = 2_000;
    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());

    assert_matches!(sandbox.call_mut(|dex| dex.register_account()), Ok(()));

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, new_amount(DEPOSIT_AMOUNT))),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
}

#[test]
fn swap_exact_in_success() {
    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.swap_exact_in(
            &[token_0, token_1],
            new_amount(100),
            new_amount(0),
        )),
        Ok(_)
    );

    // TODO: check that swap produced correct results
}

#[test]
fn swap_exact_in_failure() {
    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.swap_exact_in(
            &[token_0, token_1],
            new_amount(1),
            new_amount(20)
        )),
        Err(_)
    );
}

#[test]
fn swap_exact_out_success() {
    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.swap_exact_out(
            &[token_0, token_1],
            new_amount(100),
            new_amount(2000),
        )),
        Ok(_)
    );

    // TODO: check that swap produced correct results
}

#[test]
fn swap_exact_out_failure() {
    let SwapTestContext {
        mut sandbox,
        token_ids: (token_0, token_1),
        ..
    } = SwapTestContext::new();

    assert_matches!(
        sandbox.call_mut(|dex| dex.swap_exact_out(
            &[token_0, token_1],
            new_amount(100),
            new_amount(1),
        )),
        Err(_)
    );
}

#[test]
fn withdraw_failure_account_not_registered() {
    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());

    assert_matches!(
        sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, new_amount(2_000), false, ())),
        Err(Error {
            kind: ErrorKind::AccountNotRegistered,
            ..
        })
    );
}

#[test]
fn withdraw_failure_token_not_registered() {
    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());

    assert_matches!(sandbox.call_mut(|dex| dex.register_account()), Ok(()));

    assert_matches!(
        sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, new_amount(2_000), false, (),)),
        Err(Error {
            kind: ErrorKind::TokenNotRegistered,
            ..
        })
    );
}

#[test]
fn success_withdraw_zero_amount_zero_balance() {
    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());

    assert_matches!(sandbox.call_mut(|dex| dex.register_account()), Ok(_));
    assert_matches!(
        sandbox.call_mut(|dex| dex.register_tokens(&acc, [&token_id])),
        Ok(_)
    );

    // Try withdraw zero amount from zero balance. This should actually succeed
    assert_matches!(
        sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, new_amount(0), false, (),)),
        Ok(None)
    );

    assert_matches!(
        sandbox.call(|dex| dex.get_deposit(&acc, &token_id)),
        Ok(balance) if balance == new_amount(0)
    );
}

#[test]
fn withdraw_failure_not_enough_tokens() {
    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());
    assert_matches!(sandbox.call_mut(|dex| dex.register_account()), Ok(_));
    assert_matches!(
        sandbox.call_mut(|dex| dex.register_tokens(&acc, [&token_id])),
        Ok(_)
    );
    // Try withdraw non-zero amount when there's zero tokens in store
    assert_matches!(
        sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, new_amount(2_000), false, ())),
        Err(Error {
            kind: ErrorKind::NotEnoughTokens { .. },
            ..
        })
    );

    assert_matches!(
        sandbox.call(|dex| dex.get_deposit(&acc, &token_id)),
        Ok(balance) if balance == new_amount(0)
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, new_amount(1_000),)),
        Ok(_)
    );
    // Try withdraw non-zero amount when there's some tokens stored but not enough
    assert_matches!(
        sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, new_amount(2_000), false, ())),
        Err(Error {
            kind: ErrorKind::NotEnoughTokens { .. },
            ..
        })
    );

    assert_matches!(
        sandbox.call(|dex| dex.get_deposit(&acc, &token_id)),
        Ok(balance) if balance == new_amount(1_000)
    );
}

#[test]
fn withdraw_success_whole_balance() {
    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());
    assert_matches!(sandbox.call_mut(|dex| dex.register_account()), Ok(_));
    assert_matches!(
        sandbox.call_mut(|dex| dex.register_tokens(&acc, [&token_id])),
        Ok(_)
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, new_amount(2_000),)),
        Ok(_)
    );
    // Check correct withdrawal of whole balance
    assert_matches!(
        sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, new_amount(0), false, ())),
        Ok(Some(()))
    );

    assert_matches!(
        sandbox.call(|dex| dex.get_deposit(&acc, &token_id)),
        Ok(balance) if balance == new_amount(0)
    );
    // Check correct withdrawal of whole balance with explicit amount
    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, new_amount(2_000),)),
        Ok(_)
    );

    assert_matches!(
        sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, new_amount(2_000), false, ())),
        Ok(Some(()))
    );

    assert_matches!(
        sandbox.call(|dex| dex.get_deposit(&acc, &token_id)),
        Ok(balance) if balance == new_amount(0)
    );
}

#[test]
fn withdraw_success_arbitrary() {
    const RANDOM_DEPOSIT_MAX: u128 = 1_000_000;
    const N_ITERS: i32 = 100;

    let acc = new_account_id();
    let token_id = new_token_id();

    let mut sandbox = Sandbox::new_default(acc.clone());
    assert_matches!(sandbox.call_mut(|dex| dex.register_account()), Ok(_));
    assert_matches!(
        sandbox.call_mut(|dex| dex.register_tokens(&acc, [&token_id])),
        Ok(_)
    );

    let mut balance = new_amount(2_000);
    assert_matches!(
        sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, balance,)),
        Ok(_)
    );

    let mut rng = rand::thread_rng();

    for _ in 0..N_ITERS {
        // Add random number of tokens
        let amount = new_amount(rng.gen_range(0..=RANDOM_DEPOSIT_MAX));

        balance += amount;
        assert_matches!(
            sandbox.call_mut(|dex| dex.deposit(&acc, &token_id, amount,)),
            Ok(_)
        );
        // Generate withdrawal amount
        let before_balance = balance;
        let amount = new_amount(rng.gen_range(1..=amount_as_u128(balance)));
        // Check correct withdrawal of amount
        assert_matches!(
            sandbox.call_mut(|dex| dex.withdraw(&acc, &token_id, amount, false, ())),
            Ok(Some(()))
        );

        balance -= amount;

        assert_matches!(
            sandbox.call(|dex| dex.get_deposit(&acc, &token_id)),
            Ok(after_balance) if after_balance == balance,
            "Balance {} is expected to be {} after withdrawing {}",
            before_balance,
            balance,
            amount,
        );
    }
}

#[test]
#[ignore]
fn test_reserves_consistency() {
    let acc = new_account_id();
    let mut sandbox = Sandbox::new_default(acc.clone());

    let amounts = (
        new_amount(1_243_319 * (1 << 26) - 333),
        new_amount(1_242_342_324 * (1 << 31) + 1212),
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

    sandbox
        .call_mut(|dex| {
            dex.open_position_full(&token_0.clone(), &token_1.clone(), 1, amounts.0, amounts.1)
        })
        .unwrap();

    let pool_info = sandbox
        .call_mut(|dex| Ok(dex.get_pool_info((token_0, token_1)).unwrap()))
        .unwrap()
        .unwrap();

    dbg!(&pool_info);

    assert!(pool_info.total_reserves.0 >= pool_info.position_reserves.0);
    assert!(pool_info.total_reserves.1 >= pool_info.position_reserves.1);
}

#[test]
#[ignore = "Exists to briefly investigate version produced"]
fn version() {
    let sbx = Sandbox::new_default(new_account_id());
    let ver = sbx.call(|dex| dex.get_version());
    assert!(ver.version.is_empty(), "Version: {ver:?}");
}

#[test]
#[allow(clippy::too_many_lines)] // The test implies multiple positions opening and events check.
fn test_open_position() {
    let acc = new_account_id();
    let mut sandbox = Sandbox::new_default(acc.clone());

    let amounts = (
        new_amount(1_243_319 * (1 << 26) - 333),
        new_amount(1_242_342_324 * (1 << 31) + 1212),
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

    let fee_rates = [1, 2, 4, 8, 16, 32, 64, 128];

    let mut open_position = |token_a: &TokenId,
                             token_b: &TokenId,
                             fee_rate,
                             max_left: Amount,
                             max_right: Amount,
                             tick_low: Tick,
                             tick_high: Tick| {
        sandbox
            .call_mut(|dex| {
                dex.open_position(
                    token_a,
                    token_b,
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
            .unwrap();
    };

    open_position(
        &token_1,
        &token_0,
        fee_rates[3],
        1_000_000_000u128.into(),
        200_000_000u128.into(),
        Tick::MIN,
        Tick::MAX,
    );
    open_position(
        &token_1,
        &token_0,
        fee_rates[5],
        1_000_000u128.into(),
        1_000_000_000u128.into(),
        Tick::new(-100).unwrap(),
        Tick::MAX,
    );
    open_position(
        &token_1,
        &token_0,
        fee_rates[1],
        500_000_000u128.into(),
        1_000_000u128.into(),
        Tick::MIN,
        Tick::new(-200).unwrap(),
    );

    // While token id for the given pair is (token_1, token_0), the following position is opened with the swapped tokens.
    // So expect the ticks in events to be transponded.
    open_position(
        &token_0,
        &token_1,
        fee_rates[1],
        500_000_000u128.into(),
        100_000_000u128.into(),
        Tick::new(-300).unwrap(),
        Tick::new(200).unwrap(),
    );

    let actual_tick_update_events: Vec<_> = sandbox
        .logs()
        .iter()
        .filter(|event| matches!(event, Event::TickUpdate { .. }))
        .map(|event| match event {
            Event::TickUpdate {
                pool,
                fee_level,
                tick,
                liquidity_change: _,
            } => (pool, *fee_level, *tick),
            _ => unreachable!(),
        })
        .collect();

    let expected_pool_id = PoolId::try_from_pair((token_0, token_1)).unwrap().0.into();
    assert_eq!(
        actual_tick_update_events,
        vec![
            (&expected_pool_id, 3, Tick::MIN.index()),
            (&expected_pool_id, 3, Tick::MAX.index()),
            (&expected_pool_id, 5, -100),
            (&expected_pool_id, 5, Tick::MAX.index()),
            (&expected_pool_id, 1, Tick::MIN.index()),
            (&expected_pool_id, 1, -200),
            (&expected_pool_id, 1, -200),
            (&expected_pool_id, 1, 300),
        ]
    );
}

#[allow(clippy::type_complexity)]
type TestPositionParams = ((u64, u64), (Tick, Tick), BasisPoints);

#[rstest]
#[case(
    vec![
        ((1_000_000_000, 1_000_000_000), (Tick::MIN, Tick::MAX), 1),
    ],
    vec![
        (new_amount(10_000_000), Side::Left),
    ],
    vec![
        // 10_000_000. * 0.0001 * 0.87 = 870
        (new_amount(870), new_amount(0)),
    ],
)]
#[case(
    vec![
        ((1_000_000_000, 1_000_000_000), (Tick::MIN, Tick::MAX), 1),
        ((1_000_000_000, 1_000_000_000), (Tick::MIN, Tick::MAX), 1),
    ],
    vec![
        (new_amount(10_000_000), Side::Left),
    ],
    vec![
        // 10_000_000. * 0.0001 * 0.87 * 1/2 = 435
        (new_amount(435), new_amount(0)),
        // 10_000_000. * 0.0001 * 0.87 * 1/2 = 435
        (new_amount(435), new_amount(0)),
    ],
)]
#[case(
    vec![
        ((1_000_000_000, 1_000_000_000), (Tick::MIN, Tick::MAX), 1),
        ((1_000_000_000, 1_000_000_000), (Tick::MIN, Tick::new(-1000).unwrap()), 1),
    ],
    vec![
        (new_amount(10_000_000), Side::Left),
    ],
    vec![
        // 10_000_000. * 0.0001 * 0.87 = 870
        (new_amount(870), new_amount(0)),
        (new_amount(0), new_amount(0)),
    ],
)]
#[case(
    vec![
        ((1_000_000_000, 1_000_000_000), (Tick::MIN, Tick::MAX), 1),

        // price from 0.25 to 4 which should give us double liquidity of full range position with same amounts at price 1
        ((1_000_000_000, 1_000_000_000), (Tick::new(-13863).unwrap(), Tick::new(13863).unwrap()), 1),
    ],
    vec![
        (new_amount(30_000_000), Side::Left),
    ],
    vec![
        // 30_000_000 * 0.0001 * 0.87 * 1/3 = 870
        (new_amount(870), new_amount(0)),
        // 30_000_000 * 0.0001 * 0.87 * 2/3 = 1740
        (new_amount(1740), new_amount(0)),
    ],
)]
#[case(
    vec![
        ((100_000_000_000, 100_000_000_000), (Tick::MIN, Tick::MAX), 1),

        // price from 1+ to 2
        ((1_000_000_000, 1_000_000_000), (Tick::new(10).unwrap(), Tick::new(6932).unwrap()), 1),

        // price from 2+ to 3
        ((1_000_000_000, 1_000_000_000), (Tick::new(6933).unwrap(), Tick::new(10987).unwrap()), 1),

        // price from 3+ to 4
        ((1_000_000_000, 1_000_000_000), (Tick::new(10987).unwrap(), Tick::new(6933*2).unwrap()), 1),
    ],
    vec![
        (new_amount(123_000_000_000), Side::Left), // move price to 5, passing all positions
        (new_amount(26_000_000_000), Side::Right), // move price to 2.5
    ],
    vec![
        // values are not checked manually (except 0), just making sure everything works.
        (new_amount(10_062_291), new_amount(2_089_887)),
        (new_amount(123_099), new_amount(0)),
        (new_amount(213_121), new_amount(84886)),
        (new_amount(301_417), new_amount(86999)),
    ],
)]
fn test_withdraw_fee(
    #[case] positions: Vec<TestPositionParams>,
    #[case] swaps: Vec<(Amount, Side)>,
    #[case] expected_fees: Vec<(Amount, Amount)>,
) {
    let acc = new_account_id();
    let mut sandbox = Sandbox::new_default(acc.clone());

    sandbox.call_mut(|dex| dex.register_account()).unwrap();

    let amounts = (new_amount(1_000_000_000_000), new_amount(1_000_000_000_000));

    sandbox.call_mut(|dex| dex.register_account()).unwrap();

    let tokens: (TokenId, TokenId) = PoolId::try_from_pair((new_token_id(), new_token_id()))
        .unwrap()
        .0
        .into();

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&tokens.0, &tokens.1]))
        .unwrap();

    sandbox
        .call_mut(|dex| dex.deposit(&acc, &tokens.0, amounts.0))
        .unwrap();
    sandbox
        .call_mut(|dex| dex.deposit(&acc, &tokens.1, amounts.1))
        .unwrap();

    let mut position_ids = Vec::new();
    for (amounts, ticks, fee_rate) in positions {
        let (pos_id, ..) = sandbox
            .call_mut(|dex| {
                dex.open_position(
                    &tokens.0,
                    &tokens.1,
                    fee_rate,
                    PositionInit {
                        amount_ranges: (
                            Range {
                                min: new_amount(0).into(),
                                max: new_amount(u128::from(amounts.0)).into(),
                            },
                            Range {
                                min: new_amount(0).into(),
                                max: new_amount(u128::from(amounts.1)).into(),
                            },
                        ),
                        ticks_range: ticks.map(|t| t.to_opt_index()),
                    },
                )
            })
            .unwrap();
        position_ids.push(pos_id);
    }

    for (amount, direction) in swaps {
        sandbox
            .call_mut(|dex| {
                let swap_tokens = if direction == Side::Left {
                    [tokens.0.clone(), tokens.1.clone()]
                } else {
                    [tokens.1.clone(), tokens.0.clone()]
                };
                dex.swap_exact_in(&swap_tokens, amount, new_amount(1))
            })
            .unwrap();
    }

    for (pid, expected_fee) in position_ids.into_iter().zip(expected_fees) {
        let fees = sandbox.call_mut(|dex| dex.withdraw_fee(pid)).unwrap();

        if expected_fee.0 == new_amount(0) {
            assert_eq!(fees.0, new_amount(0));
        } else {
            // actual fee amount may be less by 1 token because of roundings in favor of dex
            assert!(expected_fee.0 - new_amount(1) <= fees.0 && fees.0 <= expected_fee.0);
        }

        if expected_fee.1 == new_amount(0) {
            assert_eq!(fees.1, new_amount(0));
        } else {
            // actual fee amount may be less by 1 token because of roundings in favor of dex
            assert!(expected_fee.1 - new_amount(1) <= fees.1 && fees.1 <= expected_fee.1);
        }

        sandbox.call_mut(|dex| dex.close_position(pid)).unwrap();
    }
}

#[test]
fn test_liqudity_fee_level_distribution() {
    let open_position = |sandbox: &mut Sandbox,
                         token_0: &TokenId,
                         token_1: &TokenId,
                         fee_level,
                         amount_multiplier: u128| {
        sandbox
            .call_mut(|dex| {
                dex.open_position(
                    &token_0.clone(),
                    &token_1.clone(),
                    fee_level,
                    PositionInit {
                        amount_ranges: (
                            Range {
                                min: new_amount(0u128).into(),
                                max: new_amount(amount_multiplier * 1000).into(),
                            },
                            Range {
                                min: new_amount(0u128).into(),
                                max: new_amount(amount_multiplier * 1000).into(),
                            },
                        ),
                        ticks_range: (None, None),
                    },
                )
            })
            .unwrap();
    };

    let get_fee_distribution = |sandbox: &Sandbox, token_0: &TokenId, token_1: &TokenId| {
        sandbox
            .call(|dex| dex.get_liqudity_fee_level_distribution((token_0.clone(), token_1.clone())))
            .unwrap()
    };

    let acc = new_account_id();
    let mut sandbox = Sandbox::new_default(acc.clone());

    sandbox.call_mut(|dex| dex.register_account()).unwrap();

    let token_0 = new_token_id();
    let token_1 = new_token_id();
    let token_2 = new_token_id();

    sandbox
        .call_mut(|dex| dex.register_tokens(&acc, [&token_0, &token_1, &token_2]))
        .unwrap();

    sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_0, 1_000_000u128.into()))
        .unwrap();
    sandbox
        .call_mut(|dex| dex.deposit(&acc, &token_1, 1_000_000u128.into()))
        .unwrap();

    // Open positions
    open_position(&mut sandbox, &token_0, &token_1, 1, 2);
    open_position(&mut sandbox, &token_0, &token_1, 2, 3);
    open_position(&mut sandbox, &token_0, &token_1, 4, 5);

    // Check no pool levels
    let no_position = get_fee_distribution(&sandbox, &token_1, &token_2);
    assert!(no_position.is_none());

    // Check valid levels
    let liquidities = get_fee_distribution(&sandbox, &token_0, &token_1).unwrap();

    // Preciseness is random here
    // Should be something like [20.000000000000004, 29.999999999999996, 50.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    assert!((liquidities[0] - 20.0.into()).abs() < 0.0001.into());
    assert!((liquidities[1] - 30.0.into()).abs() < 0.0001.into());
    assert!((liquidities[2] - 50.0.into()).abs() < 0.0001.into());
}

#[test]
fn test_log_positions_ticks() {
    let acc = new_account_id();
    let mut sandbox = Sandbox::new_default(acc.clone());

    let amounts = (
        new_amount(1_243_319 * (1 << 26) - 333),
        new_amount(1_242_342_324 * (1 << 31) + 1212),
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

    let fee_rates = [1, 2, 4, 8, 16, 32, 64, 128];

    let mut open_position = |token_a: &TokenId,
                             token_b: &TokenId,
                             fee_rate,
                             max_left: Amount,
                             max_right: Amount,
                             tick_low: Tick,
                             tick_high: Tick,
                             close: bool| {
        if close {
            sandbox.call_mut(|dex| dex.close_position(2)).unwrap();
            return;
        }

        sandbox
            .call_mut(|dex| {
                dex.open_position(
                    token_a,
                    token_b,
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
            .unwrap();
    };

    for t in 1..100 {
        open_position(
            &token_1,
            &token_0,
            fee_rates[3],
            1_000_000_000u128.into(),
            200_000_000u128.into(),
            Tick::new(-t).unwrap(),
            Tick::new(t).unwrap(),
            false,
        );
    }

    assert_matches!(
        sandbox.call_mut(|dex| {
            dex.log_ticks_liquidity_change((token_0.clone(), token_1.clone()), 3, Tick::MIN.index(), 20)
        }),
        Ok(last_logged_tick) if last_logged_tick == -80
    );

    assert_matches!(
        sandbox.call_mut(|dex| {
            dex.log_ticks_liquidity_change((token_0.clone(), token_1.clone()), 3, -79, 15)
        }),
        Ok(last_logged_tick) if last_logged_tick == -65
    );

    // NOTE: Same tick liquidity changes may be logged multiple times
    assert_matches!(
        sandbox.call_mut(|dex| {
            dex.log_ticks_liquidity_change((token_0.clone(), token_1.clone()), 3, Tick::MIN.index(), 200)
        }),
        Ok(last_logged_tick) if last_logged_tick == 99
    );
}
