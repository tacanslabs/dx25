use super::super::dex_types::Types;
use crate::chain::VmApi;
use crate::WEGLD_NOT_INIT_ERROR;
use crate::{
    api_types::{Action, MethodCall, Withdrawal},
    dex,
    dex_state::StateMutWrapper,
    AccountId, Amount, Dx25Contract, TokenId,
};
use dex::{Result, StateMut as _};
use multiversx_sc::{contract_base::ErrorHelper, types::EgldOrEsdtTokenIdentifier};

pub struct DexWrapper<'a, C: Dx25Contract> {
    dex: dex::Dex<Types<C::Api>, StateMutWrapper<'a, C>, StateMutWrapper<'a, C>>,
}

impl<'a, C: Dx25Contract> std::ops::Deref for DexWrapper<'a, C> {
    type Target = dex::Dex<Types<C::Api>, StateMutWrapper<'a, C>, StateMutWrapper<'a, C>>;

    fn deref(&self) -> &Self::Target {
        &self.dex
    }
}

impl<'a, C: Dx25Contract> std::ops::DerefMut for DexWrapper<'a, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.dex
    }
}

fn map_token_id<C: Dx25Contract>(
    token_id: EgldOrEsdtTokenIdentifier<VmApi>,
    wegld_id: Option<&TokenId>,
) -> (TokenId, bool) {
    if let Some(token) = token_id.into_esdt_option() {
        (TokenId::new(token), false)
    } else {
        let wegld_id = wegld_id
            .unwrap_or_else(|| {
                ErrorHelper::<C::Api>::signal_error_with_message(WEGLD_NOT_INIT_ERROR)
            })
            .clone();
        (wegld_id, true)
    }
}

fn map_action<C: Dx25Contract>(
    action: Action,
    wegld_id: Option<&TokenId>,
) -> dex::Action<(bool, Option<MethodCall>)> {
    match action {
        Action::Withdraw(token_id, amount, method_call) => {
            let (token_id, extra) = map_token_id::<C>(token_id, wegld_id);
            dex::Action::Withdraw(token_id, amount, (extra, method_call))
        }
        Action::RegisterAccount => dex::Action::RegisterAccount,
        Action::RegisterTokens(tokens) => dex::Action::RegisterTokens(tokens),
        Action::SwapExactIn(swap) => dex::Action::SwapExactIn(swap),
        Action::SwapExactOut(swap) => dex::Action::SwapExactOut(swap),
        Action::SwapToPrice(swap) => dex::Action::SwapToPrice(swap),
        Action::Deposit => dex::Action::Deposit,
        Action::OpenPosition {
            tokens,
            fee_rate,
            position,
        } => dex::Action::OpenPosition {
            tokens,
            fee_rate,
            position,
        },
        Action::ClosePosition(pos) => dex::Action::ClosePosition(pos),
        Action::WithdrawFee(pos) => dex::Action::WithdrawFee(pos),
    }
}

impl<'a, C: Dx25Contract> DexWrapper<'a, C> {
    pub fn new(
        dex: dex::Dex<Types<C::Api>, StateMutWrapper<'a, C>, StateMutWrapper<'a, C>>,
    ) -> Self {
        Self { dex }
    }

    #[allow(clippy::type_complexity)]
    fn map_actions(
        &mut self,
        actions: Vec<Action>,
    ) -> Vec<dex::Action<(bool, Option<MethodCall>)>> {
        let wegld_id = self.wegld().map(|(_, id)| id);
        actions
            .into_iter()
            .map(|action| map_action::<C>(action, wegld_id))
            .collect()
    }

    pub fn deposit_execute_actions(
        &mut self,
        account_id: &AccountId,
        deposit_data: &[dex::DepositPayment],
        register_account_cb: dex::AccountCallbackType<'_, Types<C::Api>>,
        actions: Vec<Action>,
    ) -> Result<Vec<Result<Option<Withdrawal>>>> {
        let actions = self.map_actions(actions);
        self.dex
            .deposit_execute_actions(account_id, deposit_data, register_account_cb, actions)
    }

    pub fn withdraw(
        &mut self,
        account_id: &AccountId,
        token_id: &EgldOrEsdtTokenIdentifier<VmApi>,
        amount: Amount,
        unregister: bool,
        method_call: Option<MethodCall>,
    ) -> Result<Option<Result<Option<Withdrawal>>>> {
        let (token_id, unwrap) =
            map_token_id::<C>(token_id.clone(), self.wegld().map(|(_, id)| id));
        self.dex.withdraw(
            account_id,
            &token_id,
            amount,
            unregister,
            (unwrap, method_call),
        )
    }

    pub fn owner_withdraw(
        &mut self,
        token_id: &EgldOrEsdtTokenIdentifier<VmApi>,
        amount: Amount,
        method_call: Option<MethodCall>,
    ) -> Result<Result<Option<Withdrawal>>> {
        let (token_id, extra) = map_token_id::<C>(token_id.clone(), self.wegld().map(|(_, id)| id));
        self.dex
            .owner_withdraw(&token_id, amount, (extra, method_call))
    }

    #[allow(clippy::type_complexity)]
    pub fn execute_actions(
        &mut self,
        register_account_cb: dex::AccountCallbackType<'_, Types<C::Api>>,
        actions: Vec<Action>,
    ) -> Result<(Vec<Result<Option<Withdrawal>>>, Option<Amount>)> {
        self.execute_actions_for(&self.dex.get_caller_id(), register_account_cb, actions)
    }

    #[allow(clippy::type_complexity)]
    pub fn execute_actions_for(
        &mut self,
        account_id: &AccountId,
        register_account_cb: dex::AccountCallbackType<'_, Types<C::Api>>,
        actions: Vec<Action>,
    ) -> Result<(Vec<Result<Option<Withdrawal>>>, Option<Amount>)> {
        let actions = self.map_actions(actions);
        self.dex
            .execute_actions_for(account_id, register_account_cb, actions)
    }
}
