use itertools::Itertools;
use multiversx_sc::{
    api::ErrorApi as _,
    contract_base::ProxyObjBase,
    err_msg, sc_panic,
    storage::mappers::SingleValueMapper,
    types::{ContractCall, EgldOrEsdtTokenIdentifier, ManagedAddress, ManagedAsyncCallResult},
};
use multiversx_sc_codec::multi_types::IgnoreValue;

use crate::{
    api_types::{
        into_token_id, Action, ApiMap, ApiVec, EstimateAddLiquidityResult, EstimateSwapExactResult,
        Fraction, MethodCall, PoolInfo, PositionInfo,
    },
    chain::{AccountId, Amount, Liquidity, TokenId, Types, VmApi},
    dex::pool::one_over_sqrt_one_minus_fee_rate,
    dex::{
        self, latest::RawFeeLevelsArray, BasisPoints, Contract, Estimations, FeeLevel,
        ItemFactory as _, Map, PairExt, PositionId, PositionInit, Set as _, State as _, StateMut,
        VersionInfo,
    },
    dex_state::{StateMutWrapper, StateWrapper},
    error_here, Float, WasmAmount, WEGLD_DOUBLE_INIT_ERROR,
};
use multiversx_wegld_swap_sc::ProxyTrait as _;

use super::{
    api_types::{Metadata, Withdrawal},
    dex_wrapper::DexWrapper,
    item_factory::ItemFactory,
    send_batch::SendBatch,
};

pub type EgldOrTokenId = EgldOrEsdtTokenIdentifier<VmApi>;
type Address = ManagedAddress<VmApi>;

#[multiversx_sc::contract]
pub trait Dx25Contract {
    #[proxy]
    fn wegld_swap_proxy(
        &self,
        sc_address: ManagedAddress,
    ) -> multiversx_wegld_swap_sc::Proxy<Self::Api>;

    #[storage_mapper("contract")]
    fn contract_state(&self) -> SingleValueMapper<Contract<Types<Self::Api>>>;

    /// Contract has a common namespace for all the storage mappers, so
    /// to create storage items like maps and sets dynamically, we need unique ID's for each of the items.
    /// This is a unique ID counter to give items unique_ids.
    #[storage_mapper("unique_storage_key_counter")]
    fn unique_id(&self) -> SingleValueMapper<u64>;

    #[event("log")]
    fn log(&self, data: ManagedBuffer);

    #[event("deposit")]
    fn log_deposit_event(&self, data: ManagedBuffer);

    #[event("withdraw")]
    fn log_withdraw_event(&self, data: ManagedBuffer);

    #[event("open_position")]
    fn log_open_position_event(&self, data: ManagedBuffer);

    #[event("harvest_fee")]
    fn log_harvest_fee_event(&self, data: ManagedBuffer);

    #[event("close_position")]
    fn log_close_position_event(&self, data: ManagedBuffer);

    #[event("swap")]
    fn log_swap_event(&self, data: ManagedBuffer);

    #[event("update_pool_state")]
    fn log_update_pool_state_event(&self, data: ManagedBuffer);

    #[event("add_verified_tokens")]
    fn log_add_verified_tokens_event(&self, data: ManagedBuffer);

    #[event("remove_verified_tokens")]
    fn log_remove_verified_tokens_event(&self, data: ManagedBuffer);

    #[event("add_guard_account")]
    fn log_add_guard_accounts_event(&self, data: ManagedBuffer);

    #[event("remove_guard_accounts")]
    fn log_remove_guard_accounts_event(&self, data: ManagedBuffer);

    #[event("suspend_payable_api")]
    fn log_suspend_payable_api_event(&self, data: ManagedBuffer);

    #[event("resume_payable_api")]
    fn log_resume_payable_api_event(&self, data: ManagedBuffer);

    #[event("tick_update")]
    fn log_tick_update_event(&self, data: ManagedBuffer);

