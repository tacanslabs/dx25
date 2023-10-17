use multiversx_sc::{
    api::StorageMapperApi, storage::mappers::SingleValueMapper, types::ManagedBuffer,
};

use crate::{
    chain::{StorageMap, StorageSet, Types},
    dex, StorageOrderedMap,
};

/// Creates nested storage maps
pub struct ItemFactory<S: StorageMapperApi> {
    unique_id_counter: SingleValueMapper<S, u64>,
}

impl<S: StorageMapperApi> ItemFactory<S> {
    pub fn new(unique_id_counter: SingleValueMapper<S, u64>) -> Self {
        Self { unique_id_counter }
    }

    fn next_unique_id(&mut self) -> ManagedBuffer<S> {
        let next = self.unique_id_counter.update(|value| {
            let next = *value;
            *value += 1u64;
            next
        });

        (&next.to_be_bytes()).into()
    }
}

impl<S: StorageMapperApi> dex::ItemFactory<Types<S>> for ItemFactory<S> {
    fn new_accounts_map(&mut self) -> <Types<S> as dex::Types>::AccountsMap {
        StorageMap::new(self.next_unique_id())
    }

    fn new_tick_states_map(&mut self) -> <Types<S> as dex::Types>::TickStatesMap {
        StorageOrderedMap::new(self.next_unique_id().to_boxed_bytes().as_slice())
    }

    fn new_account_token_balances_map(
        &mut self,
    ) -> <Types<S> as dex::Types>::AccountTokenBalancesMap {
        StorageMap::new(self.next_unique_id())
    }

    fn new_account_withdraw_tracker(&mut self) -> <Types<S> as dex::Types>::AccountWithdrawTracker {
        dex::withdraw_trackers::FullTracker::default()
    }

    fn new_pools_map(&mut self) -> <Types<S> as dex::Types>::PoolsMap {
        StorageMap::new(self.next_unique_id())
    }

    fn new_pool_positions_map(&mut self) -> <Types<S> as dex::Types>::PoolPositionsMap {
        StorageMap::new(self.next_unique_id())
    }

    fn new_account_positions_set(&mut self) -> <Types<S> as dex::Types>::AccountPositionsSet {
        StorageSet::new(self.next_unique_id())
    }

    fn new_verified_tokens_set(&mut self) -> <Types<S> as dex::Types>::VerifiedTokensSet {
        StorageSet::new(self.next_unique_id())
    }

    fn new_position_to_pool_map(&mut self) -> <Types<S> as dex::Types>::PositionToPoolMap {
        StorageMap::new(self.next_unique_id())
    }

    fn new_guards(&mut self) -> <Types<S> as dex::Types>::AccountIdSet {
        StorageSet::new(self.next_unique_id())
    }
}
