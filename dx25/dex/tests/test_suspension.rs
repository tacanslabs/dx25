#[macro_use]
mod contract_builder;

use dx25::{api_types::ApiVec, AccountId, ContractObj, Dx25Contract};

use contract_builder::{error_wrapper::TestResult, Dx25Setup};

use multiversx_sc_scenario::{rust_biguint, DebugApi};

#[test]
fn test_suspension() {
    let mut cf_setup = Dx25Setup::setup();

    // No permissions to suspend
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.suspend_payable_api();
    })
    .assert_failed("Permission denied");

    // No permissions to add a guard
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.add_guard_accounts(ApiVec::default());
    })
    .assert_failed("Permission denied");

    let guard_address = AccountId::from_address(&cf_setup.first_user_address);

    // The owner adds a guard
    transaction!(cf_setup, owner_address, |sc: ContractObj<DebugApi>| {
        sc.add_guard_accounts(ApiVec::from(vec![guard_address.clone()]));
    })
    .assert_ok();

    // A guard suspends API
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.suspend_payable_api();
    })
    .assert_ok();

    // Failed to use suspended API
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.withdraw_fee(0);
    })
    .assert_failed("Payable API suspended");

    // Resume API
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.resume_payable_api();
    })
    .assert_ok();

    // Remove guard
    transaction!(cf_setup, owner_address, |sc: ContractObj<DebugApi>| {
        sc.remove_guard_accounts(ApiVec::from(vec![guard_address.clone()]));
    })
    .assert_ok();

    // No permissions to suspend anymore
    transaction!(cf_setup, first_user_address, |sc: ContractObj<DebugApi>| {
        sc.suspend_payable_api();
    })
    .assert_failed("Permission denied");
}
