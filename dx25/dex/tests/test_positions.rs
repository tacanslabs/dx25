#[macro_use]
mod contract_builder;

use multiversx_sc::types::TokenIdentifier;
use multiversx_sc_codec::TopDecode;
use multiversx_sc_scenario::{rust_biguint, DebugApi};

use dx25::{
    api_types::{Action, ApiVec},
    dex::{PositionId, PositionInit},
    events::event,
    ContractObj, Dx25Contract, Float, TokenId,
};

use contract_builder::{error_wrapper::TestResult, Dx25Setup, BTC_TOKEN_ID, ESDT_TOKEN_ID};

#[test]
#[allow(clippy::too_many_lines)]
fn test_positions() {
    let mut cf_setup = Dx25Setup::setup();

    // Open position with invalid fees
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let _position = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            0,
            PositionInit::new_full_range(0u32, 100u32, 0u32, 100u32),
        );
    })
    .assert_failed("Illegal fee");

    eprintln!("OK: Open position with invalid fees");

    // Open position with invalid deposit
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let _position = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(100u32, 1000u32, 100u32, 1000u32),
        );
    })
    .assert_failed("Not enough tokens in deposit");

    eprintln!("OK: Open position with invalid deposit");

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

    // Open with empty pool
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let _position = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(100u32, 1000u32, 0u32, 0u32),
        );
    })
    .assert_failed("Slippage error");

    eprintln!("OK: Open with empty pool");

    let mut position_id1: PositionId = 0;

    // Open valid position
    let tx_result = transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let (pos_id, amount1, amount2, _liquidity) = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(100u32, 900u32, 100u32, 900u32),
        );

        position_id1 = pos_id;
        assert_eq!(amount1, 900);
        assert_eq!(amount2, 900);
        // TODO: Proper assert for float value.
        // assert_eq!(liquidity, 1000.into());
    });

    tx_result.assert_ok();

    eprintln!("OK: Open valid position");

    // Check logs
    assert_eq!(tx_result.result_logs.len(), 4);

    assert!(tx_result.result_logs[0]
        .topics
        .contains(&b"tick_update".to_vec()));
    assert!(tx_result.result_logs[1]
        .topics
        .contains(&b"tick_update".to_vec()));

    let open_position_log = &tx_result.result_logs[2];
    assert!(open_position_log
        .topics
        .contains(&b"open_position".to_vec()));

    let open_position_event =
        event::OpenPosition::top_decode(open_position_log.data.clone()).unwrap();
    assert_eq!(
        open_position_event.user.to_address(),
        cf_setup.first_user_address
    );
    assert_eq!(
        open_position_event.pool,
        (
            TokenIdentifier::from_esdt_bytes(ESDT_TOKEN_ID),
            TokenIdentifier::from_esdt_bytes(BTC_TOKEN_ID)
        )
    );
    assert_eq!(open_position_event.amounts, (900u32.into(), 900u32.into()));
    assert_eq!(open_position_event.fee_rate, 16);
    assert_eq!(open_position_event.position_id, position_id1);

    // Get postition info
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let position_info = sc.get_position_info(position_id1);

        assert_eq!(
            position_info.tokens_ids,
            (
                TokenId::from_bytes(ESDT_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID)
            )
        );
        assert_eq!(position_info.balance, (899u32.into(), 899u32.into()));
    })
    .assert_ok();

    eprintln!("OK: Get position info");

    let mut position_id2: PositionId = 0;
    // Open second position
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let (pos_id, amount1, amount2, _liquidity) = sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            16,
            PositionInit::new_full_range(10u32, 100u32, 10u32, 100u32),
        );

        position_id2 = pos_id;
        assert_eq!(amount1, 100);
        assert_eq!(amount2, 100);
        // TODO: Proper assert for float value.
        // assert_eq!(liquidity, 1000.into());
    })
    .assert_ok();

    eprintln!("OK: Open second position");

    // Get multiple postitions info
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let position_info = sc.get_positions_info(vec![position_id1, position_id2]);

        assert_eq!(
            position_info[0]
                .as_ref()
                .map(|info| (info.tokens_ids.clone(), info.balance.clone())),
            Some((
                (
                    TokenId::from_bytes(ESDT_TOKEN_ID),
                    TokenId::from_bytes(BTC_TOKEN_ID)
                ),
                (899u32.into(), 899u32.into())
            ))
        );

        assert_eq!(
            position_info[1]
                .as_ref()
                .map(|info| (info.tokens_ids.clone(), info.balance.clone())),
            Some((
                (
                    TokenId::from_bytes(ESDT_TOKEN_ID),
                    TokenId::from_bytes(BTC_TOKEN_ID)
                ),
                (99u32.into(), 99u32.into())
            ))
        );
    })
    .assert_ok();

    // Get pool info
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let pool_info = sc
            .get_pool_info((
                TokenId::from_bytes(ESDT_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID),
            ))
            .unwrap();

        assert_eq!(pool_info.total_reserves, (1000u32.into(), 1000u32.into()));
        assert_eq!(pool_info.fee_divisor, 10000);
        assert_eq!(pool_info.fee_rates, [1, 2, 4, 8, 16, 32, 64, 128]);
        // TODO: Proper assert allowing some error
        // assert_eq!(pool_info.liquidities, [0, 0, 0, 0, 1000, 0, 0, 0]);
    })
    .assert_ok();

    eprintln!("OK: Get pool info");

    // Withdraw fee from an invalid user
    transaction!(cf_setup, second_user_address, |sc: ContractObj<
        DebugApi,
    >| {
        sc.withdraw_fee(position_id1);
    })
    .assert_failed("Account not registered");

    eprintln!("OK: Withdraw fee from an invalid user");

    // Withdraw fee
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        let fee = sc.withdraw_fee(position_id1);

        assert_eq!(fee, (0u32.into(), 0u32.into()));
    })
    .assert_ok();

    eprintln!("OK: Withdraw fee");

    // Close position
    let tx_result = transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.close_position(position_id1);
    });

    tx_result.assert_ok();

    eprintln!("OK: Close position");

    // Check logs
    assert_eq!(tx_result.result_logs.len(), 5);

    assert!(tx_result.result_logs[0]
        .topics
        .contains(&b"tick_update".to_vec()));
    assert!(tx_result.result_logs[1]
        .topics
        .contains(&b"tick_update".to_vec()));
    assert!(tx_result.result_logs[2]
        .topics
        .contains(&b"harvest_fee".to_vec()));

    let close_position_log = &tx_result.result_logs[3];
    assert!(close_position_log
        .topics
        .contains(&b"close_position".to_vec()));

    let close_position_event =
        event::ClosePosition::top_decode(close_position_log.data.clone()).unwrap();
    assert_eq!(close_position_event.position_id, position_id1);
    assert_eq!(close_position_event.amounts, (899u32.into(), 899u32.into()));
}

