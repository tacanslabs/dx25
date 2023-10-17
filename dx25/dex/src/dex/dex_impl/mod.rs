#![allow(unused_imports)]
pub mod estimations;

use super::errors::{ErrorKind, Result};
use super::traits::AccountExtra;
use super::util_types::{PoolId, Side};
use super::utils::swap_if;
use super::{
    state_types, Account, AccountLatest, AccountV0, AccountWithdrawTracker, Action, BasisPoints,
    DepositPayment, EstimateSwapExactResult, FeeLevel, ItemFactory, Logger, Map, MapRemoveKey,
    Pool, PoolInfo, PoolV0, PositionClosedInfo, PositionId, PositionInfo, PositionInit,
    PositionOpenedInfo, Range, Set, State, StateMembersMut, StateMut, SwapAction, SwapKind,
    SwapToPriceAction, Tick, Types, VersionInfo, BASIS_POINT_DIVISOR,
};
use crate::chain::{AccountId, Amount, AmountUFP, Liquidity, TokenId};
use crate::{dex, fp};
use crate::{ensure_here, error_here, Float};
use dex::latest::{FeeLevelsArray, RawFeeLevelsArray, NUM_FEE_LEVELS};
use dex::map_with_context::MapWithContext;
use dex::pool::pool_impl::{fee_rate_ticks, fee_rates_ticks, PoolImpl};
use dex::pool::Pool as _;
use dex::{validate_protocol_fee_fraction, PairExt, PoolUpdateReason};

use array_init::array_init;
use itertools::Itertools;
#[allow(unused)] // Some impls use it, some don't
use num_traits::{One, Zero};
use std::borrow::{Borrow, BorrowMut};
use std::cell::RefCell;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

// #[cfg(feature = "test-utils")]
// use super::Float;
use super::Path;
#[cfg(feature = "smart-routing")]
use crate::chain::FixedPointBig;

#[cfg(test)]
mod tests;

pub type AccountCallbackType<'a, T> =
    &'a mut dyn FnMut(&AccountId, &mut Account<T>, bool) -> Result<()>;

/// Represents result of action execution
#[derive(Debug)]
enum ActionResult<S> {
    RegisterAccount,
    RegisterTokens,
    SwapExactIn(Amount),
    SwapExactOut(Amount),
    SwapToPrice(Amount),
    Deposit,
    Withdraw(Option<S>),
    OpenPosition,
    ClosePosition,
    WithdrawFee,
}

pub struct Dex<T, S, SS> {
    state: SS,
    _phantom_s: PhantomData<S>,
    _phantom_t: PhantomData<T>,
}

impl<T: Types, S: State<T>, SS: Borrow<S>> Deref for Dex<T, S, SS> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        self.state.borrow()
    }
}

impl<T: Types, S: State<T>, SS: BorrowMut<S>> DerefMut for Dex<T, S, SS> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.borrow_mut()
    }
}

impl<T: Types, S: State<T>, SS: Borrow<S>> Dex<T, S, SS> {
    pub fn new(state: SS) -> Self {
        Self {
            state,
            _phantom_s: PhantomData,
            _phantom_t: PhantomData,
        }
    }
}
/// Mutable view into contract's state, but for specific account
struct AccountViewMut<'a, T: Types> {
    account_id: &'a AccountId,
    account: &'a mut AccountLatest<T>,
    pools: &'a mut state_types::PoolsMap<T>,
    pool_count: &'a mut u64,
    next_free_position_id: &'a mut u64,
    position_to_pool_id: &'a mut state_types::PositionToPoolMap<T>,

    item_factory: &'a mut dyn ItemFactory<T>,
    logger: &'a mut dyn Logger,
}

impl<T: Types, S: State<T>, SS: Borrow<S>> Dex<T, S, SS> {
    pub fn get_deposit(&self, account: &AccountId, token: &TokenId) -> Result<Amount> {
        self.contract()
            .as_ref()
            .accounts
            .try_inspect(account, |Account::V0(ref acc)| {
                acc.token_balances.try_inspect(token, |balance| *balance)
            })?
    }

    pub fn get_pool_info(&self, tokens: (TokenId, TokenId)) -> Result<Option<PoolInfo>> {
        let (pool_id, swapped) = PoolId::try_from_pair(tokens).map_err(|e| error_here!(e))?;
        let side = if swapped { Side::Right } else { Side::Left };
        let result = self
            .contract()
            .as_ref()
            .pools
            .inspect(&pool_id, |Pool::V0(ref pool)| pool.pool_info(side))
            .transpose()?;
        Ok(result)
    }

    pub fn get_pool_infos(&self) -> Result<Vec<(PoolId, PoolInfo)>> {
        let mut infos = Vec::new();
        for (pool_id, pool) in self.contract().as_ref().pools.iter() {
            let Pool::V0(ref pool) = &*pool;
            infos.push((pool_id.clone(), pool.pool_info(Side::Left)?));
        }
        Ok(infos)
    }

    pub fn get_position_info(&self, position_id: u64) -> Result<PositionInfo> {
        let contract = self.contract().as_ref();
        contract
            .position_to_pool_id
            .try_inspect(&position_id, |pool_id| {
                contract.pools.try_inspect(pool_id, |Pool::V0(ref pool)| {
                    pool.get_position_info(pool_id, position_id)
                })
            })??
    }

    pub fn get_positions_info(&self, position_ids: &[u64]) -> Vec<Option<PositionInfo>> {
        let contract = self.contract().as_ref();

        position_ids
            .iter()
            .map(|position_id| {
                contract
                    .position_to_pool_id
                    .try_inspect(position_id, |pool_id| {
                        contract.pools.try_inspect(pool_id, |Pool::V0(ref pool)| {
                            pool.get_position_info(pool_id, *position_id)
                        })
                    })
                    .and_then(|position| position.and_then(|pool| pool))
                    .ok()
            })
            .collect()
    }

    pub fn get_version(&self) -> VersionInfo {
        VersionInfo {
            version: env!("DEX_CORE_VERSION").to_string(),
        }
    }

    pub fn fee_rate_ticks(&self, fee_level: FeeLevel) -> BasisPoints {
        fee_rate_ticks(fee_level)
    }

    pub fn fee_rates_ticks(&self) -> [BasisPoints; NUM_FEE_LEVELS as usize] {
        fee_rates_ticks()
    }

    pub fn get_liqudity_fee_level_distribution(
        &self,
        tokens: (TokenId, TokenId),
    ) -> Result<Option<[Float; NUM_FEE_LEVELS as usize]>> {
        match self.get_pool_info(tokens)? {
            None => Ok(None),
            Some(PoolInfo { liquidities, .. }) => {
                let total_liquidity = liquidities
                    .iter()
                    .fold(Float::zero(), |total_liquidity, liquidity| {
                        total_liquidity + (*liquidity).into()
                    });

                if total_liquidity.is_zero() {
                    return Ok(None);
                }

                Ok(Some(liquidities.map(|liquidity| {
                    Float::from(liquidity) * 100.into() / total_liquidity
                })))
            }
        }
    }

    pub fn protocol_fee_fraction(&self) -> BasisPoints {
        self.contract().as_ref().protocol_fee_fraction
    }

    pub fn get_pool_ticks(&self, pool: (TokenId, TokenId), fee_level: u8) -> Option<usize> {
        let (pool_id, _swapped) = PoolId::try_from_pair(pool).ok()?;

        let contract = self.contract().as_ref();

        contract
            .pools
            .try_inspect(&pool_id, |Pool::V0(ref pool)| {
                Some(pool.tick_states[fee_level].len())
            })
            .unwrap_or(None)
    }

    #[cfg(feature = "test-utils")]
    pub fn eff_sqrtprices(
        &self,
        tokens: (TokenId, TokenId),
        direction: Side,
    ) -> Result<RawFeeLevelsArray<Float>> {
        let (pool_id, swapped) = PoolId::try_from_pair(tokens).map_err(|e| error_here!(e))?;
        let side = direction.opposite_if(swapped);
        self.contract()
            .as_ref()
            .pools
            .try_inspect(&pool_id, |Pool::V0(ref pool)| {
                [0, 1, 2, 3, 4, 5, 6, 7].map(|level| pool.eff_sqrtprice(level, side))
            })
    }

