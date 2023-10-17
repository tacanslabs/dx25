use multiversx_sc::types::{
    ContractCall, ContractCallNoPayment, ManagedArgBuffer, ManagedBuffer, TokenIdentifier,
};

use crate::{
    api_types::{ApiVec, MethodCall, Withdrawal},
    chain::{AccountId, Amount, TokenId},
    dex::{self, Result, StateMut},
    CallbackProxy, Dx25Contract, WasmAmount,
};

/// Contains set of asynchronous withdrawals which are then enqueued for execution
///
/// FIXME: there's `promise` feature and promise-based API, although it's not documented
/// and seems to be "unstable"
#[must_use]
pub struct SendBatch;

impl SendBatch {
    /// Process outcomes of `send_tokens` calls and start async calls chain, if necessary
    ///
    /// **Attention**: this call must be the last one in any contract method since it may diverge
    ///
    /// * If any item in sequence is an `Err(_)`, call fails with first such error
    /// * All `Ok(None)`'s are filtered out since they designate synchronous withdrawals which already happened
    /// * If resulting sequence is empty, `Ok(())` is returned, i.e. no batch at all
    /// * If final sequence isn't empty, function diverges into async call
    pub fn try_handle_outcomes<C: Dx25Contract>(
        contract: &C,
        outcomes: impl IntoIterator<Item = Result<Option<Withdrawal>>>,
    ) -> Result<()> {
        let mut withdrawals = Vec::new();

        for item in outcomes {
            if let Some(item) = item? {
                withdrawals.push(item);
            }
        }

        Self::handle_withdrawals(contract, withdrawals);
        Ok(())
    }
    /// Handle set of asynchronous withdrawals
    ///
    /// **Attention**: this method should be used as last call in `Dx25Contract::withdraw_callback`
    /// since it handles transfers which are known to be async, and may diverge into more async calls
    ///
    /// If input vector is empty, returns immediately;
    /// otherwise diverges into asynchronous call by scheduling first
    /// transfer and passing rest as more transfers in chain
    pub fn handle_withdrawals<C: Dx25Contract>(contract: &C, mut withdrawals: Vec<Withdrawal>) {
        if withdrawals.is_empty() {
            return;
        }

        let head = withdrawals.remove(0);
        let tail = withdrawals;

        Self::call_contract(contract, head, ApiVec(tail));
    }
    /// Either sends tokens to addressee synchronously or prepares `Withdrawal` data for later
    /// asynchronous transfer
    ///
    /// Tracks withdraw if it's asynchronous.
    ///
    /// # Parameters
    /// * `contract` - reference to contract instance
    /// * `account_id` - target account
    /// * `token_id` - identifier of token to send
    /// * `amount` - amount of token to send
    /// * `unwrap` - `true` if token in question is a wrapped eGld token which must be unwrapped before send
    /// * `method_call` - receiver method, if receiver is a contract
    ///
    /// # Returns
    /// * `Ok(None)` - if tokens were successfully transferred synchronously
    /// * `Ok(Some(withdrawal))` - if transfer should be asynchronous, for later
    ///     packing into `SendBatch`
    /// * `Err(error)` - if any error happened; method call should fail if encounters this
    pub fn send_sync_or_return_withdrawal<C: Dx25Contract, F: FnOnce(Amount)>(
        contract: &C,
        account_id: &AccountId,
        token_id: &TokenId,
        amount: Amount,
        unwrapper: Option<F>,
        callback: Option<MethodCall>,
    ) -> Result<Option<Withdrawal>> {
        // Scenarios:
        // 1. Transfer to user - just use `direct_esdt`
        // 2. Transfer to user with unwrap - unwrap, then `direct_egld`
        // 3. Transfer to contract - use async contract_call
        // 4. Transfer to contract with unwrap - unwrap, then either `direct_egld` or via method call

        // If transfer:
        //      If user: direct_esdt
        //      Else contract:
        //          If
        //

        let dx25_address = account_id.to_address().into();
        let is_contract = contract.blockchain().is_smart_contract(&dx25_address);
        let dx25_contract = contract;
        // Receiver is either just user or there's no callback to invoke -> perform direct
        // transfer
        if !is_contract || callback.is_none() {
            if let Some(unwrapper) = unwrapper {
                unwrapper(amount);

                dx25_contract
                    .send()
                    .direct_egld(&dx25_address, &amount.into());
            } else {
                dx25_contract.send().direct_esdt(
                    &dx25_address,
                    &TokenIdentifier::<C::Api>::from_esdt_bytes(token_id.native().to_boxed_bytes()),
                    0,
                    &amount.into(),
                );
            }

            return Ok(None);
        }
        // Always `Some(...)` due to condition above
        let Some(callback) = callback else { unreachable!() };
        let (entrypoint, arguments): (ManagedBuffer<C::Api>, ManagedArgBuffer<C::Api>) = (
            callback.entrypoint.as_str().into(),
            callback.arguments.0.as_slice().into(),
        );

        // FIXME: Blockchain mock doesn't implement `get_shard_of_address`, even as of 0.41.2
        // So we always assume we do cross-contract call
        //
        // let is_same_shard = dx25_contract
        //     .blockchain()
        //     .get_shard_of_address(&dx25_contract.blockchain().get_sc_address())
        //     == dx25_contract
        //         .blockchain()
        //         .get_shard_of_address(&dx25_address);

        let is_same_shard = false;

        // Receiver is a contract in the same shard, and there's a callback -> sync call
        if is_same_shard {
            if let Some(unwrapper) = unwrapper {
                unwrapper(amount);

                dx25_contract
                    .send()
                    .contract_call::<()>(dx25_address, entrypoint)
                    .with_egld_transfer(amount.into())
                    .with_raw_arguments(arguments)
                    .transfer_execute();
            } else {
                Self::prepare_esdt_transfer(
                    dx25_contract,
                    account_id,
                    token_id,
                    amount,
                    entrypoint,
                    arguments,
                )
                .transfer_execute();
            }

            Ok(None)
        }
        // Receiver is a contract on a different shard
        else {
            let mut dex = dx25_contract.as_dex_mut();
            let contract = dex.contract_mut().latest();

            contract
                .accounts
                .try_update(account_id, |dex::Account::V0(ref mut acc)| {
                    // Track transfer
                    acc.withdraw_tracker.track(token_id.clone(), amount);
                    // Finally, return withdraw payload
                    Ok(Some(Withdrawal {
                        account_id: account_id.to_address(),
                        token_id: token_id.clone(),
                        amount,
                        callback: Some(callback),
                    }))
                })
        }
    }

