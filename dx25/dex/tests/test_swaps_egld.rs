#[macro_use]
mod contract_builder;

use std::collections::HashMap;

use multiversx_sc::types::BigUint;
use multiversx_sc_scenario::{rust_biguint, DebugApi};

use dx25::{
    api_types::ApiVec, dex::PositionInit, ContractObj, Dx25Contract, EgldOrTokenId, TokenId,
};

use contract_builder::{error_wrapper::TestResult, Dx25Setup, BTC_TOKEN_ID, WEGLD_TOKEN_ID};

#[test]
#[allow(clippy::too_many_lines)]
fn test_swaps_egld() {
    // Need to use Dx25ClientSetup here, because we deposit EGLD
    let mut cf_setup = Dx25Setup::setup();

    // Deposit EGLD
    transfer_egld!(cf_setup, first_user_address, 1000, |sc: ContractObj<
        DebugApi,
    >| {
        sc.deposit(ApiVec::default());
        let deposit = sc.get_deposit(
            cf_setup.first_user_address.clone().into(),
            TokenId::from_bytes(WEGLD_TOKEN_ID),
        );
        assert_eq!(deposit, 1000);
    })
    .assert_ok();

    // Deposit tokens
    transfer!(
        cf_setup,
        first_user_address,
        BTC_TOKEN_ID,
        1000,
        |sc: ContractObj<DebugApi>| {
            sc.deposit(ApiVec::default());
            let deposit = sc.get_deposit(
                cf_setup.first_user_address.clone().into(),
                TokenId::from_bytes(BTC_TOKEN_ID),
            );
            assert_eq!(deposit, 1000);
        }
    )
    .assert_ok();

    // Position to close later
    let mut position_id = 0u64;

    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let pos = sc.open_position(
            &TokenId::from_bytes(WEGLD_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(0u32, 1000u32, 0u32, 1000u32),
        );

        position_id = pos.0;
    })
    .assert_ok();

    // Deposit tokens
    transfer!(
        cf_setup,
        second_user_address,
        BTC_TOKEN_ID,
        1000,
        |sc: ContractObj<DebugApi>| {
            sc.deposit(ApiVec::default());
            let deposit = sc.get_deposit(
                cf_setup.second_user_address.clone().into(),
                TokenId::from_bytes(BTC_TOKEN_ID),
            );
            assert_eq!(deposit, 1000);
        }
    )
    .assert_ok();

    // Swap in tests
    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.swap_exact_in(
            vec![
                TokenId::from_bytes(WEGLD_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID),
            ]
            .into(),
            2000u32.into(),
            4000u32.into(),
        )
    })
    .assert_failed("Slippage error");

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let info = sc
            .get_pool_info((
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(WEGLD_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (1000u32.into(), 1000u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 1000);
    })
    .assert_ok();

    // Valid swap in
    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.swap_exact_in(
            vec![
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(WEGLD_TOKEN_ID),
            ]
            .into(),
            1000u32.into(),
            100u32.into(),
        )
    })
    .assert_ok();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let info = sc
            .get_pool_info((
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(WEGLD_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (2000u32.into(), 501u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 0);
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 499);
    })
    .assert_ok();

    // Insufficient liquidity
    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.swap_exact_out(
            vec![
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(WEGLD_TOKEN_ID),
            ]
            .into(),
            1000u32.into(),
            499u32.into(),
        )
    })
    .assert_failed("Insufficient liquidity in the pool to perform the swap");

    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.swap_exact_out(
            vec![
                TokenId::from_bytes(WEGLD_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID),
            ]
            .into(),
            900u32.into(),
            499u32.into(),
        )
    })
    .assert_ok();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let info = sc
            .get_pool_info((
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(WEGLD_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (1100u32.into(), 912u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 900);
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 88);
    })
    .assert_ok();

    // Close position and withdraw tokens
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.close_position(position_id);

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 1099);

        sc.withdraw(
            EgldOrTokenId::esdt(BTC_TOKEN_ID),
            BigUint::from(1099u64),
            None,
        );
    })
    .assert_ok();

    // Withdraw coins, because each withdrawal terminates contract call
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .into();

        // BTC withdrawed in the last transaction
        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 0);
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 910);

        sc.withdraw(
            EgldOrTokenId::esdt(WEGLD_TOKEN_ID),
            BigUint::from(910u64),
            None,
        );

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.first_user_address.clone().into())
            .into();

        // Sending direct EGLD doesn't terminate execution, so we can check here
        assert_eq!(deposits[&TokenId::from_bytes(WEGLD_TOKEN_ID)], 0);
    })
    .assert_ok();

    // Check if ESDT succesfully withdrawn
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        BTC_TOKEN_ID,
        &rust_biguint!(1099),
    );

    // Check if wEGLD succesfully withdrawn
    cf_setup.blockchain_wrapper.check_esdt_balance(
        &cf_setup.first_user_address,
        WEGLD_TOKEN_ID,
        &rust_biguint!(910),
    );
}
