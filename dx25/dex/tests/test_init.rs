#[macro_use]
mod contract_builder;

use dx25::{api_types::ApiVec, token_id::TokenId, ContractObj, Dx25Contract};

use contract_builder::Dx25Setup;

use multiversx_sc_scenario::{testing_framework::ScCallMandos, DebugApi};

#[test]
fn test_init() {
    let mut cf_setup = Dx25Setup::setup();

    query!(cf_setup, |sc: ContractObj<DebugApi>| {
        assert_eq!(sc.get_owner(), cf_setup.owner_address.clone().into());
    })
    .assert_ok();
}

/// Utility test to generate serialized function arguments
#[ignore]
#[test]
fn gen_mandos() {
    let mut cf_setup = Dx25Setup::setup();

    let mut call = ScCallMandos::new(
        &cf_setup.owner_address,
        cf_setup.cf_wrapper.address_ref(),
        "extend_verified_tokens",
    );
    call.add_argument(&ApiVec::from(vec![
        TokenId::<DebugApi>::from_bytes(b"ETH-78f83a"),
        TokenId::<DebugApi>::from_bytes(b"BTC-9a6eb0"),
        TokenId::<DebugApi>::from_bytes(b"USDT-3f0fe6"),
        TokenId::<DebugApi>::from_bytes(b"USDC-35dd62"),
        TokenId::<DebugApi>::from_bytes(b"TRASH-7c1af8"),
        TokenId::<DebugApi>::from_bytes(b"AQA1-d8c759"),
        TokenId::<DebugApi>::from_bytes(b"AQA2-9fb507"),
    ]));
    cf_setup.blockchain_wrapper.add_mandos_sc_call(call, None);
    cf_setup.blockchain_wrapper.write_mandos_output("gen_test");
}