    /// - `wegld_token_id` is wEGLD token ID, which we ask user to unwrap into
    /// EGLD to work with dx25
    #[init]
    fn init(
        &self,
        owner_id: AccountId,
        protocol_fee_fraction: BasisPoints,
        fee_rates: RawFeeLevelsArray<BasisPoints>,
    ) {
        // Do not erase storage if updating
        if !self.contract_state().is_empty() {
            return;
        }

        self.unique_id().set(1);

        let contract = self.result_unwrap(self.item_factory().new_contract(
            owner_id,
            protocol_fee_fraction,
            fee_rates,
        ));

        self.contract_state().set(contract);
    }

    #[endpoint(initWeGLD)]
    fn init_wegld(&self, network_wegld_wrappers: ApiVec<ManagedAddress>) {
        let mut dex = self.as_dex_mut();
        let caller = dex.get_caller_id();
        let contract = dex.contract_mut().latest();

        if caller != contract.owner_id
            && caller != self.blockchain().get_owner_address().to_byte_array().into()
        {
            self.fail(error_here!(dex::ErrorKind::PermissionDenied));
        }

        let wegld = &mut contract.extra.wegld;

        if wegld.is_some() {
            sc_panic!(WEGLD_DOUBLE_INIT_ERROR);
        }

        let wegld_address = self.get_wegld_address(network_wegld_wrappers.0);

        let wegld_token_id: TokenIdentifier = self
            .wegld_swap_proxy(wegld_address.clone())
            .wrapped_egld_token_id()
            .execute_on_dest_context();

        *wegld = Some((
            wegld_address.to_byte_array().into(),
            TokenId::from_bytes(wegld_token_id.to_boxed_bytes()),
        ));

        self.extend_verified_tokens(vec![into_token_id(&wegld_token_id)].into());
    }

    #[endpoint(init_wegld)]
    fn init_wegld_snake_case(&self, network_wegld_wrappers: ApiVec<ManagedAddress>) {
        self.init_wegld(network_wegld_wrappers);
    }

