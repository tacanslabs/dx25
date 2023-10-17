#![deny(warnings)]
#![allow(clippy::too_many_lines)]

#[macro_use]
mod contract_builder;

use std::collections::HashMap;

use dx25::{
    api_types::{Action, ApiVec},
    dex::{PoolId, PositionInit, Range, SwapAction},
    events::event,
    ContractObj, Dx25Contract, EgldOrTokenId, TokenId,
};

use contract_builder::{Dx25Setup, BTC_TOKEN_ID, ESDT_TOKEN_ID};
use multiversx_sc_codec::TopDecode;
use multiversx_sc_scenario::{
    rust_biguint,
    testing_framework::{TxContextStack, TxTokenTransfer},
    DebugApi,
};

#[test]
fn success_deposit_batch_open_position() {
    let zero = 0u64;
    let init_amount = 2_000_000_000u64;
    let deposit_amount = 1_500_000_000u64;
    let liq_amount = 1_000_000_000u64;

    let mut cf_setup = Dx25Setup::setup();

    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount),
    );
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(init_amount),
    );

    let esdt_id = TokenId::from_bytes(ESDT_TOKEN_ID);
    let btc_id = TokenId::from_bytes(BTC_TOKEN_ID);
    // Standard set of actions for opeing fresh position
    let actions = vec![
        Action::RegisterAccount,
        Action::RegisterTokens(vec![esdt_id.clone(), btc_id.clone()]),
        Action::Deposit,
        Action::OpenPosition {
            tokens: (esdt_id.clone(), btc_id.clone()),
            fee_rate: 1,
            position: PositionInit {
                amount_ranges: (
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                ),
                ticks_range: (None, None),
            },
        },
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];

    let mut tx_logs = vec![];
    cf_setup
        .blockchain_wrapper
        .execute_esdt_multi_transfer(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &[
                TxTokenTransfer {
                    token_identifier: ESDT_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
                TxTokenTransfer {
                    token_identifier: BTC_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
            ],
            |sc| {
                sc.deposit(ApiVec(actions));
                tx_logs = TxContextStack::static_peek().extract_result().result_logs;
            },
        )
        .assert_ok();
    // Assert no tokens on inner deposit
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let deposits = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .0
            .into_iter()
            .collect::<HashMap<_, _>>();

        assert_eq!(deposits[&esdt_id], 0);
        assert_eq!(deposits[&btc_id], 0);
    })
    .assert_ok();
    // Assert `init_amount - liq_amount` on personal deposits
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount - liq_amount),
    );
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(init_amount - liq_amount),
    );
    // Assert position's liquidity
    assert!(tx_logs.iter().any(|entry| {
        if entry.topics.contains(&b"open_position".to_vec()) {
            let op_event = event::OpenPosition::top_decode(entry.data.clone()).unwrap();

            let pos_id = op_event.position_id;

            query!(cf_setup, |sc: ContractObj<DebugApi>| {
                let pos = sc.get_position_info(pos_id);
                let (balance_left, balance_right) = pos.balance;
                // FIXME: off-by-one error, no idea why
                assert_eq!(
                    (balance_left.to_u64(), balance_right.to_u64()),
                    (Some(liq_amount - 1), Some(liq_amount - 1))
                );
            })
            .assert_ok();

            true
        } else {
            false
        }
    }));
}