    #[cfg(feature = "smart-routing")]
    pub fn calculate_path_liquidity(&self, token_id_vec: &[TokenId]) -> Result<Liquidity> {
        match token_id_vec.len() {
            2 => Ok(self
                .total_liquidity_of_pair(token_id_vec[0].clone(), token_id_vec[1].clone())
                .map_err(|e| error_here!(e))?),
            3 => {
                let x = token_id_vec[0].clone();
                let a = token_id_vec[1].clone();
                let y = token_id_vec[2].clone();
                let liqiudity_xa = self
                    .total_liquidity_of_pair(x.clone(), a.clone())
                    .map_err(|e| error_here!(e))?;
                let liqiudity_ay = self
                    .total_liquidity_of_pair(a.clone(), y.clone())
                    .map_err(|e| error_here!(e))?;
                let price_xa = self
                    .price_of_pair(x, a.clone())
                    .map_err(|e| error_here!(e))?;
                let price_ay = self.price_of_pair(a, y).map_err(|e| error_here!(e))?;
                Ok(
                    ((liqiudity_xa * liqiudity_ay) / (price_xa * price_xa * price_ay * price_ay))
                        .integer_sqrt(),
                )
            }
            4 => {
                let x = token_id_vec[0].clone();
                let a = token_id_vec[1].clone();
                let b = token_id_vec[2].clone();
                let y = token_id_vec[3].clone();
                let liqiudity_xa = self
                    .total_liquidity_of_pair(x.clone(), a.clone())
                    .map_err(|e| error_here!(e))?;
                let liqiudity_ab = self
                    .total_liquidity_of_pair(a.clone(), b.clone())
                    .map_err(|e| error_here!(e))?;
                let liqiudity_by = self
                    .total_liquidity_of_pair(b.clone(), y.clone())
                    .map_err(|e| error_here!(e))?;
                let price_xa = self.price_of_pair(x, a).map_err(|e| error_here!(e))?;
                let price_by = self.price_of_pair(b, y).map_err(|e| error_here!(e))?;
                let fixed_point_big: FixedPointBig = ((liqiudity_xa * liqiudity_ab * liqiudity_by)
                    / (price_xa * price_xa * price_by * price_by))
                    .into();

                Ok(Liquidity::try_from(fixed_point_big.integer_cbrt())
                    .map_err(|e| error_here!(e))?)
            }
            _ => Err(error_here!(ErrorKind::InvalidParams)),
        }
    }

    #[cfg(feature = "smart-routing")]
    fn price_of_pair(&self, token_a: TokenId, token_b: TokenId) -> Result<Liquidity, ErrorKind> {
        let contract = self.contract().as_ref();
        let (pool_id, swapped) = PoolId::try_from_pair((token_a, token_b))?;
        let price = contract
            .pools
            .try_inspect(&pool_id, |Pool::V0(ref pool)| pool.reserves_ratio())
            .map_err(|e| e.kind)?;
        if swapped {
            Ok(price.recip())
        } else {
            Ok(price)
        }
    }

    #[cfg(feature = "smart-routing")]
    fn total_liquidity_of_pair(
        &self,
        token_a: TokenId,
        token_b: TokenId,
    ) -> Result<Liquidity, ErrorKind> {
        let (pool_id, _) = PoolId::try_from_pair((token_a, token_b))?;
        self.contract()
            .as_ref()
            .pools
            .try_inspect(&pool_id, |Pool::V0(ref pool)| pool.total_liquidity())
            .map_err(|e| e.kind)
    }
}

impl<T: Types, S: StateMut<T>, SS: BorrowMut<S>> Dex<T, S, SS> {
    fn with_account_mut<R>(
        &mut self,
        account_id: &AccountId,
        cb: impl FnOnce(AccountViewMut<'_, T>) -> Result<R>,
    ) -> Result<R> {
        let StateMembersMut {
            contract,
            item_factory,
            logger,
        } = self.members_mut();
        let contract = contract.latest();

        contract
            .accounts
            .try_update(account_id, |Account::V0(ref mut account)| {
                cb(AccountViewMut {
                    account_id,
                    account,
                    pools: &mut contract.pools,
                    pool_count: &mut contract.pool_count,
                    next_free_position_id: &mut contract.next_free_position_id,
                    position_to_pool_id: &mut contract.position_to_pool_id,
                    item_factory,
                    logger,
                })
            })
    }

    fn with_caller_account_mut<R>(
        &mut self,
        cb: impl FnOnce(AccountViewMut<'_, T>) -> Result<R>,
    ) -> Result<R> {
        let account_id = self.get_caller_id();
        self.with_account_mut(&account_id, cb)
    }
    /// Register caller's account in smart contract storage
    ///
    /// Same as `register_account_and_then` with `account_id: None` and no-op callback which always succeeds
    ///
    /// # Returns
    /// * `Ok(())` if succeeds, `Err(_)` if fails, for some reason
    pub fn register_account(&mut self) -> Result<()> {
        self.register_account_and_then(None, |_, _, _| Ok(()))
    }

    /// Register account and tokens if they weren't registered before.
    /// Uses a caller ID as an account ID to register.
    /// Used by platforms where we don't need to register account and tokens manually, during ops (cdex and dx25).
    ///
    /// # Returns
    /// * `Ok(())` if succeeds, `Err(_)` if fails, for some reason
    #[cfg(not(feature = "near"))]
    fn register_account_and_tokens(
        &mut self,
        account_id: Option<AccountId>,
        tokens: &[TokenId],
    ) -> Result<()> {
        self.register_account_and_then(account_id, |_, &mut Account::V0(ref mut account), _| {
            account.register_tokens(tokens);
            Ok(())
        })
    }
    /// Register token holder account and invoke provided callback over it
    ///
    /// # Arguments
    /// * `account_id` - either identifier of new account or `None`, in which case caller id is used
    /// * `register_cb` - callback which receives account identifier, mutable reference to account
    ///     data structure, and boolean flag which is `true` if account already exists; if returns `Err(_)`,
    ///     whole account registration is deemed failure
    ///
    /// # Returns
    /// Either success result of `register_cb` or failure, either caused by account registration itself
    /// or callback
    pub fn register_account_and_then<R>(
        &mut self,
        account_id: impl Into<Option<AccountId>>,
        register_cb: impl FnOnce(&AccountId, &mut Account<T>, bool /*exists*/) -> Result<R>,
    ) -> Result<R> {
        self.ensure_payable_api_resumed()?;

        let account_id: Option<AccountId> = account_id.into();
        let account_id = account_id.unwrap_or_else(|| self.get_caller_id());
        let StateMembersMut {
            contract,
            item_factory,
            ..
        } = self.members_mut();
        let contract = contract.latest();
        contract.accounts.update_or_insert(
            &account_id,
            || item_factory.new_account(),
            |account, exists| register_cb(&account_id, account, exists),
        )
    }

    /// Try unregister caller account, if one's found
    ///
    /// Equivalent to `unregister_account_with_cb(None, |_, _| Ok(()))`
    pub fn unregister_account(&mut self) -> Result<Option<()>> {
        self.unregister_account_with_cb(None, |_, _| Ok(()))
    }

    /// Try unregister user account from Dex
    ///
    /// If account identified by `account_id` parameter is found, it's first checked to not
    /// track any withdrawals and have no tokens in store; `unregister_cb` is called as last check;
    /// if all checks succeed, account is unregistered
    ///
    /// # Parameters
    /// * `account_id` - either identifier of new account or `None`, in which case caller id is used
    /// * `unregister_cb` - callback usable for additional checks before actually unregistering account;
    ///     can cancel unregistration by returning error
    ///
    /// # Returns
    /// * `Ok(None)` - if account isn't registered
    /// * `Ok(Some(result))` - return value of unregister callback, if all checks succeeded
    ///     and account was unregistered
    /// * `Err(error)` - if any error occured, including one returned by unregister callback
    pub fn unregister_account_with_cb<R>(
        &mut self,
        account_id: impl Into<Option<AccountId>>,
        unregister_cb: impl FnOnce(&AccountId, &Account<T>) -> Result<R>,
    ) -> Result<Option<R>> {
        self.ensure_payable_api_resumed()?;

        let account_id: Option<AccountId> = account_id.into();
        let account_id = account_id.unwrap_or_else(|| self.get_caller_id());

        let contract = self.contract_mut().latest();

        contract
            .accounts
            .inspect(&account_id, |account| {
                let Account::V0(ref acc) = account;
                ensure_here!(
                    acc.token_balances.is_empty(),
                    ErrorKind::TokensStorageNotEmpty
                );
                ensure_here!(acc.positions.is_empty(), ErrorKind::UserHasPositions);
                ensure_here!(
                    !acc.withdraw_tracker.is_any_withdraw_in_progress(),
                    ErrorKind::WithdrawInProgress
                );
                unregister_cb(&account_id, account)
            })
            .map(|r| {
                if r.is_ok() {
                    contract.accounts.remove(&account_id);
                }
                r
            })
            .transpose()
    }

    fn ensure_caller_is_owner(&self) -> Result<()> {
        ensure_here!(
            self.contract().as_ref().owner_id == &self.get_caller_id(),
            ErrorKind::PermissionDenied
        );
        Ok(())
    }

    fn ensure_suspended(&self) -> Result<()> {
        ensure_here!(
            self.contract().as_ref().suspended,
            ErrorKind::GuardChangeStateDenied
        );
        Ok(())
    }

    fn ensure_resumed(&self) -> Result<()> {
        ensure_here!(
            !self.contract().as_ref().suspended,
            ErrorKind::GuardChangeStateDenied
        );
        Ok(())
    }

    fn ensure_caller_is_guard(&self) -> Result<()> {
        let caller = self.get_caller_id();
        let contract = self.contract().as_ref();
        ensure_here!(
            contract.owner_id == &caller || contract.guards.contains_item(&caller),
            ErrorKind::PermissionDenied
        );
        Ok(())
    }

    pub(crate) fn ensure_payable_api_resumed(&self) -> Result<()> {
        ensure_here!(
            !self.contract().as_ref().suspended,
            ErrorKind::PayableAPISuspended
        );
        Ok(())
    }

    pub fn add_verified_tokens(&mut self, tokens: impl IntoIterator<Item = TokenId>) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        self.ensure_caller_is_owner()?;
        let contract = self.contract_mut().latest();
        let verified_tokens = &mut contract.verified_tokens;
        let mut new_tokens = Vec::new();

        for token in tokens {
            if !verified_tokens.contains_item(&token) {
                verified_tokens.add_item(token.clone());
                new_tokens.push(token);
            }
        }

        self.logger_mut().log_add_verified_tokens_event(&new_tokens);

        Ok(())
    }

    pub fn remove_verified_tokens(
        &mut self,
        tokens: impl IntoIterator<Item = TokenId>,
    ) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        self.ensure_caller_is_owner()?;
        let contract = self.contract_mut().latest();
        let verified_tokens = &mut contract.verified_tokens;
        let mut removed_tokens = Vec::new();
        for token in tokens {
            if verified_tokens.contains_item(&token) {
                verified_tokens.remove_item(&token);
                removed_tokens.push(token);
            }
        }

        self.logger_mut()
            .log_remove_verified_tokens_event(&removed_tokens);

        Ok(())
    }