#[test]
fn test_liqudity_fee_level_distribution() {
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

    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            2,
            PositionInit::new_full_range(0u32, amount * 3, 0u32, amount * 3),
        );
    })
    .assert_ok();

    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.open_position(
            &TokenId::from_bytes(ESDT_TOKEN_ID),
            &TokenId::from_bytes(BTC_TOKEN_ID),
            4,
            PositionInit::new_full_range(0u32, amount * 5, 0u32, amount * 5),
        );
    })
    .assert_ok();

    // Test distribution
    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        let liquidities = sc
            .get_liqudity_fee_level_distribution((
                TokenId::from_bytes(ESDT_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID),
            ))
            .unwrap();

        // Preciseness is random here
        // Should be something like [20.000000000000004, 29.999999999999996, 50.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        assert!((Float::from(liquidities[0].clone()) - 20.0.into()).abs() < 0.0001.into());
        assert!((Float::from(liquidities[1].clone()) - 30.0.into()).abs() < 0.0001.into());
        assert!((Float::from(liquidities[2].clone()) - 50.0.into()).abs() < 0.0001.into());
    })
    .assert_ok();
}

#[test]
fn test_single_token_positions() {
    let mut cf_setup = Dx25Setup::setup();

    // Deposit a single token this should autoregister token, but fail with a slipage error
    transfer!(
        cf_setup,
        first_user_address,
        ESDT_TOKEN_ID,
        1000,
        |sc: ContractObj<DebugApi>| {
            sc.deposit(
                vec![Action::OpenPosition {
                    tokens: (
                        TokenId::from_bytes(ESDT_TOKEN_ID),
                        TokenId::from_bytes(BTC_TOKEN_ID),
                    ),
                    fee_rate: 16,
                    position: PositionInit::new_full_range(0u32, 0u32, 100u32, 900u32),
                }]
                .into(),
            );
        }
    )
    .assert_failed("Slippage error");
}
