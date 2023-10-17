#![allow(dead_code)]

pub mod error_wrapper;

use std::path::Path;

use dx25::{wasm, Dx25Contract};
use multiversx_sc::types::{Address, EsdtLocalRole, ManagedAddress, TokenIdentifier};
use multiversx_sc_scenario::{
    rust_biguint,
    testing_framework::{BlockchainStateWrapper, ContractObjWrapper},
    DebugApi,
};

pub const ESDT_TOKEN_ID: &[u8] = b"DX25-ESDT";
pub const BTC_TOKEN_ID: &[u8] = b"DX25-BTC";
#[allow(dead_code)]
pub const WEGLD_TOKEN_ID: &[u8] = b"wEGLD";

pub const DX25_WASM_PATH: &str = "";
pub const CLIENT_SC_WASM_PATH: &str = "../client-sc/output/dx25-client-sc.wasm";
pub const WEGLD_SWAP_WASM_PATH: &str = "../wegld-swap/output/multiversx-wegld-swap-sc.wasm";

pub struct Dx25Setup {
    pub blockchain_wrapper: BlockchainStateWrapper,
    pub owner_address: Address,
    // Additional field to use in macros
    pub client_address: Address,
    pub first_user_address: Address,
    pub second_user_address: Address,
    pub cf_wrapper:
        ContractObjWrapper<wasm::ContractObj<DebugApi>, fn() -> wasm::ContractObj<DebugApi>>,
    pub client_wrapper: ContractObjWrapper<
        dx25_client_sc::ContractObj<DebugApi>,
        fn() -> dx25_client_sc::ContractObj<DebugApi>,
    >,
    pub wegld_swap_wrapper: ContractObjWrapper<
        multiversx_wegld_swap_sc::ContractObj<DebugApi>,
        fn() -> multiversx_wegld_swap_sc::ContractObj<DebugApi>,
    >,
}

impl Dx25Setup {
    pub fn setup() -> Self {
        let _ = DebugApi::dummy();

        assert!(Path::new(CLIENT_SC_WASM_PATH).exists() && Path::new(WEGLD_SWAP_WASM_PATH).exists(),
        "Please run './build-client-sc.sh' and './build-wegld-swap.sh' before running the tests");

        let rust_zero = rust_biguint!(0u64);
        let mut blockchain_wrapper = BlockchainStateWrapper::new();
        let owner_address = blockchain_wrapper.create_user_account(&rust_zero);
        let first_user_address = blockchain_wrapper.create_user_account(&rust_zero);
        let second_user_address = blockchain_wrapper.create_user_account(&rust_zero);

        blockchain_wrapper.set_esdt_balance(
            &first_user_address,
            ESDT_TOKEN_ID,
            &rust_biguint!(1_000),
        );
        blockchain_wrapper.set_esdt_balance(
            &first_user_address,
            BTC_TOKEN_ID,
            &rust_biguint!(1_000),
        );

        blockchain_wrapper.set_egld_balance(&first_user_address, &rust_biguint!(1_000));

        blockchain_wrapper.set_esdt_balance(
            &second_user_address,
            BTC_TOKEN_ID,
            &rust_biguint!(1_000),
        );

        // Main contract
        let cf_wrapper = blockchain_wrapper.create_sc_account(
            &rust_zero,
            Some(&owner_address),
            dx25::wasm::contract_obj as fn() -> wasm::ContractObj<DebugApi>,
            DX25_WASM_PATH,
        );

        // Client SC
        let client_wrapper = blockchain_wrapper.create_sc_account(
            &rust_zero,
            Some(&owner_address),
            dx25_client_sc::contract_obj as fn() -> dx25_client_sc::ContractObj<DebugApi>,
            CLIENT_SC_WASM_PATH, // Path to the client sc bytecode
        );

        // WEGLD swap
        let wegld_swap_wrapper = blockchain_wrapper.create_sc_account(
            &rust_biguint!(1_000_000_000),
            Some(&owner_address),
            multiversx_wegld_swap_sc::contract_obj
                as fn() -> multiversx_wegld_swap_sc::ContractObj<DebugApi>,
            WEGLD_SWAP_WASM_PATH, // Path to the wrapper sc bytecode
        );

        blockchain_wrapper.set_esdt_balance(
            client_wrapper.address_ref(),
            ESDT_TOKEN_ID,
            &rust_biguint!(1_000),
        );
        blockchain_wrapper.set_esdt_balance(
            client_wrapper.address_ref(),
            BTC_TOKEN_ID,
            &rust_biguint!(1_000),
        );

        blockchain_wrapper.set_egld_balance(client_wrapper.address_ref(), &rust_biguint!(1_000));

        // Set wrapper SC roles
        blockchain_wrapper.set_esdt_local_roles(
            wegld_swap_wrapper.address_ref(),
            WEGLD_TOKEN_ID,
            &[EsdtLocalRole::Burn, EsdtLocalRole::Mint],
        );

        // Client SC constructor
        blockchain_wrapper
            .execute_tx(&owner_address, &client_wrapper, &rust_zero, |sc| {
                use dx25_client_sc::Dx25ClientContract;
                sc.init(cf_wrapper.address_ref().into());
            })
            .assert_ok();

        // wEGLD wrap SC constructor
        blockchain_wrapper
            .execute_tx(&owner_address, &wegld_swap_wrapper, &rust_zero, |sc| {
                use multiversx_wegld_swap_sc::EgldEsdtSwap;
                sc.init(TokenIdentifier::from_esdt_bytes(WEGLD_TOKEN_ID));
            })
            .assert_ok();

        // Dx25 constructor
        blockchain_wrapper
            .execute_tx(&owner_address, &cf_wrapper, &rust_zero, |sc| {
                sc.init(
                    ManagedAddress::from_address(&owner_address),
                    1300,
                    [1, 2, 4, 8, 16, 32, 64, 128],
                );
                sc.init_wegld(
                    vec![ManagedAddress::from_address(
                        wegld_swap_wrapper.address_ref(),
                    )]
                    .into(),
                );
            })
            .assert_ok();

        Dx25Setup {
            blockchain_wrapper,
            owner_address,
            client_address: client_wrapper.address_ref().clone(),
            first_user_address,
            second_user_address,
            cf_wrapper,
            client_wrapper,
            wegld_swap_wrapper,
        }
    }
}

