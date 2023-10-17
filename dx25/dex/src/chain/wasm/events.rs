use std::fmt::Arguments;

use multiversx_sc::{log_util, types::ManagedBuffer};

use crate::{
    api_types::ApiVec,
    chain::{AccountId, Amount, TokenId},
    dex::{
        self, latest::RawFeeLevelsArray, BasisPoints, FeeLevel, Float, PoolUpdateReason,
        PositionId, Tick,
    },
    Dx25Contract,
};

pub struct Logger<'a, C: Dx25Contract> {
    contract: &'a C,
}

impl<'a, C: Dx25Contract> Logger<'a, C> {
    pub fn new(contract: &'a C) -> Self {
        Self { contract }
    }
}

#[allow(unused)]
impl<'a, C: Dx25Contract> dex::Logger for Logger<'a, C> {
    fn log(&mut self, args: Arguments<'_>) {
        let buffer = ManagedBuffer::from(std::fmt::format(args).as_bytes());

        self.contract.log(buffer);
    }

    fn log_deposit_event(
        &mut self,
        user: &AccountId,
        token_id: &TokenId,
        amount: &Amount,
        balance: &Amount,
    ) {
        let data = log_util::serialize_log_data(event::Deposit {
            user: user.clone(),
            token_id: token_id.native().clone(),
            amount: (*amount).into(),
            balance: (*balance).into(),
        });

        self.contract.log_deposit_event(data);
    }

    fn log_withdraw_event(
        &mut self,
        user: &AccountId,
        token_id: &TokenId,
        amount: &Amount,
        balance: &Amount,
    ) {
        let data = log_util::serialize_log_data(event::Withdraw {
            user: user.clone(),
            token_id: token_id.native().clone(),
            amount: (*amount).into(),
            balance: (*balance).into(),
        });

        self.contract.log_withdraw_event(data);
    }

    fn log_open_position_event(
        &mut self,
        user: &AccountId,
        pool: (&TokenId, &TokenId),
        amounts: (&Amount, &Amount),
        fee_rate: BasisPoints,
        position_id: PositionId,
        ticks_range: (Tick, Tick),
    ) {
        let data = log_util::serialize_log_data(event::OpenPosition {
            user: user.clone(),
            pool: (pool.0.native().clone(), pool.1.native().clone()),
            amounts: ((*amounts.0).into(), (*amounts.1).into()),
            fee_rate,
            position_id,
            ticks_range: (ticks_range.0.index(), ticks_range.1.index()),
        });

        self.contract.log_open_position_event(data);
    }

    fn log_harvest_fee_event(&mut self, position_id: PositionId, fee_amounts: (Amount, Amount)) {
        let data = log_util::serialize_log_data(event::HarvestFee {
            position_id,
            amounts: (fee_amounts.0.into(), fee_amounts.1.into()),
        });

        self.contract.log_harvest_fee_event(data);
    }

    fn log_close_position_event(&mut self, position_id: PositionId, amounts: (Amount, Amount)) {
        let data = log_util::serialize_log_data(event::ClosePosition {
            position_id,
            amounts: (amounts.0.into(), amounts.1.into()),
        });

        self.contract.log_close_position_event(data);
    }

    fn log_swap_event(
        &mut self,
        user: &AccountId,
        tokens: (&TokenId, &TokenId),
        amounts: (&Amount, &Amount),
        fees: &[(&TokenId, &BasisPoints)],
    ) {
        let data = log_util::serialize_log_data(event::Swap {
            user: user.clone(),
            tokens: (tokens.0.native().clone(), tokens.1.native().clone()),
            amounts: ((*amounts.0).into(), (*amounts.1).into()),
            fees: ApiVec(
                fees.iter()
                    .copied()
                    .map(|(id, point)| (id.native().clone(), *point))
                    .collect(),
            ),
        });

        self.contract.log_swap_event(data);
    }

    fn log_update_pool_state_event(
        &mut self,
        reason: PoolUpdateReason,
        pool: (&TokenId, &TokenId),
        amounts_a: &RawFeeLevelsArray<Amount>,
        amounts_b: &RawFeeLevelsArray<Amount>,
        sqrt_prices: &RawFeeLevelsArray<Float>,
        liquidities: &RawFeeLevelsArray<Float>,
    ) {
        let data = log_util::serialize_log_data(event::UpdatePoolState {
            pool: (pool.0.native().clone(), pool.1.native().clone()),
            reason,
            amounts_a: (*amounts_a).map(Into::into),
            amounts_b: (*amounts_b).map(Into::into),
            sqrt_prices: *sqrt_prices,
            liquidities: *liquidities,
        });

        self.contract.log_update_pool_state_event(data);
    }

    fn log_add_verified_tokens_event(&mut self, tokens: &[TokenId]) {
        let data = log_util::serialize_log_data(event::AddVerifiedTokens {
            tokens: ApiVec(tokens.iter().map(|token| token.native().clone()).collect()),
        });

        self.contract.log_add_verified_tokens_event(data);
    }

    fn log_remove_verified_tokens_event(&mut self, tokens: &[TokenId]) {
        let data = log_util::serialize_log_data(event::RemoveVerifiedTokens {
            tokens: ApiVec(tokens.iter().map(|token| token.native().clone()).collect()),
        });

        self.contract.log_remove_verified_tokens_event(data);
    }

