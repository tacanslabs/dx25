#[macro_use]
mod contract_builder;

use multiversx_sc::types::BigUint;
use multiversx_sc_scenario::{rust_biguint, DebugApi};

use dx25::{api_types::ApiVec, dex::PositionInit, ContractObj, Dx25Contract, TokenId};

use contract_builder::{Dx25Setup, BTC_TOKEN_ID, ESDT_TOKEN_ID};

#[test]
fn test_estimation() {
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

    let amount = 100u32;

    // Open positions
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            1,
            PositionInit::new_full_range(0u32, amount * 2, 0u32, amount * 2),
        );
    })
    .assert_ok();

    // Get postition info
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let estimation = sc.estimate_swap_exact(
            true,
            TokenId::from_bytes(ESDT_TOKEN_ID),
            TokenId::from_bytes(BTC_TOKEN_ID),
            100u32.into(),
            1,
        );

        assert_eq!(estimation.result, BigUint::from(66u16));
    })
    .assert_ok();
}