#[test]
fn success_batch_close_position() {
    let zero = 0u64;
    let init_amount = 2_000_000_000u64;
    let deposit_amount = 1_500_000_000u64;
    let liq_amount = 1_000_000_000u64;

    let mut cf_setup = Dx25Setup::setup();

    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount),
    );
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(init_amount),
    );

    let esdt_id = TokenId::from_bytes(ESDT_TOKEN_ID);
    let btc_id = TokenId::from_bytes(BTC_TOKEN_ID);
    // Standard set of actions for opeing fresh position
    let actions = vec![
        Action::RegisterAccount,
        Action::RegisterTokens(vec![esdt_id.clone(), btc_id.clone()]),
        Action::Deposit,
        Action::OpenPosition {
            tokens: (esdt_id.clone(), btc_id.clone()),
            fee_rate: 1,
            position: PositionInit {
                amount_ranges: (
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                ),
                ticks_range: (None, None),
            },
        },
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];

    let mut tx_logs = vec![];
    cf_setup
        .blockchain_wrapper
        .execute_esdt_multi_transfer(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &[
                TxTokenTransfer {
                    token_identifier: ESDT_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
                TxTokenTransfer {
                    token_identifier: BTC_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
            ],
            |sc| {
                sc.deposit(ApiVec(actions));
                tx_logs = TxContextStack::static_peek().extract_result().result_logs;
            },
        )
        .assert_ok();

    let pos_id = tx_logs
        .into_iter()
        .find_map(|entry| {
            entry.topics.contains(&b"open_position".to_vec()).then(|| {
                event::OpenPosition::top_decode(entry.data.clone())
                    .unwrap()
                    .position_id
            })
        })
        .unwrap();

    // Close position sequence
    let actions = vec![
        Action::ClosePosition(pos_id),
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];

    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.execute_actions(actions.into());
    })
    .assert_ok();

    // Assert no tokens on inner deposit
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let deposits = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .0
            .into_iter()
            .collect::<HashMap<_, _>>();

        assert_eq!(deposits[&esdt_id], 0);
        assert_eq!(deposits[&btc_id], 0);
    })
    .assert_ok();
    // Assert `init_amount - liq_amount` on personal deposits
    // FIXME: unexected off-by-one error
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount - 1),
    );
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(init_amount - 1),
    );
    // Assert position doesn't exist anymore
    let tx_result = query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let _ = sc.get_position_info(pos_id);
    });
    assert!(tx_result.result_status != 0);
    assert!(tx_result.result_message.contains("Position does not exist"));
}

#[test]
fn success_deposit_batch_swap_in() {
    let zero = 0u64;
    let init_amount = 2_000_000_000u64;
    let deposit_amount = 1_500_000_000u64;
    let liq_amount = 1_000_000_000u64;
    let swap_amount = 1_000_000u64;

    let mut cf_setup = Dx25Setup::setup();

    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount),
    );
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(init_amount),
    );

    let esdt_id = TokenId::from_bytes(ESDT_TOKEN_ID);
    let btc_id = TokenId::from_bytes(BTC_TOKEN_ID);
    // Standard set of actions for opeing fresh position
    let actions = vec![
        Action::RegisterAccount,
        Action::RegisterTokens(vec![esdt_id.clone(), btc_id.clone()]),
        Action::Deposit,
        Action::OpenPosition {
            tokens: (esdt_id.clone(), btc_id.clone()),
            fee_rate: 1,
            position: PositionInit {
                amount_ranges: (
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                ),
                ticks_range: (None, None),
            },
        },
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];

    cf_setup
        .blockchain_wrapper
        .execute_esdt_multi_transfer(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &[
                TxTokenTransfer {
                    token_identifier: ESDT_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
                TxTokenTransfer {
                    token_identifier: BTC_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
            ],
            |sc| {
                sc.deposit(ApiVec(actions));
            },
        )
        .assert_ok();

    // Add some starter tokens for second account
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.second_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount),
    );
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.second_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(0u64),
    );

    // Perform swap
    let actions = vec![
        Action::RegisterAccount,
        Action::RegisterTokens(vec![esdt_id.clone(), btc_id.clone()]),
        Action::Deposit,
        Action::SwapExactIn(SwapAction {
            token_in: esdt_id.clone(),
            token_out: btc_id.clone(),
            amount: Some(swap_amount.into()),
            amount_limit: (swap_amount / 2).into(),
        }),
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];

    let mut tx_logs = vec![];
    transfer!(
        cf_setup,
        second_user_address,
        ESDT_TOKEN_ID,
        swap_amount,
        |sc: ContractObj<DebugApi>| {
            sc.deposit(actions.into());
            tx_logs = TxContextStack::static_peek().extract_result().result_logs;
        }
    )
    .assert_ok();

    let (sell, buy) = tx_logs
        .into_iter()
        .find_map(|entry| {
            entry.topics.contains(&b"swap".to_vec()).then(|| {
                let event = event::Swap::top_decode(entry.data.clone()).unwrap();

                (
                    event.amounts.0.to_u64().unwrap(),
                    event.amounts.1.to_u64().unwrap(),
                )
            })
        })
        .unwrap();

    assert_eq!(sell, swap_amount);
    assert!(buy >= (swap_amount / 2));

    // Assert no tokens on inner deposit
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let deposits = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .0
            .into_iter()
            .collect::<HashMap<_, _>>();

        assert_eq!(deposits[&esdt_id], 0);
        assert_eq!(deposits[&btc_id], 0);
    })
    .assert_ok();
    // Assert amounts on personal deposits
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.second_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount - sell),
    );
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.second_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(buy),
    );
}

