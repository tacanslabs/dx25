#[macro_use]
mod contract_builder;

use std::collections::{HashMap, HashSet};

use multiversx_sc::types::BigUint;

use multiversx_sc_scenario::{rust_biguint, testing_framework::TxTokenTransfer, DebugApi};

use multiversx_sc_codec::TopDecode;

use dx25::{
    api_types::{Action, ApiVec},
    chain::{wasm::events::event, TokenId},
    ContractObj, Dx25Contract, EgldOrTokenId,
};

use contract_builder::{
    error_wrapper::TestResult, Dx25Setup, BTC_TOKEN_ID, ESDT_TOKEN_ID, WEGLD_TOKEN_ID,
};

#[test]
#[allow(clippy::too_many_lines)]
fn test_deposit_withdraw() {
    let mut cf_setup = Dx25Setup::setup();

    // Deposit tokens
    let tx_result = transfer!(
        cf_setup,
        first_user_address,
        ESDT_TOKEN_ID,
        1000,
        |sc: ContractObj<DebugApi>| {
            sc.deposit(ApiVec::default());
            let deposit = sc.get_deposit(
                cf_setup.first_user_address.clone().into(),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            );
            assert_eq!(deposit, 1000);
        }
    );

    tx_result.assert_ok();

    // Check balance
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(0),
    );
    cf_setup.blockchain_wrapper.check_esdt_balance(
        cf_setup.cf_wrapper.address_ref(),
        ESDT_TOKEN_ID,
        &rust_biguint!(1000),
    );

    // Check logs
    assert_eq!(tx_result.result_logs.len(), 1);

    let deposit_log = &tx_result.result_logs[0];
    assert!(deposit_log.topics.contains(&b"deposit".to_vec()));

    let deposit_event = event::Deposit::top_decode(deposit_log.data.clone()).unwrap();
    assert_eq!(deposit_event.user.to_address(), cf_setup.first_user_address);
    assert_eq!(
        deposit_event.token_id.to_boxed_bytes().as_slice(),
        ESDT_TOKEN_ID
    );
    assert_eq!(deposit_event.amount, 1000);
    assert_eq!(deposit_event.balance, 1000);

    // Check all user2tokens-related API's
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        // Get user deposits
        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .into();

        assert_eq!(deposits.len(), 1);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 1000);

        // Get user deposit
        let deposit = sc.get_deposit(
            cf_setup.first_user_address.clone().into(),
            TokenId::from_bytes(ESDT_TOKEN_ID),
        );

        assert_eq!(deposit, 1000);

        // Get user tokens
        let user_toks: HashSet<_> = sc
            .get_user_tokens(cf_setup.first_user_address.clone().into())
            .into();
        assert_eq!(user_toks.len(), 1);
        assert!(user_toks.contains(&TokenId::from_bytes(ESDT_TOKEN_ID)));
    })
    .assert_ok();

    // Try to withdraw invalid amount
    let result = transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw(
            EgldOrTokenId::esdt(ESDT_TOKEN_ID),
            BigUint::from(1001u64),
            None,
        );
    });

    result.assert_failed("Not enough tokens in deposit");

    // Try to withdraw half of the tokens
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw(
            EgldOrTokenId::esdt(ESDT_TOKEN_ID),
            BigUint::from(500u64),
            None,
        );
    })
    .assert_ok();

    // Check balance
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(500),
    );
    cf_setup.blockchain_wrapper.check_esdt_balance(
        cf_setup.cf_wrapper.address_ref(),
        ESDT_TOKEN_ID,
        &rust_biguint!(500),
    );

    // Try to withdraw other half of the tokens
    let tx_result = transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw(
            EgldOrTokenId::esdt(ESDT_TOKEN_ID),
            BigUint::from(500u64),
            None,
        );
    });

    tx_result.assert_ok();

    // Check balance
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(1000),
    );
    cf_setup.blockchain_wrapper.check_esdt_balance(
        cf_setup.cf_wrapper.address_ref(),
        ESDT_TOKEN_ID,
        &rust_biguint!(0),
    );

    // Check logs
    // FIXME: Logs cannot be correctly checked ATM - `BlockchainStateWrapper::execute_tx`
    // used to implement contract call ignores result produced by callback.
    // Since we must write withdraw event in that specific callback, we cannot
    // test ATM that event was written correctly
    /*
    assert_eq!(tx_result.result_logs.len(), 2);

    let withdraw_log = &tx_result.result_logs[0];
    assert!(withdraw_log.topics.contains(&b"withdraw".to_vec()));

    let withdraw_event = event::Withdraw::top_decode(withdraw_log.data.clone()).unwrap();
    assert_eq!(
        withdraw_event.user.to_address(),
        cf_setup.first_user_address
    );
    assert_eq!(
        withdraw_event.token_id.to_boxed_bytes().as_slice(),
        ESDT_TOKEN_ID
    );
    assert_eq!(withdraw_event.amount, 500);
    assert_eq!(withdraw_event.balance, 0);
    */
}

#[test]
fn test_multiple_deposits() {
    let mut cf_setup = Dx25Setup::setup();

    cf_setup.blockchain_wrapper.set_esdt_balance(
        &cf_setup.first_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(1000),
    );

    cf_setup
        .blockchain_wrapper
        .execute_esdt_multi_transfer(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &[
                TxTokenTransfer {
                    token_identifier: ESDT_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: rust_biguint!(100),
                },
                TxTokenTransfer {
                    token_identifier: BTC_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: rust_biguint!(200),
                },
            ],
            |sc| {
                sc.deposit(ApiVec::default());
            },
        )
        .assert_ok();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        // Get user deposits
        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .into();

        assert_eq!(deposits.len(), 2);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 100);
        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 200);
    })
    .assert_ok();

    cf_setup
        .blockchain_wrapper
        .execute_esdt_multi_transfer(
            &cf_setup.first_user_address,
            &cf_setup.cf_wrapper,
            &[
                TxTokenTransfer {
                    token_identifier: ESDT_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: rust_biguint!(200),
                },
                TxTokenTransfer {
                    token_identifier: WEGLD_TOKEN_ID.to_vec(),
                    nonce: 0,
                    value: rust_biguint!(500),
                },
            ],
            |sc| sc.deposit(vec![Action::Deposit].into()),
        )
        .assert_ok();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        // Get user deposits
        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .into();

        assert_eq!(deposits.len(), 3);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 300);
        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 200);
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 500);
    })
    .assert_ok();
}
