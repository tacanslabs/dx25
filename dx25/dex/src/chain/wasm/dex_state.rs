use multiversx_sc::{
    contract_base::ErrorHelper,
    types::{ContractCall, EsdtTokenPayment},
};
use multiversx_sc_codec::multi_types::IgnoreValue;
use multiversx_wegld_swap_sc::ProxyTrait;

use super::{events::Logger, send_batch::SendBatch};
use crate::{
    api_types::{into_account_id, MethodCall, Withdrawal},
    chain::{AccountId, Amount, TokenId, Types},
    dex::{self, Contract},
    item_factory::ItemFactory,
    Dx25Contract, WEGLD_NOT_INIT_ERROR,
};
use std::marker::PhantomData;

/// State mapper implementation for the contract.
pub struct StateWrapper<C: Dx25Contract> {
    /// Preloaded value to return to users
    contract_instance: Contract<Types<C::Api>>,
    _phantom: PhantomData<C>,
}

impl<C: Dx25Contract> StateWrapper<C> {
    pub fn new(contract: &C) -> Self {
        Self {
            contract_instance: contract.contract_state().get(),
            _phantom: PhantomData,
        }
    }
}

impl<C: Dx25Contract> dex::State<Types<C::Api>> for StateWrapper<C> {
    fn contract(&self) -> &dex::Contract<Types<C::Api>> {
        &self.contract_instance
    }
}

/// Mutable wrapper, which writes out changes into the blockchain on drop
pub struct StateMutWrapper<'a, C: Dx25Contract> {
    contract: &'a C,
    item_factory: ItemFactory<C::Api>,
    logger: Logger<'a, C>,
    /// Preloaded value to return to users to later write out changes
    contract_instance: Contract<Types<C::Api>>,
}

impl<'a, C: Dx25Contract> StateMutWrapper<'a, C> {
    pub fn new(contract: &'a C) -> Self {
        Self {
            contract,
            item_factory: contract.item_factory(),
            logger: Logger::new(contract),
            contract_instance: contract.contract_state().get(),
        }
    }

    pub(super) fn wegld(&mut self) -> Option<&(AccountId, TokenId)> {
        self.contract_instance.latest().extra.wegld.as_ref()
    }
}

impl<'a, C: Dx25Contract> dex::State<Types<C::Api>> for StateMutWrapper<'a, C> {
    fn contract(&self) -> &dex::Contract<Types<C::Api>> {
        &self.contract_instance
    }
}

impl<'a, C: Dx25Contract> dex::StateMut<Types<C::Api>> for StateMutWrapper<'a, C> {
    type SendTokensResult = dex::Result<Option<Withdrawal>>;
    type SendTokensExtraParam = (bool, Option<MethodCall>);

    fn members_mut(&mut self) -> dex::StateMembersMut<'_, Types<C::Api>> {
        dex::StateMembersMut {
            contract: &mut self.contract_instance,
            item_factory: &mut self.item_factory,
            logger: &mut self.logger,
        }
    }

    fn send_tokens(
        &mut self,
        account_id: &AccountId,
        token_id: &TokenId,
        amount: Amount,
        _unregister: bool,
        (unwrap, extra): Self::SendTokensExtraParam,
    ) -> Self::SendTokensResult {
        let unwrapper = if unwrap {
            let (address, token_id) = self.wegld().unwrap_or_else(|| {
                ErrorHelper::<C::Api>::signal_error_with_message(WEGLD_NOT_INIT_ERROR)
            });

            let (address, token_id) = (
                address.to_byte_array().into(),
                token_id.native().to_boxed_bytes().as_slice().into(),
            );

            let mut proxy = self.contract.wegld_swap_proxy(address);

            #[allow(clippy::semicolon_if_nothing_returned)]
            let cb = move |amount: Amount| {
                let _: IgnoreValue = proxy
                    .unwrap_egld()
                    .with_esdt_transfer(EsdtTokenPayment::new(token_id, 0, amount.into()))
                    .execute_on_dest_context();
            };

            Some(cb)
        } else {
            None
        };

        SendBatch::send_sync_or_return_withdrawal(
            self.contract,
            account_id,
            token_id,
            amount,
            unwrapper,
            extra,
        )
    }

    fn get_initiator_id(&self) -> AccountId {
        // Currently initiator is always the caller
        into_account_id(&self.contract.blockchain().get_caller())
    }

    fn get_caller_id(&self) -> AccountId {
        into_account_id(&self.contract.blockchain().get_caller())
    }
}

/// Save changed value of a mutable reference
/// Writing into a retrieved value doe nothing. We need to use mapper to write into the blockchain.
impl<'a, C: Dx25Contract> Drop for StateMutWrapper<'a, C> {
    fn drop(&mut self) {
        self.contract.contract_state().set(&self.contract_instance);
    }
}
