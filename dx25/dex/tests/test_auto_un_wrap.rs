#![deny(warnings)]
#![allow(clippy::too_many_lines)]

use std::collections::HashMap;

use contract_builder::{Dx25Setup, ESDT_TOKEN_ID, WEGLD_TOKEN_ID};
use dx25::{
    api_types::ApiVec,
    chain::wasm::api_types::Action,
    dex::{PositionInit, SwapAction},
    Dx25Contract, EgldOrTokenId, TokenId,
};
use multiversx_sc_codec::TopDecode;
use multiversx_sc_scenario::rust_biguint;

#[macro_use]
mod contract_builder;

type Dx25ContractObj = dx25::ContractObj<multiversx_sc_scenario::DebugApi>;

#[test]
fn success_deposit_withdraw_egld() {
    let init_amount = 2_000_000_000u64;
    let deposit_amount = 1_500_000_000u64;

    let mut cf_setup = Dx25Setup::setup();

    cf_setup
        .blockchain_wrapper
        .set_egld_balance(&cf_setup.first_user_address, &rust_biguint!(init_amount));
    //
    // Deposit eGld
    //
    cf_setup
        .blockchain_wrapper
        .execute_tx(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &rust_biguint!(deposit_amount),
            |sc| {
                sc.deposit(ApiVec::default());
            },
        )
        .assert_ok();

    cf_setup.blockchain_wrapper.check_egld_balance(
        &cf_setup.first_user_address,
        &rust_biguint!(init_amount - deposit_amount),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(0),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        cf_setup.cf_wrapper.address_ref(),
        WEGLD_TOKEN_ID,
        &rust_biguint!(deposit_amount),
    );
    //
    // Withdraw eGld
    //
    cf_setup
        .blockchain_wrapper
        .execute_tx(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &rust_biguint!(0),
            |sc| {
                sc.withdraw(EgldOrTokenId::egld(), rust_biguint!(0).into(), None);
            },
        )
        .assert_ok();

    cf_setup
        .blockchain_wrapper
        .check_egld_balance(&cf_setup.first_user_address, &rust_biguint!(init_amount));

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(0),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        cf_setup.cf_wrapper.address_ref(),
        WEGLD_TOKEN_ID,
        &rust_biguint!(0),
    );
}