    #[cfg(target_arch = "wasm32")]
    fn get_wegld_address(&self, network_wegld_wrappers: Vec<ManagedAddress>) -> ManagedAddress {
        let own_shard = self
            .blockchain()
            .get_shard_of_address(&self.blockchain().get_sc_address());

        // We need wEGLD wrapper on the same shard. If we failed to find one, this is a critical failure
        let same_shard_wrapper = network_wegld_wrappers
            .into_iter()
            .find(|wrapper| self.blockchain().get_shard_of_address(wrapper) == own_shard);

        if let Some(wrapper) = same_shard_wrapper {
            wrapper
        } else {
            sc_panic!("Failed to find wEGLD wrapper in the same shard");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn get_wegld_address(&self, network_wegld_wrappers: Vec<ManagedAddress>) -> ManagedAddress {
        if let Some(wrapper) = network_wegld_wrappers.into_iter().next() {
            wrapper
        } else {
            sc_panic!("No wEGLD wrapper supplied")
        }
    }

    #[view]
    fn metadata(&self) -> Metadata {
        let dex = self.as_dex();
        let fee_rates = dex.fee_rates_ticks();
        let contract = dex.contract().as_ref();

        Metadata {
            owner: contract.owner_id.clone(),
            pool_count: contract.pool_count,
            protocol_fee_fraction: contract.protocol_fee_fraction,
            fee_rates,
            fee_divisor: dex::BASIS_POINT_DIVISOR,
        }
    }

    /// Returns balances of the deposits for given user outside of any pools.
    /// Returns empty list if no tokens deposited.
    #[view]
    fn get_deposits(&self, address: Address) -> ApiMap<TokenId, WasmAmount> {
        self.as_dex()
            .contract()
            .as_ref()
            .accounts
            .inspect(&address, |dex::Account::V0(ref account)| {
                account
                    .token_balances
                    .iter()
                    .map(|(token_id, amount)| (token_id.clone(), (*amount).into()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns balance of the deposit for given user outside of any pools.
    #[view]
    fn get_deposit(&self, account: AccountId, token_id: TokenId) -> WasmAmount {
        self.as_dex()
            .contract()
            .as_ref()
            .accounts
            .inspect(&account, |dex::Account::V0(ref account)| {
                account.token_balances.inspect(&token_id, |v| *v)
            })
            .flatten()
            .unwrap_or_else(|| 0.into())
            .into()
    }

    /// Get ordered allowed tokens list.
    #[view]
    fn get_verified_tokens(&self) -> ApiVec<TokenId> {
        self.as_dex()
            .contract()
            .as_ref()
            .verified_tokens
            .iter()
            .map(|t| t.clone())
            .collect()
    }

    /// Get specific user tokens.
    #[view]
    fn get_user_tokens(&self, account_id: AccountId) -> ApiVec<TokenId> {
        self.as_dex()
            .contract()
            .as_ref()
            .accounts
            .inspect(&account_id, |dex::Account::V0(ref account)| {
                account
                    .token_balances
                    .iter()
                    .map(|(token_id, _)| token_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[view]
    fn get_pool_info(&self, tokens: (TokenId, TokenId)) -> Option<PoolInfo> {
        let result = self
            .result_unwrap(self.as_dex().get_pool_info(tokens))
            .map(TryInto::try_into)
            .transpose();
        self.result_unwrap(result)
    }

    #[view]
    fn get_liqudity_fee_level_distribution(
        &self,
        tokens: (TokenId, TokenId),
    ) -> Option<RawFeeLevelsArray<Fraction>> {
        self.result_unwrap(self.as_dex().get_liqudity_fee_level_distribution(tokens))
            .map(|array| array.map(|value| self.result_unwrap(value.try_into())))
    }

    #[allow(unused_variables)] // Keep args names to leave API unchanged
    #[view]
    fn token_register_of(&self, account_id: AccountId, token_id: TokenId) -> bool {
        true
    }

    #[view]
    fn get_owner(&self) -> AccountId {
        self.contract_state().get().as_ref().owner_id.clone()
    }

    #[view]
    fn get_version(&self) -> VersionInfo {
        self.as_dex().get_version()
    }

    #[endpoint(extendVerifiedTokens)]
    fn extend_verified_tokens(&self, token_ids: ApiVec<TokenId>) {
        self.result_unwrap(self.as_dex_mut().add_verified_tokens(token_ids.0));
    }

    #[endpoint(extend_verified_tokens)]
    fn extend_verified_tokens_snake_case(&self, token_ids: ApiVec<TokenId>) {
        self.extend_verified_tokens(token_ids);
    }

    #[endpoint(removeVerifiedTokens)]
    fn remove_verified_tokens(&self, token_ids: ApiVec<TokenId>) {
        self.result_unwrap(self.as_dex_mut().remove_verified_tokens(token_ids.0));
    }

    #[endpoint(remove_verified_tokens)]
    fn remove_verified_tokens_snake_case(&self, token_ids: ApiVec<TokenId>) {
        self.remove_verified_tokens(token_ids);
    }

    #[endpoint(setProtocolFeeFraction)]
    fn set_protocol_fee_fraction(&self, protocol_fee_fraction: BasisPoints) {
        self.result_unwrap(
            self.as_dex_mut()
                .set_protocol_fee_fraction(protocol_fee_fraction),
        );
    }

    #[endpoint(set_protocol_fee_fraction)]
    fn set_protocol_fee_fraction_snake_case(&self, protocol_fee_fraction: BasisPoints) {
        self.set_protocol_fee_fraction(protocol_fee_fraction);
    }

    #[endpoint(executeActions)]
    fn execute_actions(&self, actions: ApiVec<Action>) {
        let result = self
            .as_dex_mut()
            .execute_actions(&mut |_, _, _| Ok(()), actions.0)
            .and_then(|(outcomes, _)| SendBatch::try_handle_outcomes(self, outcomes));

        self.result_unwrap(result);
    }

    #[endpoint(execute_actions)]
    fn execute_actions_snake_case(&self, actions: ApiVec<Action>) {
        self.execute_actions(actions);
    }

    #[endpoint(swapExactIn)]
    fn swap_exact_in(
        &self,
        tokens: ApiVec<TokenId>,
        amount_in: WasmAmount,
        min_amount_out: WasmAmount,
    ) -> (WasmAmount, WasmAmount) {
        let res = self.result_unwrap(self.as_dex_mut().swap_exact_in(
            &tokens.0,
            amount_in.into(),
            min_amount_out.into(),
        ));

        (res.0.into(), res.1.into())
    }

    #[endpoint(swap_exact_in)]
    fn swap_exact_in_snake_case(
        &self,
        tokens: ApiVec<TokenId>,
        amount_in: WasmAmount,
        min_amount_out: WasmAmount,
    ) -> (WasmAmount, WasmAmount) {
        self.swap_exact_in(tokens, amount_in, min_amount_out)
    }

    #[endpoint(swapExactOut)]
    fn swap_exact_out(
        &self,
        tokens: ApiVec<TokenId>,
        amount_out: WasmAmount,
        max_amount_in: WasmAmount,
    ) -> (WasmAmount, WasmAmount) {
        let res = self.result_unwrap(self.as_dex_mut().swap_exact_out(
            &tokens.0,
            amount_out.into(),
            max_amount_in.into(),
        ));

        (res.0.into(), res.1.into())
    }

    #[endpoint(swap_exact_out)]
    fn swap_exact_out_snake_case(
        &self,
        tokens: ApiVec<TokenId>,
        amount_out: WasmAmount,
        max_amount_in: WasmAmount,
    ) -> (WasmAmount, WasmAmount) {
        self.swap_exact_out(tokens, amount_out, max_amount_in)
    }

    #[endpoint(swapToPrice)]
    fn swap_to_price(
        &self,
        tokens: ApiVec<TokenId>,
        amount_in: WasmAmount,
        effective_price_limit: Fraction,
    ) -> (WasmAmount, WasmAmount) {
        let res = self.result_unwrap(self.as_dex_mut().swap_to_price(
            &tokens.0,
            amount_in.into(),
            effective_price_limit.into(),
        ));

        (res.0.into(), res.1.into())
    }

    #[endpoint(swap_to_price)]
    fn swap_to_price_snake_case(
        &self,
        tokens: ApiVec<TokenId>,
        amount_in: WasmAmount,
        effective_price_limit: Fraction,
    ) -> (WasmAmount, WasmAmount) {
        self.swap_to_price(tokens, amount_in, effective_price_limit)
    }

    #[endpoint(openPosition)]
    fn open_position(
        &self,
        token_a: &TokenId,
        token_b: &TokenId,
        fee_rate: dex::BasisPoints,
        position: PositionInit,
    ) -> (PositionId, WasmAmount, WasmAmount, Fraction) {
        let (position_id, amount_a, amount_b, net_liquidity) = self.result_unwrap(
            self.as_dex_mut()
                .open_position(token_a, token_b, fee_rate, position),
        );

        let fee_level: FeeLevel = self.result_unwrap(
            self.as_dex()
                .fee_rates_ticks()
                .iter()
                .find_position(|rate| **rate == fee_rate)
                .unwrap_or_else(|| sc_panic!("Failed to find fee rate"))
                .0
                .try_into(),
        );

        let liquidity = net_liquidity
            * self.result_unwrap(Liquidity::try_from(one_over_sqrt_one_minus_fee_rate(
                fee_level,
            )));

        let liquidity = self.result_unwrap(Float::from(liquidity).try_into());

        (position_id, amount_a.into(), amount_b.into(), liquidity)
    }

    #[endpoint(open_position)]
    fn open_position_snake_case(
        &self,
        token_a: &TokenId,
        token_b: &TokenId,
        fee_rate: dex::BasisPoints,
        position: PositionInit,
    ) -> (PositionId, WasmAmount, WasmAmount, Fraction) {
        self.open_position(token_a, token_b, fee_rate, position)
    }

    #[endpoint(closePosition)]
    fn close_position(&self, position_id: PositionId) {
        self.result_unwrap(self.as_dex_mut().close_position(position_id));
    }

    #[endpoint(close_position)]
    fn close_position_snake_case(&self, position_id: PositionId) {
        self.close_position(position_id);
    }

    #[endpoint(withdrawFee)]
    fn withdraw_fee(&self, position_id: PositionId) -> (WasmAmount, WasmAmount) {
        self.result_unwrap(self.as_dex_mut().withdraw_fee(position_id))
            .map_into()
    }

    #[endpoint(withdraw_fee)]
    fn withdraw_fee_snake_case(&self, position_id: PositionId) -> (WasmAmount, WasmAmount) {
        self.withdraw_fee(position_id)
    }

    #[view]
    fn get_position_info(&self, position_id: PositionId) -> PositionInfo {
        let position_info = self.result_unwrap(self.as_dex().get_position_info(position_id));

        self.result_unwrap(position_info.try_into())
    }

    #[view]
    fn get_positions_info(&self, positions_ids: Vec<PositionId>) -> Vec<Option<PositionInfo>> {
        self.as_dex()
            .get_positions_info(&positions_ids)
            .into_iter()
            .map(|info| info.map(|info| self.result_unwrap(info.try_into())))
            .collect()
    }

    /// Deposit tokens. Receives EGLD or single ESDT payment
    #[endpoint]
    #[payable("*")]
    fn deposit(&self, actions: ApiVec<Action>) {
        // Check if we have esdt payments
        let mut payments: Vec<dex::DepositPayment> = self
            .call_value()
            .all_esdt_transfers()
            .into_iter()
            .map(|egld_payment| dex::DepositPayment {
                token_id: into_token_id(&egld_payment.token_identifier),
                amount: egld_payment.amount.into(),
            })
            .collect();

        // Fetch EGLD payment if any
        let egld_value = self.call_value().egld_value();
        let mut self_as_dex = self.as_dex_mut();

        if *egld_value > 0 {
            let (wegld_addr, wegld_id) = self_as_dex
                .wegld()
                .cloned()
                .unwrap_or_else(|| sc_panic!(WEGLD_DOUBLE_INIT_ERROR));

            let _: IgnoreValue = self
                .wegld_swap_proxy(wegld_addr.to_byte_array().into())
                .wrap_egld()
                .with_egld_transfer(egld_value.clone_value())
                .execute_on_dest_context();

            payments.push(dex::DepositPayment {
                token_id: wegld_id,
                amount: egld_value.clone_value().into(),
            });
        }

        let actions = actions.0;
        let caller_id = self_as_dex.get_caller_id();

        let result = if actions.is_empty() {
            self_as_dex.deposit_execute_actions(
                &caller_id,
                &payments,
                &mut |_, _, _| Ok(()),
                vec![Action::Deposit],
            )
        } else {
            self_as_dex.deposit_execute_actions(
                &caller_id,
                &payments,
                &mut |_, _, _| Ok(()),
                actions,
            )
        }
        .and_then(|outcomes| SendBatch::try_handle_outcomes(self, outcomes));

        self.result_unwrap(result);
    }

    /// Withdraw fungible tokens from specified account to their source contract
    /// Operates with ESDT tokens
    /// Client should register a callback to where reciveve the tokens to
    #[endpoint]
    fn withdraw(&self, token_id: EgldOrTokenId, amount: WasmAmount, callback: Option<MethodCall>) {
        let mut dex = self.as_dex_mut();

        let result = dex
            .withdraw(
                &dex.get_caller_id(),
                &token_id,
                amount.into(),
                false,
                callback,
            )
            .and_then(|outcome| SendBatch::try_handle_outcomes(self, outcome));

        self.result_unwrap(result);
    }

    #[callback]
    fn withdraw_callback(
        &self,
        head: Withdrawal,
        tail: ApiVec<Withdrawal>,
        #[call_result] result: ManagedAsyncCallResult<()>,
    ) {
        let Withdrawal {
            account_id,
            token_id,
            amount,
            // FIXME: callback data should be empty here, add some debug check?
            callback: _,
        } = head;

        let account_id = account_id.into();

        let mut dex = self.as_dex_mut();
        let dex::StateMembersMut {
            contract,
            logger,
            item_factory: _,
        } = dex.members_mut();
        let contract = contract.latest();

        let result =
            contract
                .accounts
                .try_update(&account_id, |dex::Account::V0(ref mut account)| {
                    // Untrack regardless of result, transfer is finished here
                    account.withdraw_tracker.untrack(&token_id, &amount);
                    // If transfer succeeded, we do nothing except remove track record
                    // If transfer failed, we return tokens back to account and write additional deposit event
                    if !result.is_ok() {
                        let balance = account.token_balances.update_or_insert(
                            &token_id,
                            || Ok(Amount::zero()),
                            |balance, _| {
                                *balance += amount;
                                Ok(*balance)
                            },
                        )?;
                        logger.log_deposit_event(&account_id, &token_id, &amount, &balance);
                    }

                    Ok(())
                });
        // Well, we should never fail here, but just in case...
        self.result_unwrap(result);
        // Handle rest  of transfers, if there are any
        SendBatch::handle_withdrawals(self, tail.0);
    }

    #[endpoint(addGuardAccounts)]
    fn add_guard_accounts(&self, accounts: ApiVec<AccountId>) {
        self.result_unwrap(self.as_dex_mut().add_guard_accounts(accounts.0));
    }

    #[endpoint(add_guard_accounts)]
    fn add_guard_accounts_snake_case(&self, accounts: ApiVec<AccountId>) {
        self.add_guard_accounts(accounts);
    }

    #[endpoint(removeGuardAccounts)]
    fn remove_guard_accounts(&self, accounts: ApiVec<AccountId>) {
        self.result_unwrap(self.as_dex_mut().remove_guard_accounts(accounts.0));
    }

    #[endpoint(remove_guard_accounts)]
    fn remove_guard_accounts_snake_case(&self, accounts: ApiVec<AccountId>) {
        self.remove_guard_accounts(accounts);
    }

    #[endpoint(suspendPayableApi)]
    fn suspend_payable_api(&self) {
        self.result_unwrap(self.as_dex_mut().suspend_payable_api());
    }

    #[endpoint(suspend_payable_api)]
    fn suspend_payable_api_snake_case(&self) {
        self.suspend_payable_api();
    }

    #[endpoint(resumePayableApi)]
    fn resume_payable_api(&self) {
        self.result_unwrap(self.as_dex_mut().resume_payable_api());
    }

    #[endpoint(resume_payable_api)]
    fn resume_payable_api_snake_case(&self) {
        self.resume_payable_api();
    }

    #[label("dx25-contract-view")]
    #[view]
    fn estimate_swap_exact(
        &self,
        is_exact_in: bool,
        token_in: TokenId,
        token_out: TokenId,
        amount: WasmAmount,
        slippage_tolerance_bp: BasisPoints,
    ) -> EstimateSwapExactResult {
        self.result_unwrap(
            self.result_unwrap(self.as_dex().estimate_swap_exact(
                is_exact_in,
                token_in,
                token_out,
                amount.into(),
                slippage_tolerance_bp,
            ))
            .try_into(),
        )
    }

    #[label("dx25-contract-view")]
    #[view]
    fn estimate_liquidity_add(
        &self,
        tokens: (TokenId, TokenId),
        fee_rate: BasisPoints,
        ticks_range: (Option<i32>, Option<i32>),
        amount_a: Option<WasmAmount>,
        amount_b: Option<WasmAmount>,
        user_price: Option<Fraction>,
        slippage_tolerance_bp: BasisPoints,
    ) -> EstimateAddLiquidityResult {
        self.result_unwrap(
            self.result_unwrap(self.as_dex().estimate_liq_add(
                tokens,
                fee_rate,
                ticks_range,
                amount_a.map(Into::into),
                amount_b.map(Into::into),
                user_price.map(Into::into),
                slippage_tolerance_bp,
            ))
            .try_into(),
        )
    }

    fn as_dex(&self) -> dex::Dex<Types<Self::Api>, StateWrapper<Self>, StateWrapper<Self>> {
        dex::Dex::new(StateWrapper::new(self))
    }

    fn as_dex_mut(&self) -> DexWrapper<'_, Self> {
        DexWrapper::new(dex::Dex::new(StateMutWrapper::new(self)))
    }

    fn item_factory(&self) -> ItemFactory<Self::Api> {
        ItemFactory::new(self.unique_id())
    }

    fn fail(&self, error: impl std::error::Error) -> ! {
        sc_panic!(error.to_string().as_bytes())
    }

    // Raising sc_panic! is a recommended way of emitting an error, so we use it for all DEX results
    /// Unwraps a Result or signals sc error
    fn result_unwrap<T>(&self, result: Result<T, impl std::error::Error>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => self.fail(err),
        }
    }
}
