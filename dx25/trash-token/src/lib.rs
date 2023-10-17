#![no_std]

multiversx_sc::imports!();

/// Dx25 trash token smart contract
#[multiversx_sc::contract]
pub trait Dx25TrashTokenContract {
    #[view]
    #[storage_mapper("tokens")]
    fn tokens(&self) -> SetMapper<TokenIdentifier>;

    #[storage_mapper("baseIssuingCost")]
    fn base_issuing_cost(&self) -> SingleValueMapper<BigUint>;

    /// Multiversx blockchain requires
    /// exactly this amount of EGLD to be paid to issue tokens.
    /// And exactly this amount of trash tokens will be allocated on issuing.
    /// Currently it is 5_000_000_000_000_000_000 for all networks
    /// * `baseIssuingCost` is a network config value for minimum issuing cost
    #[init]
    #[payable("EGLD")]
    fn init(&self, base_issuing_cost: BigUint) {
        self.base_issuing_cost().set(base_issuing_cost);
    }

    /// Calls the system contract to issue tokens.
    fn system_issue(
        &self,
        token_name: &ManagedBuffer,
        token_ticker: &ManagedBuffer,
        num_decimals: usize,
    ) {
        let tokens_to_issue = self.base_issuing_cost().get();

        self.send()
            .esdt_system_sc_proxy()
            .issue_fungible(
                tokens_to_issue.clone(),
                token_name,
                token_ticker,
                &tokens_to_issue,
                FungibleTokenProperties {
                    num_decimals,
                    can_freeze: true,
                    can_wipe: false,
                    can_pause: false,
                    can_mint: true,
                    can_burn: true,
                    can_change_owner: false,
                    can_upgrade: true,
                    can_add_special_roles: true,
                },
            )
            .async_call()
            .with_callback(self.callbacks().issue_callback())
            .call_and_exit();
    }

    #[endpoint]
    fn issue(&self, token_name: &ManagedBuffer, token_ticker: ManagedBuffer, num_decimals: usize) {
        self.blockchain().check_caller_is_owner();

        self.system_issue(token_name, &token_ticker, num_decimals);
    }

    #[endpoint]
    fn mint(&self, token_id: TokenIdentifier, amount: BigUint) {
        if !self.tokens().contains(&token_id) {
            sc_panic!("Token is not registered");
        }

        self.send().esdt_local_mint(&token_id, 0, &amount);
        self.send()
            .direct_esdt(&self.blockchain().get_caller(), &token_id, 0, &amount);
    }

    /// Register token manually if something went wrong during issuing
    #[endpoint]
    fn register_token(&self, token_id: TokenIdentifier) {
        self.blockchain().check_caller_is_owner();

        self.register_issued_token(token_id);
    }

    fn register_issued_token(&self, token_id: TokenIdentifier) {
        self.tokens().insert(token_id.clone());

        // Token issuer can't mint tokens by default. How cool is that?
        // Anyways let's allow the contract to mint issued tokens
        self.send()
            .esdt_system_sc_proxy()
            .set_special_roles(
                &self.blockchain().get_sc_address(),
                &token_id,
                [EsdtLocalRole::Mint].into_iter(),
            )
            .async_call()
            .call_and_exit_ignore_callback();
    }

    #[callback]
    fn issue_callback(&self, #[call_result] result: ManagedAsyncCallResult<()>) {
        match result {
            ManagedAsyncCallResult::Ok(()) => {
                let (token_id, _) = self.call_value().single_fungible_esdt();

                self.register_issued_token(token_id);
            }
            ManagedAsyncCallResult::Err(message) => {
                sc_panic!(message.err_msg);
            }
        }
    }
}
