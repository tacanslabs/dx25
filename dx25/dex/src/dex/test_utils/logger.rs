use super::dex;
use crate::chain::{AccountId, Amount, TokenId};
use dex::{latest, BasisPoints, FeeLevel, PositionId, Tick};

/// Enum with all event types written into blockchain.
/// Intended for matching in tests, so stores all values directly.
#[allow(clippy::large_enum_variant)] // Size doesn't matter that much for testing
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Message(String),
    Deposit {
        user: AccountId,
        token: TokenId,
        amount: Amount,
        balance: Amount,
    },
    Withdraw {
        user: AccountId,
        token: TokenId,
        amount: Amount,
        balance: Amount,
    },
    OpenPosition {
        user: AccountId,
        pool: (TokenId, TokenId),
        amounts: (Amount, Amount),
        fee_rate: BasisPoints,
        position_id: PositionId,
        ticks_range: (i32, i32),
    },
    HarvestFee {
        position_id: PositionId,
        amounts: (Amount, Amount),
    },
    ClosePosition {
        position_id: PositionId,
        amounts: (Amount, Amount),
    },
    Swap {
        user: AccountId,
        tokens: (TokenId, TokenId),
        amounts: (Amount, Amount),
        fees: Vec<(TokenId, BasisPoints)>,
    },
    UpdatePoolState {
        reason: dex::PoolUpdateReason,
        pool: (TokenId, TokenId),
        amounts_a: latest::RawFeeLevelsArray<Amount>,
        amounts_b: latest::RawFeeLevelsArray<Amount>,
        sqrt_prices: latest::RawFeeLevelsArray<dex::Float>,
        liquidities: latest::RawFeeLevelsArray<dex::Float>,
    },
    AddVerifiedTokens {
        tokens: Vec<TokenId>,
    },
    RemoveVerifiedTokens {
        tokens: Vec<TokenId>,
    },
    AddGuardAccounts {
        accounts: Vec<AccountId>,
    },
    RemoveGuardAccounts {
        accounts: Vec<AccountId>,
    },
    SuspendPayableAPI {
        account: AccountId,
    },
    ResumePayableAPI {
        account: AccountId,
    },
    TickUpdate {
        pool: (TokenId, TokenId),
        fee_level: u8,
        tick: i32,
        liquidity_change: f64,
    },
}
/// Mock event logger, with persistent and mutable parts
pub struct Logger {
    persistent: Vec<Event>,
    mutable: Vec<Event>,
    prev_log_index: usize,
}

impl Logger {
    pub fn new() -> Self {
        Self {
            persistent: Vec::new(),
            mutable: Vec::new(),
            prev_log_index: 0,
        }
    }
    /// Move all entries from mutable part into persistent one,
    /// used when some operation succeeds
    pub fn commit(&mut self) {
        self.prev_log_index = self.persistent.len();
        self.persistent.append(&mut self.mutable);
    }
    /// Drops all entries from mutable part, effectively discarding them
    pub fn reject(&mut self) {
        self.mutable.clear();
    }
    /// Provides read-only access to persistent logs
    pub fn logs(&self) -> &[Event] {
        &self.persistent
    }
    /// Provides read-only access to log messages recorded via last commit
    pub fn latest_logs(&self) -> &[Event] {
        &self.persistent[self.prev_log_index..]
    }
}

impl dex::Logger for Logger {
    fn log(&mut self, args: std::fmt::Arguments<'_>) {
        self.mutable.push(Event::Message(std::fmt::format(args)));
    }

    fn log_deposit_event(
        &mut self,
        user: &AccountId,
        token: &TokenId,
        amount: &Amount,
        balance: &Amount,
    ) {
        self.mutable.push(Event::Deposit {
            user: user.clone(),
            token: token.clone(),
            amount: *amount,
            balance: *balance,
        });
    }