    fn log_add_guard_accounts_event(&mut self, tokens: &[AccountId]) {
        let data = log_util::serialize_log_data(event::AddGuardAccounts {
            accounts: ApiVec(tokens.to_vec()),
        });

        self.contract.log_add_guard_accounts_event(data);
    }

    fn log_remove_guard_accounts_event(&mut self, tokens: &[AccountId]) {
        let data = log_util::serialize_log_data(event::RemoveGuardAccounts {
            accounts: ApiVec(tokens.to_vec()),
        });

        self.contract.log_remove_guard_accounts_event(data);
    }

    fn log_suspend_payable_api_event(&mut self, account: &AccountId) {
        let data = log_util::serialize_log_data(event::SuspendPayableAPI {
            account: account.clone(),
        });

        self.contract.log_suspend_payable_api_event(data);
    }

    fn log_resume_payable_api_event(&mut self, account: &AccountId) {
        let data = log_util::serialize_log_data(event::ResumePayableAPI {
            account: account.clone(),
        });

        self.contract.log_resume_payable_api_event(data);
    }

    fn log_tick_update_event(
        &mut self,
        pool: (&TokenId, &TokenId),
        fee_level: FeeLevel,
        tick: Tick,
        liquidity_change: Float,
    ) {
        let data = log_util::serialize_log_data(event::TickUpdate {
            pool: (pool.0.native().clone(), pool.1.native().clone()),
            fee_level,
            tick: tick.index(),
            liquidity_change,
        });

        self.contract.log_tick_update_event(data);
    }
}

pub mod event {
    use crate::{
        api_types::ApiVec,
        chain::{AccountId, VmApi},
        dex::{latest::RawFeeLevelsArray, BasisPoints, Float, PoolUpdateReason, PositionId},
        WasmAmount,
    };
    use multiversx_sc::types::TokenIdentifier;
    use multiversx_sc_codec::{
        self as codec,
        derive::{TopDecode, TopEncode},
    };

    type NativeTokenId = TokenIdentifier<VmApi>;

    #[derive(TopEncode, TopDecode)]
    pub struct Deposit {
        pub user: AccountId,
        pub token_id: NativeTokenId,
        pub amount: WasmAmount,
        pub balance: WasmAmount,
    }

    #[derive(TopEncode, TopDecode)]
    pub struct Withdraw {
        pub user: AccountId,
        pub token_id: NativeTokenId,
        pub amount: WasmAmount,
        pub balance: WasmAmount,
    }

    #[derive(TopEncode, TopDecode)]
    pub struct OpenPosition {
        pub user: AccountId,
        pub pool: (NativeTokenId, NativeTokenId),
        pub amounts: (WasmAmount, WasmAmount),
        pub fee_rate: BasisPoints,
        pub position_id: PositionId,
        pub ticks_range: (i32, i32),
    }

    #[derive(TopEncode, TopDecode)]
    pub struct HarvestFee {
        pub position_id: PositionId,
        pub amounts: (WasmAmount, WasmAmount),
    }

    #[derive(TopEncode, TopDecode)]
    pub struct ClosePosition {
        pub position_id: PositionId,
        pub amounts: (WasmAmount, WasmAmount),
    }

    #[derive(TopEncode, TopDecode)]
    pub struct Swap {
        pub user: AccountId,
        pub tokens: (NativeTokenId, NativeTokenId),
        pub amounts: (WasmAmount, WasmAmount),
        pub fees: ApiVec<(NativeTokenId, BasisPoints)>,
    }

    #[derive(TopEncode, TopDecode)]
    pub struct UpdatePoolState {
        pub pool: (NativeTokenId, NativeTokenId),
        pub reason: PoolUpdateReason,
        pub amounts_a: RawFeeLevelsArray<WasmAmount>,
        pub amounts_b: RawFeeLevelsArray<WasmAmount>,
        pub sqrt_prices: RawFeeLevelsArray<Float>,
        pub liquidities: RawFeeLevelsArray<Float>,
    }

    #[derive(TopEncode, TopDecode)]
    pub struct AddVerifiedTokens {
        pub tokens: ApiVec<NativeTokenId>,
    }

    #[derive(TopEncode, TopDecode)]
    pub struct RemoveVerifiedTokens {
        pub tokens: ApiVec<NativeTokenId>,
    }

    #[derive(TopEncode)]
    pub struct AddGuardAccounts {
        pub accounts: ApiVec<AccountId>,
    }

    #[derive(TopEncode)]
    pub struct RemoveGuardAccounts {
        pub accounts: ApiVec<AccountId>,
    }

    #[derive(TopEncode)]
    pub struct SuspendPayableAPI {
        pub account: AccountId,
    }

    #[derive(TopEncode)]
    pub struct ResumePayableAPI {
        pub account: AccountId,
    }

    #[derive(TopEncode)]
    pub struct TickUpdate {
        pub pool: (NativeTokenId, NativeTokenId),
        pub fee_level: u8,
        pub tick: i32,
        pub liquidity_change: Float,
    }
}
