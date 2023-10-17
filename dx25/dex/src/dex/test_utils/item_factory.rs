use super::collections::{Map, OrderedMap, TypedStorage};
use super::{dex, Types};

/// Simple item factory, also contains storage
///
/// Uses mock `Map` for all containers
#[derive(Default)]
pub struct ItemFactory(TypedStorage);

impl ItemFactory {
    /// Creates new item factory with standalone empty storage
    pub fn new() -> Self {
        Self::with_storage(TypedStorage::new())
    }
    /// Creates item factory initialized with specified shared storage instance
    pub(super) fn with_storage(storage: TypedStorage) -> Self {
        Self(storage)
    }

    fn new_map<K, V>(&mut self) -> Map<K, V> {
        self.0.new_map()
    }

    fn new_ord_map<K, V>(&mut self) -> OrderedMap<K, V> {
        self.0.new_ord_map()
    }
}

impl dex::ItemFactory<Types> for ItemFactory {
    fn new_accounts_map(&mut self) -> <Types as dex::Types>::AccountsMap {
        self.new_map()
    }

    fn new_account_token_balances_map(&mut self) -> <Types as dex::Types>::AccountTokenBalancesMap {
        self.new_map()
    }

    fn new_account_withdraw_tracker(&mut self) -> <Types as dex::Types>::AccountWithdrawTracker {
        dex::withdraw_trackers::NoopTracker
    }

    fn new_pools_map(&mut self) -> <Types as dex::Types>::PoolsMap {
        self.new_map()
    }

    fn new_pool_positions_map(&mut self) -> <Types as dex::Types>::PoolPositionsMap {
        self.new_map()
    }

    fn new_tick_states_map(&mut self) -> <Types as dex::Types>::TickStatesMap {
        self.new_ord_map()
    }

    fn new_account_positions_set(&mut self) -> <Types as dex::Types>::AccountPositionsSet {
        self.new_map()
    }

    fn new_verified_tokens_set(&mut self) -> <Types as dex::Types>::VerifiedTokensSet {
        self.new_map()
    }

    fn new_position_to_pool_map(&mut self) -> <Types as dex::Types>::PositionToPoolMap {
        self.new_map()
    }

    fn new_guards(&mut self) -> <Types as dex::Types>::AccountIdSet {
        self.new_map()
    }
}