    pub fn get_verified_tokens(&self) -> Vec<TokenId> {
        self.contract()
            .as_ref()
            .verified_tokens
            .iter()
            .map(|token| token.clone())
            .collect()
    }

    #[allow(clippy::clone_on_copy)]
    pub fn add_guard_accounts(
        &mut self,
        guard_accounts: impl IntoIterator<Item = AccountId>,
    ) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        self.ensure_caller_is_owner()?;
        let contract = self.contract_mut().latest();
        let guards = &mut contract.guards;
        let mut new_guards = Vec::new();

        for guard_account in guard_accounts {
            if !guards.contains_item(&guard_account) {
                guards.add_item(guard_account.clone());
                new_guards.push(guard_account);
            }
        }

        self.logger_mut().log_add_guard_accounts_event(&new_guards);

        Ok(())
    }

    pub fn remove_guard_accounts(
        &mut self,
        guard_accounts: impl IntoIterator<Item = AccountId>,
    ) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        self.ensure_caller_is_owner()?;
        let contract = self.contract_mut().latest();
        let guards = &mut contract.guards;
        let mut removed_guards = Vec::new();

        for guard_account in guard_accounts {
            if guards.contains_item(&guard_account) {
                guards.remove_item(&guard_account);
                removed_guards.push(guard_account);
            }
        }

        self.logger_mut()
            .log_remove_guard_accounts_event(&removed_guards);

        Ok(())
    }

    pub fn suspend_payable_api(&mut self) -> Result<()> {
        self.ensure_caller_is_guard()?;
        self.ensure_resumed()?;

        let contract = self.contract_mut().latest();
        contract.suspended = true;

        let caller_id = self.get_caller_id();
        self.logger_mut().log_suspend_payable_api_event(&caller_id);

        Ok(())
    }

    pub fn resume_payable_api(&mut self) -> Result<()> {
        self.ensure_caller_is_guard()?;
        self.ensure_suspended()?;

        let contract = self.contract_mut().latest();
        contract.suspended = false;

        let caller_id = self.get_caller_id();
        self.logger_mut().log_resume_payable_api_event(&caller_id);

        Ok(())
    }

    pub fn set_protocol_fee_fraction(&mut self, protocol_fee_fraction: BasisPoints) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        self.ensure_caller_is_owner()?;
        let contract = self.contract_mut().latest();
        contract.protocol_fee_fraction =
            validate_protocol_fee_fraction(protocol_fee_fraction).map_err(|e| error_here!(e))?;
        Ok(())
    }

    #[cfg_attr(feature = "concordium", allow(unused))]
    pub fn owner_withdraw(
        &mut self,
        token_id: &TokenId,
        amount: Amount,
        extra: S::SendTokensExtraParam,
    ) -> Result<S::SendTokensResult> {
        self.ensure_payable_api_resumed()?;
        self.ensure_caller_is_owner()?;
        ensure_here!(amount > Amount::zero(), ErrorKind::IllegalWithdrawAmount);
        let contract = self.contract_mut().latest();
        contract
            .accounts
            .try_update(&contract.owner_id, |Account::V0(ref mut account)| {
                // Note: subtraction and deregistration will be reverted if the promise fails.
                account
                    .withdraw(token_id, amount)
                    .map_err(|e| error_here!(e))
            })?;
        #[allow(clippy::clone_on_copy)] // Some blockchains have address copyable, some don't
        let owner_id = contract.owner_id.clone();

        Ok(self.send_tokens(&owner_id, token_id, amount, false, extra))
    }

    pub fn register_tokens<'a>(
        &mut self,
        account_id: &AccountId,
        tokens: impl IntoIterator<Item = &'a TokenId>,
    ) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        let contract = self.contract_mut().latest();
        contract
            .accounts
            .try_update(account_id, |Account::V0(ref mut account)| {
                account.register_tokens(tokens);
                Ok(())
            })
    }

    pub fn unregister_tokens<'a>(
        &mut self,
        account_id: &AccountId,
        tokens: impl IntoIterator<Item = &'a TokenId>,
    ) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        let contract = self.contract_mut().latest();
        contract
            .accounts
            .try_update(account_id, |Account::V0(ref mut account)| {
                account.unregister_tokens(tokens)
            })
    }

    pub fn deposit(
        &mut self,
        account_id: &AccountId,
        token_id: &TokenId,
        amount: Amount,
    ) -> Result<Amount> {
        self.ensure_payable_api_resumed()?;

        // We need manual token registration for NEAR to supply storage maintanance fee
        // Add other dex'es register account and tokens automatically
        #[cfg(not(feature = "near"))]
        #[allow(clippy::clone_on_copy)] // not all account ids are copyable
        self.register_account_and_tokens(Some(account_id.clone()), &[token_id.clone()])?;

        let StateMembersMut {
            contract, logger, ..
        } = self.members_mut();
        let contract = contract.latest();
        contract
            .accounts
            .try_update(account_id, |Account::V0(ref mut account)| {
                Self::deposit_impl(account_id, account, token_id, amount, logger)
            })
    }

    fn deposit_impl(
        account_id: &AccountId,
        account: &mut AccountV0<T>,
        token_id: &TokenId,
        amount: Amount,
        logger: &mut dyn Logger,
    ) -> Result<Amount> {
        let balance = account
            .deposit(token_id, amount)
            .map_err(|e| error_here!(e))?;
        logger.log_deposit_event(account_id, token_id, &amount, &balance);
        Ok(balance)
    }

    pub fn withdraw(
        &mut self,
        account_id: &AccountId,
        token_id: &TokenId,
        amount: Amount,
        unregister: bool,
        extra: S::SendTokensExtraParam,
    ) -> Result<Option<S::SendTokensResult>> {
        self.ensure_payable_api_resumed()?;
        let StateMembersMut {
            contract, logger, ..
        } = self.members_mut();
        let contract = contract.latest();

        let sender = contract
            .accounts
            .try_update(account_id, |Account::V0(ref mut account)| {
                Self::withdraw_impl(
                    account_id, account, token_id, amount, unregister, extra, logger,
                )
            })?;

        Ok(sender.map(|func| func(self)))
    }
    /// Internal implementation of token withdrawal, including event logging
    /// and sending tokens to new owner
    ///
    /// # Parameters
    /// * `account_id` - account identifier, used to schedule transfer
    /// * `account` - actual account record
    /// * `token_id` - token identifier
    /// * `amount` - amount to withdraw
    ///     * if 0 is specified, remaining balance is fully withdrawn;
    ///         if token balance is 0, no actual token send is performed, and deregistration
    ///         is performed in-place; token being not registered is also not an error
    ///
    /// # Returns
    /// * `Ok(None)` - if requested amount was zero, and balance was zero too or token wasn't registered,
    /// * `Ok(Some(closure))` - if nonzero amount was withdrawn. `closure` will perform
    ///     actual tokens send and return that send result. So function callers should call it like
    ///     `Self::withdraw_impl(...)?.map(|func| func(self))`
    /// * `Err(_)` if any error happens on the way
    fn withdraw_impl(
        account_id: &AccountId,
        account: &mut AccountLatest<T>,
        token_id: &TokenId,
        amount: Amount,
        unregister: bool,
        extra: S::SendTokensExtraParam,
        logger: &mut dyn Logger,
    ) -> Result<Option<impl FnOnce(&mut Self) -> S::SendTokensResult>> {
        // If amount is zero, we try withdraw all what remains
        let amount = if amount.is_zero() {
            // First, fetch balance
            match account.token_balances.inspect(token_id, |balance| *balance) {
                // No token, bail out immediately
                None => return Ok(None),
                Some(balance) =>
                // Balance is zero, unregister if requested and bail out
                {
                    if balance == Amount::zero() {
                        if unregister {
                            account.unregister_tokens([token_id])?;
                        }
                        return Ok(None);
                    }
                    // Balance nonzero, continue with normal withdrawal
                    balance
                }
            }
        } else {
            amount
        };
        // Should never happen
        debug_assert_ne!(amount, Amount::zero());

        // Perform withdraw
        let new_balance = account
            .withdraw(token_id, amount)
            .map_err(|e| error_here!(e))?;

        // Log event, happens regardless of transfer mode
        logger.log_withdraw_event(account_id, token_id, &amount, &new_balance);

        #[allow(clippy::clone_on_copy)] // not all account ids are copyable
        let account_id = account_id.clone();
        let token_id = token_id.clone();
        let sender = move |dex: &mut Self| {
            dex.send_tokens(&account_id, &token_id, amount, unregister, extra)
        };
        Ok(Some(sender))
    }

    /// Returns:
    ///  - `position_id`
    ///  - actually deposited amount of first token
    ///  - actually deposited amount of second token
    ///  - accounted net liquidity
    pub fn open_position(
        &mut self,
        token_a: &TokenId,
        token_b: &TokenId,
        fee_rate: BasisPoints,
        position: PositionInit,
    ) -> Result<(PositionId, Amount, Amount, Liquidity)> {
        self.ensure_payable_api_resumed()?;

        // We need manual token registration for NEAR to supply storage maintanance fee
        // Add other dex'es register account and tokens automatically
        #[cfg(not(feature = "near"))]
        self.register_account_and_tokens(None, &[token_a.clone(), token_b.clone()])?;

        self.with_caller_account_mut(|mut account_view| {
            Self::open_position_impl(token_a, token_b, fee_rate, position, &mut account_view)
        })
    }

    #[allow(clippy::too_many_lines)] // FIXME: refactor
    fn open_position_impl(
        // Actual parameters from pub func
        token_a: &TokenId,
        token_b: &TokenId,
        fee_rate: BasisPoints,
        position: PositionInit,
        // Passed down contract context
        account_view: &mut AccountViewMut<'_, T>,
    ) -> Result<(PositionId, Amount, Amount, Liquidity)> {
        let (pool_id, transposed) = PoolId::try_from_pair((token_a.clone(), token_b.clone()))
            .map_err(|e| error_here!(e))?;

        if !account_view.pools.contains_key(&pool_id) {
            account_view.account.extra.on_pool_created()?;
        }

        let position = position.transpose_if(transposed);
        let fee_rates = fee_rates_ticks();

        let position_id = *account_view.next_free_position_id;
        *account_view.next_free_position_id += 1;

        let factory = RefCell::new(&mut *account_view.item_factory);

        let fee_level: FeeLevel = fee_rates
            .iter()
            .find_position(|r| **r == fee_rate)
            .ok_or(error_here!(ErrorKind::IllegalFee))?
            .0
            .try_into()
            .map_err(|_| error_here!(ErrorKind::ConvOverflow))?;

        let (deposited_amounts, accounted_net_liquidity) = account_view.pools.update_or_insert(
            &pool_id,
            || {
                *account_view.pool_count += 1;
                let pool = factory.borrow_mut().new_pool()?;
                Ok(pool)
            },
            |Pool::V0(ref mut pool), _| {
                let PositionOpenedInfo {
                    deposited_amounts,
                    net_liquidity,
                    low_tick_liquidity_change,
                    high_tick_liquidity_change,
                } = pool.open_position(position, fee_level, position_id, *factory.borrow_mut())?;

                ensure_here!(
                    !account_view.account.positions.contains_item(&position_id),
                    ErrorKind::PositionAlreadyExists
                );

                // Subtract updated amounts from deposits.
                // This will fail if there is not enough funds for any of the tokens.
                account_view
                    .account
                    .withdraw(&pool_id.0, deposited_amounts.0)
                    .map_err(|e| error_here!(e))?;
                account_view
                    .account
                    .withdraw(&pool_id.1, deposited_amounts.1)
                    .map_err(|e| error_here!(e))?;

                account_view.account.positions.add_item(position_id);

                account_view
                    .position_to_pool_id
                    .insert(position_id, pool_id.clone());

                for (tick, liquidity_change) in
                    [low_tick_liquidity_change, high_tick_liquidity_change]
                {
                    account_view.logger.log_tick_update_event(
                        pool_id.as_refs(),
                        fee_level,
                        tick,
                        liquidity_change,
                    );
                }

                let (tick_low, tick_high) =
                    (low_tick_liquidity_change.0, high_tick_liquidity_change.0);

                // Event is emitted here because method is also called by add_simple_pool directly
                account_view.logger.log_open_position_event(
                    account_view.account_id,
                    pool_id.as_refs(),
                    deposited_amounts.as_refs(),
                    fee_rate,
                    position_id,
                    (tick_low, tick_high),
                );

                Self::log_pool_v0_state(
                    &pool_id,
                    pool,
                    account_view.logger,
                    PoolUpdateReason::AddLiquidity,
                );

                Ok((deposited_amounts, net_liquidity))
            },
        )?;

        let deposited_amounts_in_user_order = swap_if(transposed, deposited_amounts);
        Ok((
            position_id,
            deposited_amounts_in_user_order.0,
            deposited_amounts_in_user_order.1,
            accounted_net_liquidity,
        ))
    }

    /// Returns:
    ///  - `position_id`
    ///  - actually deposited amount of first token
    ///  - actually deposited amount of second token
    ///  - accounted net liquidity
    pub fn open_position_full(
        &mut self,
        token_a: &TokenId,
        token_b: &TokenId,
        fee_rate: BasisPoints,
        amount_a: Amount,
        amount_b: Amount,
    ) -> Result<(PositionId, Amount, Amount, Liquidity)> {
        self.open_position(
            token_a,
            token_b,
            fee_rate,
            PositionInit {
                amount_ranges: (
                    Range {
                        min: Amount::one().into(),
                        max: amount_a.into(),
                    },
                    Range {
                        min: Amount::one().into(),
                        max: amount_b.into(),
                    },
                ),
                ticks_range: (None, None),
            },
        )
    }

    pub fn close_position(&mut self, position_id: PositionId) -> Result<()> {
        self.ensure_payable_api_resumed()?;
        self.with_caller_account_mut(|mut account_view| {
            Self::close_position_impl(position_id, &mut account_view)
        })
    }

    fn close_position_impl(
        position_id: PositionId,
        account_view: &mut AccountViewMut<'_, T>,
    ) -> Result<()> {
        // Get pool_id and at the same time check if position exists
        let (pool_id, fees, amounts, tick_updates, fee_level) =
            account_view
                .position_to_pool_id
                .try_inspect(&position_id, |pool_id| {
                    // Check if the caller is the owner of the position,
                    // and remove the position from `account_to_positions`
                    ensure_here!(
                        account_view.account.positions.contains_item(&position_id),
                        ErrorKind::NotYourPosition
                    );

                    account_view.account.positions.remove_item(&position_id);

                    // Do close the position along with widrawing the fees,
                    // and deposit the assets on the owner's account
                    account_view.pools.try_update_or(
                        pool_id,
                        // Inconsistent state: position is present in `position_to_pool_id`,
                        // but the pool doesn't exist
                        ErrorKind::InternalLogicError,
                        |Pool::V0(ref mut pool)| {
                            let PositionClosedInfo {
                                fees,
                                balance: amounts,
                                fee_level,
                                low_tick_liquidity_change,
                                high_tick_liquidity_change,
                            } = pool.withdraw_fee_and_close_position(position_id)?;

                            account_view
                                .account
                                .deposit(&pool_id.0, amounts.0 + fees.0)
                                .map_err(|e| error_here!(e))?;
                            account_view
                                .account
                                .deposit(&pool_id.1, amounts.1 + fees.1)
                                .map_err(|e| error_here!(e))?;
                            Ok((
                                pool_id.clone(),
                                fees,
                                amounts,
                                [low_tick_liquidity_change, high_tick_liquidity_change],
                                fee_level,
                            ))
                        },
                    )
                })??;

        account_view.position_to_pool_id.remove(&position_id);

        for (tick, liquidity_change) in tick_updates {
            account_view.logger.log_tick_update_event(
                pool_id.as_refs(),
                fee_level,
                tick,
                liquidity_change,
            );
        }

        account_view.logger.log_harvest_fee_event(position_id, fees);

        account_view
            .logger
            .log_close_position_event(position_id, amounts);

        account_view.pools.inspect(&pool_id, |Pool::V0(ref pool)| {
            Self::log_pool_v0_state(
                &pool_id,
                pool,
                account_view.logger,
                PoolUpdateReason::RemoveLiquidity,
            );
        });

        Ok(())
    }

    pub fn withdraw_fee(&mut self, position_id: PositionId) -> Result<(Amount, Amount)> {
        self.ensure_payable_api_resumed()?;
        self.with_caller_account_mut(|mut account_view| {
            Self::withdraw_fee_impl(position_id, &mut account_view)
        })
    }

    fn withdraw_fee_impl(
        position_id: PositionId,
        account_view: &mut AccountViewMut<'_, T>,
    ) -> Result<(Amount, Amount)> {
        // Get pool_id and at the same time check if position exists:
        let amounts = account_view
            .position_to_pool_id
            .try_inspect(&position_id, |pool_id| {
                // Position exists. Check if the caller is the owner of the position:
                ensure_here!(
                    account_view.account.positions.contains_item(&position_id),
                    ErrorKind::NotYourPosition
                );

                // Do withdraw the fee and deposit the assets on the owner's account:
                account_view.pools.try_update_or(
                    pool_id,
                    // Inconsistent state: position is present in `position_to_pool_id`,
                    // but the pool doesn't exist
                    ErrorKind::InternalLogicError,
                    |Pool::V0(ref mut pool)| {
                        let fees = pool.withdraw_fee(position_id)?;
                        account_view
                            .account
                            .deposit(&pool_id.0, fees.0)
                            .map_err(|e| error_here!(e))?;
                        account_view
                            .account
                            .deposit(&pool_id.1, fees.1)
                            .map_err(|e| error_here!(e))?;
                        Ok(fees)
                    },
                )
            })??;

        account_view
            .logger
            .log_harvest_fee_event(position_id, amounts);

        Ok(amounts)
    }

    pub fn withdraw_protocol_fee(
        &mut self,
        pool_id: (TokenId, TokenId),
    ) -> Result<(Amount, Amount)> {
        self.ensure_payable_api_resumed()?;
        let sender_id = self.get_caller_id();
        let contract = self.contract_mut().latest();
        ensure_here!(contract.owner_id == sender_id, ErrorKind::PermissionDenied);

        let (pool_id, swapped) = PoolId::try_from_pair(pool_id).map_err(|e| error_here!(e))?;
        let protocol_fees = contract
            .pools
            .try_update(&pool_id, |Pool::V0(ref mut pool)| {
                let protocol_fees = pool.withdraw_protocol_fee()?;

                contract
                    .accounts
                    .try_update(&sender_id, |Account::V0(ref mut account)| {
                        account
                            .deposit(&pool_id.0, protocol_fees.0)
                            .map_err(|e| error_here!(e))?;
                        account
                            .deposit(&pool_id.1, protocol_fees.1)
                            .map_err(|e| error_here!(e))?;

                        Ok(())
                    })?;

                Ok(protocol_fees)
            })?;
        Ok(swap_if(swapped, protocol_fees))
    }

    /// Common implementation of `execute_actions` and `deposit_execute_actions`, handles all actions
    /// with respect to execution context
    #[allow(clippy::too_many_lines)] // Because of lengthy worker functions invocations. Relatively simple otherwise
    fn execute_actions_impl(
        &mut self,
        account_id: &AccountId,
        mut deposit_data: &[DepositPayment],
        register_account_cb: AccountCallbackType<'_, T>,
        actions: Vec<Action<S::SendTokensExtraParam>>,
    ) -> Result<Vec<ActionResult<S::SendTokensResult>>> {
        // Var to allow deposit only once
        let mut deposit_handled = false;
        // First, we use peeking to process possible register account request
        // before we visit account
        let mut actions = actions.into_iter().peekable();
        // Keeps results of all actions. Withdraws contain send callbacks which are remapped
        // into results after main loop
        let mut results = Vec::with_capacity(actions.size_hint().0);
        // Track chains of swaps
        let mut prev_swap_action: Option<(TokenId, SwapKind, Amount)> = None;

        if let Some(Action::RegisterAccount) = actions.peek() {
            // take it out of batch
            std::mem::drop(actions.next());
            // register account
            #[allow(clippy::clone_on_copy)] // not all account ids are copyable
            self.register_account_and_then(account_id.clone(), register_account_cb)?;
            results.push(ActionResult::RegisterAccount);
        } else {
            // All dex'es except NEAR register account automatically
            #[cfg(not(feature = "near"))]
            #[allow(clippy::clone_on_copy)] // not all account ids are copyable
            self.register_account_and_then(account_id.clone(), register_account_cb)?;
        }

        let protocol_fee_fraction = self.protocol_fee_fraction();

        // Process rest of actions
        self.with_account_mut(account_id, |mut account_view| {
            for action in actions {
                let result = match action {
                    Action::RegisterAccount => {
                        return Err(error_here!(ErrorKind::UnexpectedRegisterAccount));
                    }
                    Action::RegisterTokens(tokens) => {
                        account_view.account.register_tokens(&tokens);
                        ActionResult::RegisterTokens
                    }
                    Action::SwapExactIn(action) => {
                        // All dex'es except NEAR register tokens automatically
                        #[cfg(not(feature = "near"))]
                        account_view
                            .account
                            .register_tokens(&[action.token_in.clone(), action.token_out.clone()]);

                        let swap_result = Self::execute_swap_action(
                            account_id,
                            account_view.account,
                            account_view.pools,
                            account_view.logger,
                            &prev_swap_action,
                            SwapKind::ExactIn,
                            action,
                            protocol_fee_fraction,
                        )?;
                        let swap_amount = swap_result.2;
                        prev_swap_action = Some(swap_result);
                        ActionResult::SwapExactIn(swap_amount)
                    }
                    Action::SwapExactOut(action) => {
                        // All dex'es except NEAR register tokens automatically
                        #[cfg(not(feature = "near"))]
                        account_view
                            .account
                            .register_tokens(&[action.token_in.clone(), action.token_out.clone()]);

                        let swap_result = Self::execute_swap_action(
                            account_id,
                            account_view.account,
                            account_view.pools,
                            account_view.logger,
                            &prev_swap_action,
                            SwapKind::ExactOut,
                            action,
                            protocol_fee_fraction,
                        )?;
                        let swap_amount = swap_result.2;
                        prev_swap_action = Some(swap_result);
                        ActionResult::SwapExactOut(swap_amount)
                    }
                    Action::SwapToPrice(action) => {
                        // All dex'es except NEAR register tokens automatically
                        #[cfg(not(feature = "near"))]
                        account_view
                            .account
                            .register_tokens(&[action.token_in.clone(), action.token_out.clone()]);

                        let swap_result = Self::execute_swap_to_price_action(
                            account_id,
                            account_view.account,
                            account_view.pools,
                            account_view.logger,
                            &prev_swap_action,
                            action,
                            protocol_fee_fraction,
                        )?;
                        let swap_amount = swap_result.2;
                        prev_swap_action = Some(swap_result);
                        ActionResult::SwapToPrice(swap_amount)
                    }
                    Action::Deposit => {
                        // Only single deposit allowed
                        ensure_here!(!deposit_handled, ErrorKind::DepositAlreadyHandled);
                        deposit_handled = true;
                        ensure_here!(!deposit_data.is_empty(), ErrorKind::DepositNotAllowed);

                        for payment in deposit_data {
                            // All dex'es except NEAR register tokens automatically
                            #[cfg(not(feature = "near"))]
                            account_view
                                .account
                                .register_tokens(&[payment.token_id.clone()]);

                            let _: Amount = Self::deposit_impl(
                                account_id,
                                account_view.account,
                                &payment.token_id,
                                payment.amount,
                                account_view.logger,
                            )?;
                        }

                        deposit_data = &[];
                        ActionResult::Deposit
                    }
                    Action::Withdraw(token_id, amount, extra) => {
                        // Because not all `WasmAmount`'s are copyable
                        let amount: Amount = amount.into();
                        let do_send = Self::withdraw_impl(
                            account_id,
                            account_view.account,
                            &token_id,
                            amount,
                            false,
                            extra,
                            account_view.logger,
                        )?;
                        ActionResult::Withdraw(do_send.map(Box::new))
                    }
                    Action::OpenPosition {
                        tokens: (token_a, token_b),
                        fee_rate,
                        position,
                    } => {
                        // If we have single-sided position, frontend doesn't generate deposit actions
                        // This leads to `TokenNotRegistered` error. We fix this here
                        #[cfg(not(feature = "near"))]
                        account_view
                            .account
                            .register_tokens(&[token_a.clone(), token_b.clone()]);

                        let _: (u64, Amount, Amount, Liquidity) = Self::open_position_impl(
                            &token_a,
                            &token_b,
                            fee_rate,
                            position,
                            &mut account_view,
                        )?;
                        ActionResult::OpenPosition
                    }
                    Action::ClosePosition(position_id) => {
                        Self::close_position_impl(position_id, &mut account_view)?;
                        ActionResult::ClosePosition
                    }
                    Action::WithdrawFee(position_id) => {
                        Self::withdraw_fee_impl(position_id, &mut account_view)?;
                        ActionResult::WithdrawFee
                    }
                };
                results.push(result);
            }
            Ok(())
        })?;

        // Deposit must be handled if requested
        ensure_here!(deposit_data.is_empty(), ErrorKind::DepositNotHandled);

        // Transform inner result into outer one
        let results = results
            .into_iter()
            .map(|r| match r {
                // Only withdrawal needs actual transformation
                ActionResult::Withdraw(r) => ActionResult::Withdraw(r.map(|func| func(self))),
                // Rest is just transformed as-is
                ActionResult::RegisterAccount => ActionResult::RegisterAccount,
                ActionResult::RegisterTokens => ActionResult::RegisterTokens,
                ActionResult::SwapExactIn(amount) => ActionResult::SwapExactIn(amount),
                ActionResult::SwapExactOut(amount) => ActionResult::SwapExactOut(amount),
                ActionResult::SwapToPrice(amount) => ActionResult::SwapToPrice(amount),
                ActionResult::Deposit => ActionResult::Deposit,
                ActionResult::OpenPosition => ActionResult::OpenPosition,
                ActionResult::ClosePosition => ActionResult::ClosePosition,
                ActionResult::WithdrawFee => ActionResult::WithdrawFee,
            })
            .collect();

        Ok(results)
    }
    /// Execute batch of actions passed as additional payload during extrnal deposit operation
    ///
    /// Please note that:
    /// * `RegisterAccount` action should appear in batch at most once, as the first action
    /// * `Deposit` action should appear exactly once in batch
    ///
    /// # Parameters
    /// * `account_id` - account for which actions should be executed; must be transaction initiator/signer
    /// * `deposit_token_id` - token identifier to deposit
    /// * `deposit_amount` - token amount to deposit
    /// * `register_account_cb` - callback which is called if account registration is requested
    /// * `actions` - list of actions to actually execute
    ///
    /// # Returns
    /// * if operation succeeds, vector of `(usize, TokenId, Amount, S::SendTokensResult)`, where
    ///     * `usize` is the index of `Withdraw` operation in batch
    ///     * `TokenId` and `Amount` describe withdrawal request parameters
    ///     * `S::SendTokensResult` is the actual result of `send_tokens` call
    /// * If it fails, failure reason is returned
    pub fn deposit_execute_actions(
        &mut self,
        account_id: &AccountId,
        deposit_data: &[DepositPayment],
        register_account_cb: AccountCallbackType<'_, T>,
        actions: Vec<Action<S::SendTokensExtraParam>>,
    ) -> Result<Vec<S::SendTokensResult>> {
        self.ensure_payable_api_resumed()?;

        ensure_here!(
            account_id == &self.get_initiator_id(),
            ErrorKind::DepositSenderMustBeSigner
        );

        let results = self
            .execute_actions_impl(account_id, deposit_data, register_account_cb, actions)?
            .into_iter()
            .filter_map(|r| {
                if let ActionResult::Withdraw(Some(r)) = r {
                    Some(r)
                } else {
                    None
                }
            })
            .collect();

        Ok(results)
    }

    /// Execute batch of actions passed as normal request
    pub fn execute_actions(
        &mut self,
        register_account_cb: AccountCallbackType<'_, T>,
        actions: Vec<Action<S::SendTokensExtraParam>>,
    ) -> Result<(Vec<S::SendTokensResult>, Option<Amount>)> {
        self.execute_actions_for(&self.get_caller_id(), register_account_cb, actions)
    }

    pub fn execute_actions_for(
        &mut self,
        account_id: &AccountId,
        register_account_cb: AccountCallbackType<T>,
        actions: Vec<Action<S::SendTokensExtraParam>>,
    ) -> Result<(Vec<S::SendTokensResult>, Option<Amount>)> {
        self.ensure_payable_api_resumed()?;

        let mut out_amount = None;

        let results = self
            .execute_actions_impl(account_id, &[], register_account_cb, actions)?
            .into_iter()
            .filter_map(|r| match r {
                ActionResult::Withdraw(Some(r)) => Some(r),
                ActionResult::SwapExactIn(amount)
                | ActionResult::SwapExactOut(amount)
                | ActionResult::SwapToPrice(amount) => {
                    out_amount = Some(amount);
                    None
                }
                _ => None,
            })
            .collect();

        Ok((results, out_amount))
    }

    pub fn swap_exact_in(
        &mut self,
        tokens: &[TokenId],
        amount_in: Amount,
        min_amount_out: Amount,
    ) -> Result<(Amount, Amount)> {
        ensure_here!(tokens.len() >= 2, ErrorKind::AtLeastOneSwap);

        let mut amount_out = amount_in;
        for (token_in, token_out) in tokens.iter().tuple_windows() {
            amount_out = self
                .swap(token_in, token_out, SwapKind::ExactIn, None, amount_out)?
                .1;
        }

        ensure_here!(amount_out >= min_amount_out, ErrorKind::Slippage);

        self.post_swap_update(tokens, amount_in, amount_out)?;

        Ok((amount_in, amount_out))
    }

    pub fn swap_exact_out(
        &mut self,
        tokens: &[TokenId],
        amount_out: Amount,
        max_amount_in: Amount,
    ) -> Result<(Amount, Amount)> {
        ensure_here!(tokens.len() >= 2, ErrorKind::AtLeastOneSwap);

        let mut amount_in = amount_out;
        for (token_in, token_out) in tokens.iter().tuple_windows() {
            amount_in = self
                .swap(token_in, token_out, SwapKind::ExactOut, None, amount_in)?
                .0;
        }

        ensure_here!(amount_in <= max_amount_in, ErrorKind::Slippage);

        self.post_swap_update(tokens, amount_in, amount_out)?;

        Ok((amount_in, amount_out))
    }

    pub fn swap_to_price(
        &mut self,
        tokens: &[TokenId],
        amount_in: Amount,
        effective_price_limit: Float,
    ) -> Result<(Amount, Amount)> {
        ensure_here!(tokens.len() == 2, ErrorKind::ExactOneSwap);

        let (amount_in, amount_out) = self.swap(
            &tokens[0],
            &tokens[1],
            SwapKind::ToPrice,
            Some(effective_price_limit),
            amount_in,
        )?;

        self.post_swap_update(tokens, amount_in, amount_out)?;

        Ok((amount_in, amount_out))
    }

    /// Returns (Amount in, Amount out)
    // XXX: Don't switch `effective_price_limit` and `amount` order. There's a bug when `amount` just dissapears
    // from parameters if it goes before `Option<Float>` in MX. If you do this, check if it still works by calling
    // any `swap_*` function
    pub fn swap(
        &mut self,
        token_in: &TokenId,
        token_out: &TokenId,
        swap_type: SwapKind,
        effective_price_limit: Option<Float>,
        amount: Amount,
    ) -> Result<(Amount, Amount)> {
        self.ensure_payable_api_resumed()?;

        // We need manual token registration for NEAR to supply storage maintanance fee
        // Add other dex'es register account and tokens automatically
        #[cfg(not(feature = "near"))]
        self.register_account_and_tokens(None, &[token_in.clone(), token_out.clone()])?;

        let (pool_id, swapped) = PoolId::try_from_pair((token_in.clone(), token_out.clone()))
            .map_err(|e| error_here!(e))?;
        let direction = if swapped { Side::Right } else { Side::Left };

        let contract = self.contract_mut().latest();
        // Pool uses square effective price. Need to convert here
        let max_eff_sqrtprice_limit = effective_price_limit.map(|limit| limit.sqrt());

        let (amount_in, amount_out, _num_tick_crossings) =
            contract
                .pools
                .try_update(&pool_id, |Pool::V0(ref mut pool)| {
                    pool.swap(
                        direction,
                        swap_type,
                        amount,
                        contract.protocol_fee_fraction,
                        max_eff_sqrtprice_limit,
                    )
                })?;

        self.log_pool_state(&pool_id, PoolUpdateReason::Swap)?;

        Ok((amount_in, amount_out))
    }

    fn post_swap_update(
        &mut self,
        tokens: &[TokenId],
        amount_in: Amount,
        amount_out: Amount,
    ) -> Result<()> {
        let (Some(first_token), Some(last_token)) = (tokens.iter().next(), tokens.iter().next_back()) else {
            // Should never fail - function requires at least 2 input tokens
            return Err(error_here!(ErrorKind::InternalLogicError));
        };

        let caller_id = &self.get_caller_id();

        let contract = self.contract_mut().latest();
        contract
            .accounts
            .try_update(caller_id, |Account::V0(ref mut account)| {
                account
                    .withdraw(first_token, amount_in)
                    .map_err(|e| error_here!(e))?;
                account
                    .deposit(last_token, amount_out)
                    .map_err(|e| error_here!(e))
            })?;

        self.logger_mut().log_swap_event(
            caller_id,
            (first_token, last_token),
            (&amount_in, &amount_out),
            &[], // TODO: add fees into swap event
        );

        Ok(())
    }

    pub fn multiple_path_swap_exact_in(
        &mut self,
        paths: &[Path],
        min_amount_out: Amount,
    ) -> Result<Vec<(Amount, Amount)>> {
        self.ensure_payable_api_resumed()?;

        let amount_pairs = self.multiple_path_swap(paths, SwapKind::ExactIn)?;

        let mut sum = Amount::zero();
        for (_, amount_out) in amount_pairs.clone() {
            sum += amount_out;
        }
        ensure_here!(sum >= min_amount_out, ErrorKind::Slippage);

        let caller_id = &self.get_caller_id();
        let contract = self.contract_mut().latest();

        for (i, path) in paths.iter().enumerate() {
            //unfallible unwrap as the length of `amount_pairs` is same as the length of `paths`
            let (amount_in, amount_out) = amount_pairs.get(i).unwrap();
            contract
                .accounts
                .try_update(caller_id, |Account::V0(ref mut account)| {
                    account
                        .withdraw(&path.tokens[0], *amount_in)
                        .map_err(|e| error_here!(e))?;
                    account
                        .deposit(&path.tokens[path.tokens.len() - 1], *amount_out)
                        .map_err(|e| error_here!(e))
                })?;
        }

        Ok(amount_pairs)
    }

    pub fn multiple_path_swap_exact_out(
        &mut self,
        paths: &[Path],
        max_amount_in: Amount,
    ) -> Result<Vec<(Amount, Amount)>> {
        self.ensure_payable_api_resumed()?;

        let amount_pairs = self.multiple_path_swap(paths, SwapKind::ExactOut)?;

        let mut sum = Amount::zero();
        for (amount_in, _) in amount_pairs.clone() {
            sum += amount_in;
        }
        ensure_here!(sum >= max_amount_in, ErrorKind::Slippage);

        let caller_id = &self.get_caller_id();
        let contract = self.contract_mut().latest();

        for (i, path) in paths.iter().enumerate() {
            //unfallible unwrap as the length of `amount_pairs` is same as the length of `paths`
            let (amount_in, amount_out) = amount_pairs.get(i).unwrap();
            contract
                .accounts
                .try_update(caller_id, |Account::V0(ref mut account)| {
                    account
                        .withdraw(path.tokens.first().unwrap(), *amount_in)
                        .map_err(|e| error_here!(e))?;
                    account
                        .deposit(path.tokens.last().unwrap(), *amount_out)
                        .map_err(|e| error_here!(e))
                })?;
        }

        Ok(amount_pairs)
    }

    fn multiple_path_swap(
        &mut self,
        paths: &[Path],
        swap_type: SwapKind,
    ) -> Result<Vec<(Amount, Amount)>> {
        let mut amounts = vec![];
        for path in paths {
            let mut amount: Amount = path.amount;
            for (token_in, token_out) in path.tokens.iter().tuple_windows() {
                amount = match swap_type {
                    SwapKind::ExactIn => {
                        self.swap(token_in, token_out, SwapKind::ExactIn, None, amount)?
                            .1
                    }
                    SwapKind::ExactOut => {
                        self.swap(token_in, token_out, SwapKind::ExactOut, None, amount)?
                            .0
                    }
                    SwapKind::ToPrice => unreachable!("Should never happen"),
                };
            }

            match swap_type {
                SwapKind::ExactIn => amounts.push((path.amount, amount)),
                SwapKind::ExactOut => amounts.push((amount, path.amount)),
                SwapKind::ToPrice => unreachable!("Should never happen"),
            }
        }

        Ok(amounts)
    }
    /// Perform single swap action
    ///
    /// NB: returns `Option` with swap result just for convenience,
    /// to simplify assignment to `prev_swap_result`
    #[allow(clippy::too_many_arguments)]
    fn execute_swap_action(
        account_id: &AccountId,
        account: &mut AccountV0<T>,
        pools: &mut state_types::PoolsMap<T>,
        logger: &mut dyn Logger,
        prev_swap_result: &Option<(TokenId, SwapKind, Amount)>,
        swap_type: SwapKind,
        action: SwapAction,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(TokenId, SwapKind, Amount)> {
        let SwapAction {
            token_in,
            token_out,
            amount,
            amount_limit,
        } = action;
        let amount: Option<Amount> = amount.map(Into::into);
        let amount_limit: Amount = amount_limit.into();

        // This should never happen. This function only works with ExactIn and ExactOut
        ensure_here!(
            !matches!(swap_type, SwapKind::ToPrice),
            ErrorKind::InternalLogicError
        );
        ensure_here!(
            account.token_balances.contains_key(&token_in),
            ErrorKind::TokenNotRegistered
        );
        ensure_here!(
            account.token_balances.contains_key(&token_out),
            ErrorKind::TokenNotRegistered
        );

        let amount = amount.map_or_else(
            || {
                // If amount is None, it should be correctly inherited from prev operation
                prev_swap_result.as_ref().map_or_else(
                    || Err(error_here!(ErrorKind::WrongActionResult)),
                    |(prev_token_id, prev_swap_type, prev_amount)| {
                        // Only if swap direction is the same
                        if swap_type != *prev_swap_type {
                            return Err(error_here!(ErrorKind::WrongActionResult));
                        }
                        // We match prev token id with current input token
                        let curr_token_id = match swap_type {
                            SwapKind::ExactIn => &token_in,
                            SwapKind::ExactOut => &token_out,
                            SwapKind::ToPrice => unreachable!("Should never happen"),
                        };
                        // Only if previous result token matches current start token
                        if prev_token_id != curr_token_id {
                            return Err(error_here!(ErrorKind::WrongActionResult));
                        }
                        Ok(*prev_amount)
                    },
                )
            },
            Ok,
        )?;
        let (pool_id, swapped) = PoolId::try_from_pair((token_in.clone(), token_out.clone()))
            .map_err(|e| error_here!(e))?;

        let (amount_in, amount_out) = pools.try_update(&pool_id, |Pool::V0(ref mut pool)| {
            let side = if swapped { Side::Right } else { Side::Left };

            let (amount_in, amount_out) = match swap_type {
                SwapKind::ExactIn => {
                    let (amount_in, amount_out, _num_tick_crossings) =
                        pool.swap_exact_in(side, amount, protocol_fee_fraction)?;
                    ensure_here!(amount_out >= amount_limit, ErrorKind::Slippage);
                    (amount_in, amount_out)
                }
                SwapKind::ExactOut => {
                    let (amount_in, amount_out, _num_tick_crossings) =
                        pool.swap_exact_out(side, amount, protocol_fee_fraction)?;
                    ensure_here!(amount_in <= amount_limit, ErrorKind::Slippage);
                    (amount_in, amount_out)
                }
                SwapKind::ToPrice => unreachable!("Should never happen"),
            };
            account
                .withdraw(&token_in, amount_in)
                .map_err(|e| error_here!(e))?;
            account
                .deposit(&token_out, amount_out)
                .map_err(|e| error_here!(e))?;

            // Log swap event and pool state
            logger.log_swap_event(
                account_id,
                (&token_in, &token_out),
                (&amount_in, &amount_out),
                &[], // TODO: add fees into swap event
            );
            Self::log_pool_v0_state(&pool_id, pool, logger, PoolUpdateReason::Swap);

            Ok((amount_in, amount_out))
        })?;
        Ok(match swap_type {
            SwapKind::ExactIn => (token_out, swap_type, amount_out),
            SwapKind::ExactOut => (token_in, swap_type, amount_in),
            SwapKind::ToPrice => unreachable!("Should never happen"),
        })
    }

    /// Perform single swap action
    ///
    /// NB: returns `Option` with swap result just for convenience,
    /// to simplify assignment to `prev_swap_result`
    #[allow(clippy::too_many_arguments)]
    fn execute_swap_to_price_action(
        account_id: &AccountId,
        account: &mut AccountV0<T>,
        pools: &mut state_types::PoolsMap<T>,
        logger: &mut dyn Logger,
        prev_swap_result: &Option<(TokenId, SwapKind, Amount)>,
        action: SwapToPriceAction,
        protocol_fee_fraction: BasisPoints,
    ) -> Result<(TokenId, SwapKind, Amount)> {
        let SwapToPriceAction {
            token_in,
            token_out,
            amount,
            effective_price_limit,
        } = action;
        let amount: Option<Amount> = amount.map(Into::into);

        // Pool uses square effective price. Need to convert here
        let max_eff_sqrtprice = effective_price_limit.sqrt();

        ensure_here!(prev_swap_result.is_none(), ErrorKind::ExactOneSwap);
        ensure_here!(
            account.token_balances.contains_key(&token_in),
            ErrorKind::TokenNotRegistered
        );
        ensure_here!(
            account.token_balances.contains_key(&token_out),
            ErrorKind::TokenNotRegistered
        );

        // We only allow a single swap to price actions
        let Some(amount) = amount else { return Err(error_here!(ErrorKind::ExactOneSwap)) };

        let (pool_id, swapped) = PoolId::try_from_pair((token_in.clone(), token_out.clone()))
            .map_err(|e| error_here!(e))?;

        let (_, amount_out) = pools.try_update(&pool_id, |Pool::V0(ref mut pool)| {
            let side = if swapped { Side::Right } else { Side::Left };

            let (amount_in, amount_out, _num_tick_crossings) =
                pool.swap_to_price(side, amount, max_eff_sqrtprice, protocol_fee_fraction)?;

            account
                .withdraw(&token_in, amount_in)
                .map_err(|e| error_here!(e))?;
            account
                .deposit(&token_out, amount_out)
                .map_err(|e| error_here!(e))?;

            // Log swap event and pool state
            logger.log_swap_event(
                account_id,
                (&token_in, &token_out),
                (&amount_in, &amount_out),
                &[], // TODO: add fees into swap event
            );
            Self::log_pool_v0_state(&pool_id, pool, logger, PoolUpdateReason::Swap);

            Ok((amount_in, amount_out))
        })?;

        // Swapt to price is basically swapping in
        Ok((token_out, SwapKind::ExactIn, amount_out))
    }

    fn log_pool_state(&mut self, pool_id: &PoolId, reason: PoolUpdateReason) -> Result<()> {
        let StateMembersMut {
            contract, logger, ..
        } = self.members_mut();
        let contract = contract.latest();

        contract.pools.try_inspect(pool_id, |Pool::V0(ref pool)| {
            Self::log_pool_v0_state(pool_id, pool, logger, reason);
        })
    }

    fn log_pool_v0_state(
        pool_id: &PoolId,
        pool: &PoolV0<T>,
        logger: &mut dyn Logger,
        reason: PoolUpdateReason,
    ) {
        let position_reserves = pool.position_reserves();
        let amounts_a = position_reserves.map(|(left, _right)| Amount::try_from(left).unwrap());
        let amounts_b = position_reserves.map(|(_left, right)| Amount::try_from(right).unwrap());
        let spot_sqrtprices = pool.spot_sqrtprices(Side::Right);
        let liquidities = pool
            .liquidities()
            .map(|liq| liq.try_into().unwrap_or_default());

        logger.log_update_pool_state_event(
            reason,
            (&pool_id.0, &pool_id.1),
            &amounts_a,
            &amounts_b,
            &spot_sqrtprices,
            &liquidities,
        );
    }

    pub fn log_ticks_liquidity_change(
        &mut self,
        pool: (TokenId, TokenId),
        fee_level: FeeLevel,
        start_tick: i32,
        number: u8,
    ) -> Result<i32> {
        self.with_caller_account_mut(|mut account_view| {
            Self::log_ticks_liquidity_change_impl(
                pool,
                fee_level,
                start_tick,
                number,
                &mut account_view,
            )
        })
    }

    fn log_ticks_liquidity_change_impl(
        pool: (TokenId, TokenId),
        fee_level: FeeLevel,
        start_tick: i32,
        number: u8,
        account_view: &mut AccountViewMut<'_, T>,
    ) -> Result<i32> {
        let (pool_id, _) = PoolId::try_from_pair(pool).map_err(|e| error_here!(e))?;
        let start_tick = Tick::new(start_tick).map_err(|e| error_here!(e))?;

        let liquiduty_changes =
            account_view
                .pools
                .try_inspect(&pool_id, |Pool::V0(ref pool)| {
                    pool.get_ticks_liquidity_change(fee_level, Side::Left, start_tick, number)
                })?;

        for (tick, liquidity_change) in &liquiduty_changes {
            account_view.logger.log_tick_update_event(
                pool_id.as_refs(),
                fee_level,
                *tick,
                *liquidity_change,
            );
        }

        liquiduty_changes
            .last()
            .map(|(tick, _)| tick.index())
            .ok_or(error_here!(ErrorKind::InternalTickNotFound))
    }
}
