#![no_std]

use multiversx_sc::storage::mappers::SingleValueMapper;

pub const PAYABLE_METHOD: &str = "receive_tokens";

/// Smart contract for testing purposes.
/// Dx25 crate builds the cntract and uses bytecode to test withdraws.
#[multiversx_sc::contract]
pub trait Dx25ClientContract {
    #[storage_mapper("dx25_sc_address")]
    fn dx25_sc_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[init]
    fn init(&self, dex_sc_address: ManagedAddress) {
        self.dx25_sc_address().set(dex_sc_address);
    }

    /// Function to test withdrawing tokens using a smart contract method
    #[endpoint]
    #[payable("*")]
    fn receive_tokens(&self) {}
}
