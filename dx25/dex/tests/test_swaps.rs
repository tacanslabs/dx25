#[macro_use]
mod contract_builder;

use std::collections::HashMap;

use multiversx_sc_scenario::{rust_biguint, DebugApi};

use dx25::{
    api_types::{Action, ApiVec},
    dex::{PositionInit, SwapAction},
    ContractObj, Dx25Contract, Float, TokenId,
};

use contract_builder::{error_wrapper::TestResult, Dx25Setup, BTC_TOKEN_ID, ESDT_TOKEN_ID};

#[test]
#[allow(clippy::too_many_lines)]
fn test_swaps() {
    let mut cf_setup = Dx25Setup::setup();

    // Deposit tokens
    transfer!(
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
    )
    .assert_ok();

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

    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let _ = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(0u32, 1000u32, 0u32, 1000u32),
        );
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
                TokenId::from_bytes(ESDT_TOKEN_ID),
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
                TokenId::from_bytes(ESDT_TOKEN_ID),
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
                TokenId::from_bytes(ESDT_TOKEN_ID),
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
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (2000u32.into(), 501u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 0);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 499);
    })
    .assert_ok();

    // Insufficient liquidity
    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.swap_exact_out(
            vec![
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(ESDT_TOKEN_ID),
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
                TokenId::from_bytes(ESDT_TOKEN_ID),
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
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (1100u32.into(), 912u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 900);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 88);
    })
    .assert_ok();

    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        let (amount_in, amount_out) = sc.swap_to_price(
            vec![
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ]
            .into(),
            900u32.into(),
            Float::from(1000.0).try_into().unwrap(),
        );

        assert_eq!(amount_in, 900);
        assert_eq!(amount_out, 409);
    })
    .assert_ok();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let info = sc
            .get_pool_info((
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (2000u32.into(), 503u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 0);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 497);
    })
    .assert_ok();

    // Also, let's check partial swap
    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        let (amount_in, amount_out) = sc.swap_to_price(
            vec![
                TokenId::from_bytes(ESDT_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID),
            ]
            .into(),
            497u32.into(),
            Float::from(0.5).try_into().unwrap(),
        );

        assert_eq!(amount_in, 207);
        assert_eq!(amount_out, 581);
    })
    .assert_ok();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let info = sc
            .get_pool_info((
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (1419u32.into(), 710u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 581);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 290);
    })
    .assert_ok();
}

#[test]
#[allow(clippy::too_many_lines)]
fn test_actions() {
    let mut cf_setup = Dx25Setup::setup();

    // Deposit tokens
    transfer!(
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
    )
    .assert_ok();

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

    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let _ = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(0u32, 1000u32, 0u32, 1000u32),
        );
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

    let action1 = Action::SwapExactIn(SwapAction {
        token_in: TokenId::from_bytes(BTC_TOKEN_ID),
        token_out: TokenId::from_bytes(ESDT_TOKEN_ID),
        amount: Some(1000u32.into()),
        amount_limit: 0u32.into(),
    });

    let action2 = Action::SwapExactOut(SwapAction {
        token_in: TokenId::from_bytes(ESDT_TOKEN_ID),
        token_out: TokenId::from_bytes(BTC_TOKEN_ID),
        amount: Some(900u32.into()),
        amount_limit: 499u32.into(),
    });

    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.execute_actions(vec![action1, action2].into());
    })
    .assert_ok();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let info = sc
            .get_pool_info((
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (1100u32.into(), 912u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 900);
        assert_eq!(deposits[&TokenId::from_bytes(ESDT_TOKEN_ID)], 88);
    })
    .assert_ok();
}

#[test]
#[allow(clippy::too_many_lines)]
fn test_invalid_swap_in() {
    let mut cf_setup = Dx25Setup::setup();

    // Deposit tokens
    transfer!(
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
    )
    .assert_ok();

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

    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let _ = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(0u32, 1000u32, 0u32, 1000u32),
        );
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
                TokenId::from_bytes(ESDT_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID),
            ]
            .into(),
            2000u32.into(),
            4000u32.into(),
        )
    })
    .assert_failed("Slippage error");

    // Not enough tokens
    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.swap_exact_in(
            vec![
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ]
            .into(),
            2000u32.into(),
            500u32.into(),
        )
    })
    .assert_failed("Not enough tokens in deposit");

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let info = sc
            .get_pool_info((
                TokenId::from_bytes(BTC_TOKEN_ID),
                TokenId::from_bytes(ESDT_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(info.total_reserves, (1000u32.into(), 1000u32.into()));

        let deposits: HashMap<_, _> = sc
            .get_deposits(cf_setup.second_user_address.clone().into())
            .into();

        assert_eq!(deposits[&TokenId::from_bytes(BTC_TOKEN_ID)], 1000);
        assert!(!deposits.contains_key(&TokenId::from_bytes(ESDT_TOKEN_ID)));
    })
    .assert_ok();
}