// We use macros, because we want to borrow blockchain wrapper mutably,
// but still have read access to other fields
#[allow(unused_macros)]
macro_rules! transaction {
    ($sc_setup:ident, $caller:ident, $func: expr) => {{
        // For some reason Debug API doesn't retrive logs, so we do it manually
        let mut tx_result = multiversx_sc_scenario::whitebox::TxResult::empty();

        let mut exec_result = $sc_setup.blockchain_wrapper.execute_tx(
            &$sc_setup.$caller,
            &$sc_setup.cf_wrapper,
            &rust_biguint!(0u64),
            |sc| {
                #[allow(clippy::redundant_closure_call)]
                ($func)(sc);

                // Get logs context
                tx_result = multiversx_sc_scenario::whitebox::TxContextStack::static_peek()
                    .extract_result();
            },
        );

        exec_result.result_logs = tx_result.result_logs;
        exec_result
    }};
}

#[allow(unused_macros)]
macro_rules! query {
    ($sc_setup:ident, $func: expr) => {{
        // For some reason Debug API doesn't retrive logs, so we do it manually
        let mut tx_result = multiversx_sc_scenario::whitebox::TxResult::empty();

        let mut exec_result =
            $sc_setup
                .blockchain_wrapper
                .execute_query(&$sc_setup.cf_wrapper, |sc| {
                    #[allow(clippy::redundant_closure_call)]
                    ($func)(sc);

                    // Get logs context
                    tx_result = multiversx_sc_scenario::whitebox::TxContextStack::static_peek()
                        .extract_result();
                });

        exec_result.result_logs = tx_result.result_logs;
        exec_result
    }};
}

#[allow(unused_macros)]
macro_rules! transfer {
    ($sc_setup:ident, $caller:ident, $token_id:ident, $amount: expr, $func: expr) => {{
        // For some reason Debug API doesn't retrive logs, so we do it manually
        let mut tx_result = multiversx_sc_scenario::whitebox::TxResult::empty();

        let mut exec_result = $sc_setup.blockchain_wrapper.execute_esdt_transfer(
            &$sc_setup.$caller,
            &$sc_setup.cf_wrapper,
            $token_id,
            0,
            &rust_biguint!($amount),
            |sc| {
                #[allow(clippy::redundant_closure_call)]
                ($func)(sc);

                // Get logs context
                tx_result = multiversx_sc_scenario::whitebox::TxContextStack::static_peek()
                    .extract_result();
            },
        );

        exec_result.result_logs = tx_result.result_logs;
        exec_result
    }};
}

#[allow(unused_macros)]
macro_rules! transfer_egld {
    ($sc_setup:ident, $caller:ident, $amount: expr, $func: expr) => {{
        // For some reason Debug API doesn't retrive logs, so we do it manually
        let mut tx_result = multiversx_sc_scenario::whitebox::TxResult::empty();

        let mut exec_result = $sc_setup.blockchain_wrapper.execute_tx(
            &$sc_setup.$caller,
            &$sc_setup.cf_wrapper,
            &rust_biguint!($amount),
            |sc| {
                #[allow(clippy::redundant_closure_call)]
                ($func)(sc);

                // Get logs context
                tx_result = multiversx_sc_scenario::whitebox::TxContextStack::static_peek()
                    .extract_result();
            },
        );

        exec_result.result_logs = tx_result.result_logs;
        exec_result
    }};
}
