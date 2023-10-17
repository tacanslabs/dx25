//! # Utilities for `dex` module unit testing
//!
//! * Mock contract state, independent of any concrete blockchain
//! * blockchain-specific functions for creating mock account and token ids
//!
//! This module contains implementation of simple key-value storage, where both keys and values
//! are byte vectors. Contract state is kept in that storage, and serialized/deserialized
//! as needed, just like with normal blockchain.
//!
//! Example of typical usage:
//! ```
//! let owner = new_account_id();
//! let mut state = State::new_default(owner.clone());
//! // register account
//! state.call_mut(owner.clone(), |dex| {
//!     dex.register_account()
//! }).unwrap();
//! // register some tokens
//! let token_0 = new_token_id();
//! let token_1 = new_token_id();
//!
//! state.call_mut(owner.clone(), |dex| {
//!     dex.register_tokens(&owner, [&token_0, &token_1])
//! }).unwrap();
//! // deposit
//! let amount_0 = Amount::from(100_000);
//! state.call_mut(owner.clone(), |dex| {
//!     dex.deposit(&owner, &token_0, amount_0)
//! });
//!
//! let amount_1 = Amount::from(1_000_000);
//! state.call_mut(owner.clone(), |dex| {
//!     dex.deposit(&owner, &token_1, amount_1)
//! });
//! // ... more code
//! ```
use super::collections::{Map, OrderedMap, TypedSnapshot, TypedStorage};
use super::dex;
use super::item_factory::ItemFactory;
use super::logger::{Event, Logger};
use super::traits::PersistentBound;
use crate::chain::{AccountId, Amount, TokenId};
use dex::{latest, Account, BasisPoints, Dex, Pool, PoolId, Position, PositionId, Result};

#[allow(unused)]
use num_traits::Zero; // Some `Amount`'s have `zero` as inherent method, some as trait impl

#[cfg(feature = "near")]
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};

#[cfg(feature = "concordium")]
use concordium_std::Serialize;

#[cfg(feature = "multiversx")]
use multiversx_sc_codec::{
    self as codec,
    derive::{NestedDecode, NestedEncode},
};
/// Contract state mock, independent of concrete blockchain
pub struct Sandbox {
    snapshot: TypedSnapshot,
    logger: Logger,
    caller_id: AccountId,
    initiator_id: AccountId,
}

#[allow(unused)]
impl Sandbox {
    /// Create new state mock, with usual set of parameters
    ///
    /// Owner is also set as default caller and initiator
    pub fn new(
        owner_id: AccountId,
        protocol_fee_fraction: BasisPoints,
        fee_rates: latest::RawFeeLevelsArray<BasisPoints>,
    ) -> Self {
        use dex::ItemFactory as _;
        let storage = TypedStorage::new();

        let mut item_factory = ItemFactory::with_storage(storage.clone());
        let contract = item_factory
            .new_contract(owner_id.clone(), protocol_fee_fraction, fee_rates)
            .unwrap();

        storage.write_root(contract);

        Self {
            snapshot: storage.freeze(),
            logger: Logger::new(),
            caller_id: owner_id.clone(),
            initiator_id: owner_id,
        }
    }

    pub fn caller_id(&self) -> &AccountId {
        &self.caller_id
    }

    pub fn set_caller_id(&mut self, caller_id: AccountId) -> AccountId {
        std::mem::replace(&mut self.caller_id, caller_id)
    }

    pub fn initiator_id(&self) -> &AccountId {
        &self.initiator_id
    }

    pub fn set_initiator_id(&mut self, initiator_id: AccountId) -> AccountId {
        std::mem::replace(&mut self.initiator_id, initiator_id)
    }

    pub fn set_initiator_caller_ids(&mut self, account_id: AccountId) -> (AccountId, AccountId) {
        let old_caller = self.set_caller_id(account_id.clone());
        let old_init = self.set_initiator_id(account_id);
        (old_init, old_caller)
    }
    /// Create new state mock, with protocol fee fraction and fee rates set to defaults
    pub fn new_default(owner_id: AccountId) -> Self {
        Self::new(owner_id, 1300, [1, 2, 4, 8, 16, 32, 64, 128])
    }
    /// Perform immutable call over state
    ///
    /// Deserializes contract's root record, creates temporary Dex instance over it
    /// and provides its immutable reference to callback
    pub fn call<F, R>(&self, call_fn: F) -> R
    where
        F: for<'a> FnOnce(&'a Dex<Types, StateInner, &'a StateInner>) -> R,
    {
        let storage = self.snapshot.thaw();
        let inner = StateInner {
            contract: storage.read_root(),
        };
        let dex = Dex::new(&inner);
        call_fn(&dex)
    }
    /// Perform mutable call over state
    ///
    /// Deserializes contract's root record, creates temporary mutable Dex state wrapper,
    /// then temporary Dex, then calls provided callback with mutable reference to it.
    /// If callback succeeds, changes are committed into contract storage, otherwise discarded.
    pub fn call_mut<F, R>(&mut self, call_mut_fn: F) -> Result<R>
    where
        F: for<'a> FnOnce(
            &'a mut Dex<Types, StateInnerMut<'a>, &'a mut StateInnerMut<'a>>,
        ) -> Result<R>,
    {
        let storage = self.snapshot.thaw();
        let mut contract = storage.read_root();
        let mut item_factory = ItemFactory::with_storage(storage.clone());

        let mut inner = StateInnerMut {
            caller_id: &self.caller_id,
            initiator_id: &self.initiator_id,
            contract: &mut contract,
            item_factory: &mut item_factory,
            logger: &mut self.logger,
        };
        let mut dex = Dex::new(&mut inner);
        // Commit if call succeeds, reject if it doesn't
        match call_mut_fn(&mut dex) {
            Ok(r) => {
                storage.write_root(contract);
                self.snapshot = storage.freeze();
                self.logger.commit();
                Ok(r)
            }
            Err(e) => {
                self.logger.reject();
                Err(e)
            }
        }
    }
    /// Read-only slice of all logs recorded during sandbox operation
    pub fn logs(&self) -> &[Event] {
        self.logger.logs()
    }
    /// Read-only slice of logs which were added by last successful operation
    pub fn latest_logs(&self) -> &[Event] {
        self.logger.latest_logs()
    }
}
/// Immutable state proxy, implements `dex::State`
#[doc(hidden)]
pub struct StateInner {
    contract: dex::Contract<Types>,
}

