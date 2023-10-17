use crate::dex::{self, Map, OrderedMap, Types};

/// Simple item factory, also contains storage
///
/// Uses mock `Map` for all containers
#[derive(Default)]
pub struct OverlayItemFactory;

impl OverlayItemFactory {
    /// Creates new item factory with standalone empty storage
    pub fn new() -> Self {
        Self
    }
}

impl<T: Types> dex::ItemFactory<T> for OverlayItemFactory {
    fn new_accounts_map(&mut self) -> T::AccountsMap {
        unimplemented!()
    }

    fn new_account_token_balances_map(&mut self) -> T::AccountTokenBalancesMap {
        unimplemented!()
    }

    fn new_account_withdraw_tracker(&mut self) -> T::AccountWithdrawTracker {
        // dex::withdraw_trackers::NoopTracker
        unimplemented!()
    }

    fn new_pools_map(&mut self) -> T::PoolsMap {
        unimplemented!()
    }

    fn new_pool_positions_map(&mut self) -> T::PoolPositionsMap {
        unimplemented!()
    }

    fn new_tick_states_map(&mut self) -> T::TickStatesMap {
        unimplemented!()
    }

    fn new_account_positions_set(&mut self) -> T::AccountPositionsSet {
        unimplemented!()
    }

    fn new_verified_tokens_set(&mut self) -> T::VerifiedTokensSet {
        unimplemented!()
    }

    fn new_position_to_pool_map(&mut self) -> T::PositionToPoolMap {
        unimplemented!()
    }

    fn new_guards(&mut self) -> T::AccountIdSet {
        unimplemented!()
    }
}
