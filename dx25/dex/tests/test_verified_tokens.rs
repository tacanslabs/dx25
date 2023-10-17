#[macro_use]
mod contract_builder;

use std::collections::HashSet;

use multiversx_sc::types::TokenIdentifier;
use multiversx_sc_codec::TopDecode;
use multiversx_sc_scenario::{rust_biguint, DebugApi};

use dx25::{chain::TokenId, events::event, ContractObj, Dx25Contract};

use contract_builder::{error_wrapper::TestResult, Dx25Setup, BTC_TOKEN_ID, ESDT_TOKEN_ID};

use crate::contract_builder::WEGLD_TOKEN_ID;

#[test]
#[allow(clippy::too_many_lines)]
fn test_verified_tokens() {
    let mut cf_setup = Dx25Setup::setup();

    // Try to extend verified token by a user
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        // NB: wEGLD is a verified token at init
        assert!(sc.get_verified_tokens().0 == [TokenId::from_bytes(WEGLD_TOKEN_ID)]);
        sc.extend_verified_tokens(vec![TokenId::from_bytes(ESDT_TOKEN_ID)].into());
    })
    .assert_failed("Permission denied");

    // Add verified tokens
    let tx_result = transaction!(cf_setup, owner_address, |sc: ContractObj<DebugApi>| {
        assert!(sc.get_verified_tokens().0 == [TokenId::from_bytes(WEGLD_TOKEN_ID)]);
        sc.extend_verified_tokens(vec![TokenId::from_bytes(ESDT_TOKEN_ID)].into());

        let verified_tokens: HashSet<_> = sc.get_verified_tokens().into();
        assert_eq!(verified_tokens.len(), 2); // Plus wEGLD
        assert!(verified_tokens.contains(&TokenId::from_bytes(ESDT_TOKEN_ID)));
    });

    tx_result.assert_ok();

    // Check logs
    assert_eq!(tx_result.result_logs.len(), 1);

    let add_log = &tx_result.result_logs[0];
    assert!(add_log.topics.contains(&b"add_verified_tokens".to_vec()));

    let add_tokens_event = event::AddVerifiedTokens::top_decode(add_log.data.clone()).unwrap();
    assert_eq!(add_tokens_event.tokens.0.len(), 1);
    assert!(add_tokens_event
        .tokens
        .0
        .contains(&TokenIdentifier::from_esdt_bytes(ESDT_TOKEN_ID)));

    // Try to add the same token
    transaction!(cf_setup, owner_address, |sc: ContractObj<DebugApi>| {
        sc.extend_verified_tokens(vec![TokenId::from_bytes(ESDT_TOKEN_ID)].into());

        let verified_tokens: HashSet<_> = sc.get_verified_tokens().into();
        assert_eq!(verified_tokens.len(), 2);
        assert!(verified_tokens.contains(&TokenId::from_bytes(ESDT_TOKEN_ID)));
    })
    .assert_ok();

    // Add second verified tokens
    transaction!(cf_setup, owner_address, |sc: ContractObj<DebugApi>| {
        sc.extend_verified_tokens(vec![TokenId::from_bytes(BTC_TOKEN_ID)].into());

        let verified_tokens: HashSet<_> = sc.get_verified_tokens().into();
        assert_eq!(verified_tokens.len(), 3);
        assert!(verified_tokens.contains(&TokenId::from_bytes(ESDT_TOKEN_ID)));
        assert!(verified_tokens.contains(&TokenId::from_bytes(BTC_TOKEN_ID)));
    })
    .assert_ok();

    // Remove verified tokens
    let tx_result = transaction!(cf_setup, owner_address, |sc: ContractObj<DebugApi>| {
        sc.remove_verified_tokens(
            vec![
                TokenId::from_bytes(ESDT_TOKEN_ID),
                TokenId::from_bytes(BTC_TOKEN_ID),
            ]
            .into(),
        );

        let verified_tokens: HashSet<_> = sc.get_verified_tokens().into();
        assert_eq!(verified_tokens.len(), 1);
    });

    tx_result.assert_ok();

    // Check logs
    assert_eq!(tx_result.result_logs.len(), 1);

    let remove_log = &tx_result.result_logs[0];
    assert!(remove_log
        .topics
        .contains(&b"remove_verified_tokens".to_vec()));

    let remove_tokens_event =
        event::RemoveVerifiedTokens::top_decode(remove_log.data.clone()).unwrap();
    assert_eq!(remove_tokens_event.tokens.0.len(), 2);
    assert!(remove_tokens_event
        .tokens
        .0
        .contains(&TokenIdentifier::from_esdt_bytes(ESDT_TOKEN_ID)));
    assert!(remove_tokens_event
        .tokens
        .0
        .contains(&TokenIdentifier::from_esdt_bytes(BTC_TOKEN_ID)));
}
