#[macro_use]
mod contract_builder;

use std::collections::HashMap;

use multiversx_sc::types::BigUint;
use multiversx_sc_scenario::{rust_biguint, DebugApi};

use dx25_client_sc::{ContractObj as ClientContractObj, Dx25ClientContract as _};

use dx25::{
    api_types::{ApiVec, MethodCall},
    chain::TokenId,
    ContractObj, Dx25Contract, EgldOrTokenId,
};

use contract_builder::{Dx25Setup, ESDT_TOKEN_ID, WEGLD_TOKEN_ID};

#[test]
#[allow(clippy::too_many_lines)]
fn test_sc_withdraw() {
    let mut cf_setup = Dx25Setup::setup();

    // ----- Deposits ------
    // Deposit EGLD
    transfer_egld!(cf_setup, client_address, 1000, |sc: ContractObj<
        DebugApi,
    >| {
        sc.deposit(ApiVec::default());
        let deposit = sc.get_deposit(
            cf_setup.client_address.clone().into(),
            TokenId::from_bytes(WEGLD_TOKEN_ID),
        );
        assert_eq!(deposit, 1000);
    })
    .assert_ok();

    // Deposit tokens
    transfer!(
        cf_setup,
        client_address,
        ESDT_TOKEN_ID,
        1000,
        |sc: ContractObj<DebugApi>| {
            sc.deposit(ApiVec::default());
            let deposit = sc.get_deposit(
                cf_setup.client_address.clone().into(),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            );
            assert_eq!(deposit, 1000);
        }
    )
    .assert_ok();

    // Check all user2tokens-related API's
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        // Get user deposits
        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.client_address.clone().into())
            .into();

        assert_eq!(deposits.len(), 2);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 1000);
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 1000);
    })
    .assert_ok();

    // Check if ESDT succesfully deposited
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.client_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(0),
    );

    // Check if EGL succesfully deposited
    cf_setup
        .blockchain_wrapper
        .check_egld_balance(&cf_setup.client_address, &rust_biguint!(0));

    // ------ Withdraw direct -----------
    // Try to withdraw half of the tokens
    transaction!(cf_setup, client_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw(
            EgldOrTokenId::esdt(ESDT_TOKEN_ID),
            BigUint::from(500u64),
            None,
        );
    })
    .assert_ok();

    // Try to withdraw half of the coins
    transaction!(cf_setup, client_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw(
            EgldOrTokenId::esdt(WEGLD_TOKEN_ID),
            BigUint::from(500u64),
            None,
        );
    })
    .assert_ok();

    // Check all user2tokens-related API's
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        // Get user deposits
        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.client_address.clone().into())
            .into();

        assert_eq!(deposits.len(), 2);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 500);
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 500);
    })
    .assert_ok();

    // Check if ESDT succesfully deposited
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.client_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(500),
    );

    // Check if succesfully withdrawn half of the tokens
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.client_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(500),
    );

    // ------ Withdraw function -----------
    // Check if function exists
    cf_setup
        .blockchain_wrapper
        .execute_tx(
            &cf_setup.owner_address,
            &cf_setup.client_wrapper,
            &rust_biguint!(0u64),
            |sc: ClientContractObj<DebugApi>| sc.receive_tokens(),
        )
        .assert_ok();

    // NOTE!!: If this test fails with `Invalid function` error,
    // you've probably forgot to build client smart contract
    // Run `./build-client-sc.sh`
    // Try to withdraw half of the tokens
    transaction!(cf_setup, client_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw(
            EgldOrTokenId::esdt(ESDT_TOKEN_ID),
            BigUint::from(500u64),
            Some(MethodCall {
                entrypoint: dx25_client_sc::PAYABLE_METHOD.into(),
                arguments: vec![].into(),
            }),
        );
    })
    .assert_ok();

    // Try to withdraw half of the coins
    transaction!(cf_setup, client_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw(
            EgldOrTokenId::esdt(WEGLD_TOKEN_ID),
            BigUint::from(500u64),
            Some(MethodCall {
                entrypoint: dx25_client_sc::PAYABLE_METHOD.into(),
                arguments: vec![].into(),
            }),
        );
    })
    .assert_ok();

    // Check all user2tokens-related API's
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        // Get user deposits
        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.client_address.clone().into())
            .into();

        assert_eq!(deposits.len(), 2);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 0);
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 0);
    })
    .assert_ok();

    // Check if ESDT succesfully withdrawn
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.client_address,
        ESDT_TOKEN_ID,
        &rust_biguint!(1000),
    );

    // Check if wEGLD succesfully withdrawn
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.client_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(1000),
    );
}