    fn log_withdraw_event(
        &mut self,
        user: &AccountId,
        token: &TokenId,
        amount: &Amount,
        balance: &Amount,
    ) {
        self.mutable.push(Event::Withdraw {
            user: user.clone(),
            token: token.clone(),
            amount: *amount,
            balance: *balance,
        });
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
        self.mutable.push(Event::OpenPosition {
            user: user.clone(),
            pool: (pool.0.clone(), pool.1.clone()),
            amounts: (*amounts.0, *amounts.1),
            fee_rate,
            position_id,
            ticks_range: (ticks_range.0.index(), ticks_range.1.index()),
        });
    }

    fn log_harvest_fee_event(&mut self, position_id: PositionId, fee_amounts: (Amount, Amount)) {
        self.mutable.push(Event::HarvestFee {
            position_id,
            amounts: fee_amounts,
        });
    }

    fn log_close_position_event(&mut self, position_id: PositionId, amounts: (Amount, Amount)) {
        self.mutable.push(Event::ClosePosition {
            position_id,
            amounts,
        });
    }

    fn log_swap_event(
        &mut self,
        user: &AccountId,
        tokens: (&TokenId, &TokenId),
        amounts: (&Amount, &Amount),
        fees: &[(&TokenId, &BasisPoints)],
    ) {
        self.mutable.push(Event::Swap {
            user: user.clone(),
            tokens: (tokens.0.clone(), tokens.1.clone()),
            amounts: (*amounts.0, *amounts.1),
            fees: fees.iter().map(|(t, f)| ((*t).clone(), **f)).collect(),
        });
    }

    fn log_update_pool_state_event(
        &mut self,
        reason: dex::PoolUpdateReason,
        pool: (&TokenId, &TokenId),
        amounts_a: &latest::RawFeeLevelsArray<Amount>,
        amounts_b: &latest::RawFeeLevelsArray<Amount>,
        sqrt_prices: &latest::RawFeeLevelsArray<dex::Float>,
        liquidities: &latest::RawFeeLevelsArray<dex::Float>,
    ) {
        self.mutable.push(Event::UpdatePoolState {
            reason,
            pool: (pool.0.clone(), pool.1.clone()),
            amounts_a: *amounts_a,
            amounts_b: *amounts_b,
            sqrt_prices: *sqrt_prices,
            liquidities: *liquidities,
        });
    }

    fn log_add_verified_tokens_event(&mut self, tokens: &[TokenId]) {
        self.mutable.push(Event::AddVerifiedTokens {
            tokens: tokens.to_vec(),
        });
    }

    fn log_remove_verified_tokens_event(&mut self, tokens: &[TokenId]) {
        self.mutable.push(Event::RemoveVerifiedTokens {
            tokens: tokens.to_vec(),
        });
    }

    fn log_add_guard_accounts_event(&mut self, accounts: &[AccountId]) {
        self.mutable.push(Event::AddGuardAccounts {
            accounts: accounts.to_vec(),
        });
    }

    fn log_remove_guard_accounts_event(&mut self, accounts: &[AccountId]) {
        self.mutable.push(Event::RemoveGuardAccounts {
            accounts: accounts.to_vec(),
        });
    }

    fn log_suspend_payable_api_event(&mut self, account: &AccountId) {
        self.mutable.push(Event::SuspendPayableAPI {
            account: account.clone(),
        });
    }

    fn log_resume_payable_api_event(&mut self, account: &AccountId) {
        self.mutable.push(Event::ResumePayableAPI {
            account: account.clone(),
        });
    }

    fn log_tick_update_event(
        &mut self,
        pool: (&TokenId, &TokenId),
        fee_level: FeeLevel,
        tick: Tick,
        liquidity_change: dex::Float,
    ) {
        self.mutable.push(Event::TickUpdate {
            pool: (pool.0.clone(), pool.1.clone()),
            fee_level,
            tick: tick.index(),
            liquidity_change: f64::from(liquidity_change),
        });
    }
}