    fn prepare_esdt_transfer<C: Dx25Contract>(
        contract: &C,
        account_id: &AccountId,
        token_id: &TokenId,
        amount: Amount,
        entrypoint: ManagedBuffer<C::Api>,
        arguments: ManagedArgBuffer<C::Api>,
    ) -> ContractCallNoPayment<C::Api, ()> {
        let mut args = ManagedArgBuffer::new();

        // Push token_id and amount at the beginning of the args as `ESDTTransfer` requires
        args.push_arg(token_id.native());
        args.push_arg(WasmAmount::from(amount));

        args.push_arg(entrypoint);
        args = args.concat(arguments);

        // Create async call
        contract
            .send()
            .contract_call::<()>(account_id.to_address().into(), "ESDTTransfer".into())
            .with_raw_arguments(args)
    }

    // Async contract call
    fn call_contract<C: Dx25Contract>(
        contract: &C,
        mut head: Withdrawal,
        tail: ApiVec<Withdrawal>,
    ) {
        let mut args = ManagedArgBuffer::new();

        // Unpack call method and arguments
        // Using empty buffer as an Address makes a call directly to the contract
        let (endpoint_name, call_args) = head.callback.take().map_or(
            (
                ManagedBuffer::<C::Api>::new(),
                ManagedArgBuffer::<C::Api>::new(),
            ),
            |call| (call.entrypoint.as_str().into(), call.arguments.0.into()),
        );

        // Push token_id and amount at the beginning of the args as `ESDTTransfer` requires
        args.push_arg(head.token_id.native());
        args.push_arg(WasmAmount::from(head.amount));

        // Push contract endpoint and endpoint args if provided
        if !endpoint_name.is_empty() {
            args.push_arg(endpoint_name);
            args = args.concat(call_args);
        }

        // Create async call
        contract
            .send()
            .contract_call::<()>(head.account_id.clone().into(), "ESDTTransfer".into())
            .with_raw_arguments(args)
            .async_call()
            .with_callback(contract.callbacks().withdraw_callback(head, tail))
            .call_and_exit();
    }
}