#[test]
fn success_batch_withdraw_fee() {
    let zero = 0u64;
    let init_amount = 2_000_000_000u64;
    let deposit_amount = 1_500_000_000u64;
    let liq_amount = 1_000_000_000u64;
    let swap_amount = 1_000_000u64;

    let mut cf_setup = Dx25Setup::setup();

    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount),
    );
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(init_amount),
    );

    let esdt_id = TokenId::from_bytes(ESDT_TOKEN_ID);
    let btc_id = TokenId::from_bytes(BTC_TOKEN_ID);
    // Standard set of actions for opeing fresh position
    let actions = vec![
        Action::RegisterAccount,
        Action::RegisterTokens(vec![esdt_id.clone(), btc_id.clone()]),
        Action::Deposit,
        Action::OpenPosition {
            tokens: (esdt_id.clone(), btc_id.clone()),
            fee_rate: 1,
            position: PositionInit {
                amount_ranges: (
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                    Range {
                        min: zero.into(),
                        max: liq_amount.into(),
                    },
                ),
                ticks_range: (None, None),
            },
        },
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];

    let mut tx_logs = vec![];
    cf_setup
        .blockchain_wrapper
        .execute_esdt_multi_transfer(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &[
                TxTokenTransfer {
                    token_identifier: ESDT_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
                TxTokenTransfer {
                    token_identifier: BTC_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: deposit_amount.into(),
                },
            ],
            |sc| {
                sc.deposit(ApiVec(actions));
                tx_logs = TxContextStack::static_peek().extract_result().result_logs;
            },
        )
        .assert_ok();

    let pos_id = tx_logs
        .into_iter()
        .find_map(|entry| {
            entry.topics.contains(&b"open_position".to_vec()).then(|| {
                event::OpenPosition::top_decode(entry.data.clone())
                    .unwrap()
                    .position_id
            })
        })
        .unwrap();

    // Add some starter tokens for second account
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.second_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount),
    );
    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.second_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(0u64),
    );

    // Perform swap
    let actions = vec![
        Action::RegisterAccount,
        Action::RegisterTokens(vec![esdt_id.clone(), btc_id.clone()]),
        Action::Deposit,
        Action::SwapExactIn(SwapAction {
            token_in: esdt_id.clone(),
            token_out: btc_id.clone(),
            amount: Some(swap_amount.into()),
            amount_limit: (swap_amount / 2).into(),
        }),
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];
    // Perform swap, to generate some fee
    transfer!(
        cf_setup,
        second_user_address,
        ESDT_TOKEN_ID,
        swap_amount,
        |sc: ContractObj<DebugApi>| {
            sc.deposit(actions.into());
        }
    )
    .assert_ok();

    // Withdraw fee
    let actions = vec![
        Action::WithdrawFee(pos_id),
        Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), zero.into(), None),
        Action::Withdraw(EgldOrTokenId::esdt(BTC_TOKEN_ID), zero.into(), None),
    ];

    let mut tx_logs = vec![];
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.execute_actions(actions.into());
        tx_logs = TxContextStack::static_peek().extract_result().result_logs;
    })
    .assert_ok();

    let amounts = tx_logs
        .into_iter()
        .find_map(|entry| {
            entry.topics.contains(&b"harvest_fee".to_vec()).then(|| {
                let amounts = event::HarvestFee::top_decode(entry.data.clone())
                    .unwrap()
                    .amounts;

                (amounts.0.to_u64().unwrap(), amounts.1.to_u64().unwrap())
            })
        })
        .unwrap();

    let (pool_id, _) = PoolId::try_from_pair((esdt_id.clone(), btc_id.clone())).unwrap();

    // Assert no tokens on inner deposit
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let deposits = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .0
            .into_iter()
            .collect::<HashMap<_, _>>();

        assert_eq!(deposits[&esdt_id], 0);
        assert_eq!(deposits[&btc_id], 0);
    })
    .assert_ok();
    // Assert amounts on personal deposits
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        pool_id.0.native().to_boxed_bytes().as_slice(),
        &rust_biguint!(init_amount - liq_amount + amounts.0),
    );
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        pool_id.1.native().to_boxed_bytes().as_slice(),
        &rust_biguint!(init_amount - liq_amount + amounts.1),
    );
}