impl dex::State<Types> for StateInner {
    fn contract(&self) -> &dex::Contract<Types> {
        &self.contract
    }
}
/// Mutable state proxy, implements `dex::StateMut`
#[doc(hidden)]
pub struct StateInnerMut<'a> {
    caller_id: &'a AccountId,
    initiator_id: &'a AccountId,
    contract: &'a mut dex::Contract<Types>,
    item_factory: &'a mut ItemFactory,
    logger: &'a mut Logger,
}

impl<'a> dex::State<Types> for StateInnerMut<'a> {
    fn contract(&self) -> &dex::Contract<Types> {
        self.contract
    }
}

impl<'a> dex::StateMut<Types> for StateInnerMut<'a> {
    type SendTokensResult = ();

    type SendTokensExtraParam = ();

    fn members_mut(&mut self) -> dex::StateMembersMut<'_, Types> {
        dex::StateMembersMut {
            contract: self.contract,
            item_factory: self.item_factory,
            logger: self.logger,
        }
    }

    fn send_tokens(
        &mut self,
        account_id: &AccountId,
        token_id: &TokenId,
        _amount: Amount,
        unregister: bool,
        _extra: Self::SendTokensExtraParam,
    ) -> Self::SendTokensResult {
        self.contract
            .latest()
            .accounts
            .try_update(account_id, |dex::Account::V0(ref mut account)| {
                // Always succeed
                // TODO: may need ways to simulate failure

                // ... try unregister if requested
                if unregister {
                    account.unregister_tokens([token_id])?;
                }
                Ok(())
            })
            // Test harness, unwrap here is ok
            .unwrap();
    }

    fn get_initiator_id(&self) -> AccountId {
        self.initiator_id.clone()
    }

    fn get_caller_id(&self) -> AccountId {
        self.caller_id.clone()
    }
}
// Mock for extra account data
#[derive(Default)]
#[cfg_attr(feature = "near", derive(BorshSerialize, BorshDeserialize))]
#[cfg_attr(feature = "concordium", derive(Serialize))]
#[cfg_attr(feature = "multiversx", derive(NestedEncode, NestedDecode))]
pub struct AccountExtraTest {}
impl dex::AccountExtra for AccountExtraTest {}
/// Mock set of types, most containers are implemented as mock `Map`
#[doc(hidden)]
#[cfg_attr(feature = "multiversx", derive(NestedEncode, NestedDecode))]
pub struct Types;

impl dex::Types for Types {
    type Bound = PersistentBound;

    type ContractExtraV1 = ();

    type AccountsMap = Map<AccountId, Account<Self>>;

    type TickStatesMap = OrderedMap<dex::Tick, dex::TickState<Self>>;

    type AccountTokenBalancesMap = Map<TokenId, Amount>;

    type AccountWithdrawTracker = dex::withdraw_trackers::NoopTracker;

    type AccountExtra = AccountExtraTest;

    type PoolsMap = Map<PoolId, Pool<Self>>;

    type PoolPositionsMap = Map<PositionId, Position<Self>>;

    type AccountPositionsSet = Map<PositionId, ()>;

    type VerifiedTokensSet = Map<TokenId, ()>;

    type PositionToPoolMap = Map<PositionId, PoolId>;

    type AccountIdSet = Map<AccountId, ()>;

    #[cfg(feature = "smart-routing")]
    type TokenConnectionsMap = Map<TokenId, Self::TokensSet>;

    #[cfg(feature = "smart-routing")]
    type TokensSet = Map<TokenId, ()>;

    #[cfg(feature = "smart-routing")]
    type TopPoolsMap = Map<TokenId, Self::TokensSet>;

    #[cfg(feature = "smart-routing")]
    type TokensArraySet = Map<TokenId, ()>;
}