#[test]
fn success_open_swap_fee_close_batch_egld() {
    let mut cf_setup = Dx25Setup::setup();

    let init_amount = 10_000_000_000u64;

    let esdt_id = TokenId::from_bytes(ESDT_TOKEN_ID);
    let wegld_id = TokenId::from_bytes(WEGLD_TOKEN_ID);

    cf_setup
        .blockchain_wrapper
        .set_egld_balance(&cf_setup.first_user_address, &rust_biguint!(init_amount));

    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount),
    );

    cf_setup
        .blockchain_wrapper
        .set_egld_balance(&cf_setup.second_user_address, &rust_biguint!(init_amount));

    //
    // Open position
    //

    let liq_deposit_amount = init_amount / 2;
    let liquidity_amount = liq_deposit_amount - liq_deposit_amount / 10;

    // Can't deposit EGLD and ESDT at the same time, deposit ESDT as separate call
    transfer!(
        cf_setup,
        first_user_address,
        ESDT_TOKEN_ID,
        liq_deposit_amount,
        |sc: Dx25ContractObj| {
            sc.deposit(ApiVec::default());
        }
    )
    .assert_ok();

    let result = transfer_egld!(
        cf_setup,
        first_user_address,
        liq_deposit_amount,
        |sc: Dx25ContractObj| {
            sc.deposit(
                vec![
                    Action::Deposit,
                    Action::OpenPosition {
                        tokens: (esdt_id.clone(), wegld_id.clone()),
                        fee_rate: 1,
                        position: PositionInit::new_full_range(
                            0u64,
                            liquidity_amount,
                            0u64,
                            liquidity_amount,
                        ),
                    },
                    Action::Withdraw(EgldOrTokenId::egld(), 0u64.into(), None),
                    Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), 0u64.into(), None),
                ]
                .into(),
            );
        }
    );
    result.assert_ok();

    let pos_id = result
        .result_logs
        .iter()
        .find_map(|log| {
            log.topics.contains(&b"open_position".to_vec()).then(|| {
                dx25::events::event::OpenPosition::top_decode(log.data.as_slice())
                    .unwrap()
                    .position_id
            })
        })
        .unwrap();

    query!(cf_setup, |sc: Dx25ContractObj| {
        let deposits: HashMap<_, _> = sc
            .get_deposits((&cf_setup.first_user_address).into())
            .0
            .into_iter()
            .collect();

        assert_eq!(deposits.get(&esdt_id), Some(&0u64.into()));
        assert_eq!(deposits.get(&wegld_id), Some(&0u64.into()));
    })
    .assert_ok();

    cf_setup.blockchain_wrapper.check_egld_balance(
        &cf_setup.first_user_address,
        &rust_biguint!(init_amount - liquidity_amount),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount - liquidity_amount),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(0),
    );

    //
    // Swap-in (second account)
    //

    let deposit_amount = init_amount / 100;
    let swap_amount = deposit_amount - deposit_amount / 10;
    let swap_limit = swap_amount / 2;

    let result = transfer_egld!(
        cf_setup,
        second_user_address,
        deposit_amount,
        |sc: Dx25ContractObj| {
            sc.deposit(
                vec![
                    Action::Deposit,
                    Action::SwapExactIn(SwapAction {
                        token_in: wegld_id.clone(),
                        token_out: esdt_id.clone(),
                        amount: Some(swap_amount.into()),
                        amount_limit: swap_limit.into(),
                    }),
                    Action::Withdraw(EgldOrTokenId::egld(), 0u64.into(), None),
                    Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), 0u64.into(), None),
                ]
                .into(),
            );
        }
    );
    result.assert_ok();

    let bought_esdt = result
        .result_logs
        .iter()
        .find_map(|log| {
            log.topics.contains(&b"swap".to_vec()).then(|| {
                dx25::events::event::Swap::top_decode(log.data.as_slice())
                    .unwrap()
                    .amounts
                    .1
                    .to_u64()
                    .unwrap()
            })
        })
        .unwrap();

    query!(cf_setup, |sc: Dx25ContractObj| {
        let deposits: HashMap<_, _> = sc
            .get_deposits((&cf_setup.second_user_address).into())
            .0
            .into_iter()
            .collect();

        assert_eq!(deposits.get(&esdt_id), Some(&0u64.into()));
        assert_eq!(deposits.get(&wegld_id), Some(&0u64.into()));
    })
    .assert_ok();

    cf_setup.blockchain_wrapper.check_egld_balance(
        &cf_setup.second_user_address,
        &rust_biguint!(init_amount - swap_amount),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.second_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(bought_esdt),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.second_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(0),
    );

    //
    // Withdraw fee
    //

    let (_, swapped) =
        dx25::dex::PoolId::try_from_pair((wegld_id.clone(), esdt_id.clone())).unwrap();

    let result = transaction!(cf_setup, first_user_address, |sc: Dx25ContractObj| {
        sc.execute_actions(
            vec![
                Action::WithdrawFee(pos_id),
                Action::Withdraw(EgldOrTokenId::egld(), 0u64.into(), None),
                Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), 0u64.into(), None),
            ]
            .into(),
        );
    });
    result.assert_ok();

    let fees = result
        .result_logs
        .iter()
        .find_map(|log| {
            log.topics.contains(&b"harvest_fee".to_vec()).then(|| {
                let event =
                    dx25::events::event::HarvestFee::top_decode(log.data.as_slice()).unwrap();
                assert_eq!(event.position_id, pos_id);
                (
                    event.amounts.0.to_u64().unwrap(),
                    event.amounts.1.to_u64().unwrap(),
                )
            })
        })
        .unwrap();

    let (egld_fee, esdt_fee) = if swapped { (fees.1, fees.0) } else { fees };

    query!(cf_setup, |sc: Dx25ContractObj| {
        let deposits: HashMap<_, _> = sc
            .get_deposits((&cf_setup.first_user_address).into())
            .0
            .into_iter()
            .collect();

        assert_eq!(deposits.get(&esdt_id), Some(&0u64.into()));
        assert_eq!(deposits.get(&wegld_id), Some(&0u64.into()));
    })
    .assert_ok();

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(0),
    );

    cf_setup.blockchain_wrapper.check_egld_balance(
        &cf_setup.first_user_address,
        &rust_biguint!(init_amount - liquidity_amount + egld_fee),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount - liquidity_amount + esdt_fee),
    );

    //
    // Close position
    //

    let result = transaction!(cf_setup, first_user_address, |sc: Dx25ContractObj| {
        sc.execute_actions(
            vec![
                Action::ClosePosition(pos_id),
                Action::Withdraw(EgldOrTokenId::egld(), 0u64.into(), None),
                Action::Withdraw(EgldOrTokenId::esdt(ESDT_TOKEN_ID), 0u64.into(), None),
            ]
            .into(),
        );
    });
    result.assert_ok();

    let liquidities = result
        .result_logs
        .iter()
        .find_map(|log| {
            log.topics.contains(&b"close_position".to_vec()).then(|| {
                let event =
                    dx25::events::event::ClosePosition::top_decode(log.data.as_slice()).unwrap();
                assert_eq!(event.position_id, pos_id);
                (
                    event.amounts.0.to_u64().unwrap(),
                    event.amounts.1.to_u64().unwrap(),
                )
            })
        })
        .unwrap();

    let (egld_liq, esdt_liq) = if swapped {
        (liquidities.1, liquidities.0)
    } else {
        liquidities
    };

    query!(cf_setup, |sc: Dx25ContractObj| {
        let deposits: HashMap<_, _> = sc
            .get_deposits((&cf_setup.first_user_address).into())
            .0
            .into_iter()
            .collect();

        assert_eq!(deposits.get(&esdt_id), Some(&0u64.into()));
        assert_eq!(deposits.get(&wegld_id), Some(&0u64.into()));
    })
    .assert_ok();

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(0),
    );

    cf_setup.blockchain_wrapper.check_egld_balance(
        &cf_setup.first_user_address,
        &rust_biguint!(init_amount - liquidity_amount + egld_fee + egld_liq),
    );

    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(init_amount - liquidity_amount + esdt_fee + esdt_liq),
    );
}
