use super::{dex, Sandbox};
use crate::chain::{AccountId, Amount, TokenId};
use dex::test_utils::{new_amount, Event};

pub struct BalanceTracker {
    pub balances: Vec<(TokenId, Option<Amount>)>,
    pub account_id: AccountId,
}

#[derive(Clone)]
pub enum Change {
    /// Balance reaches specific value
    Exact(Amount),
    /// Balance should become bigger by specified amount
    Inc(Amount),
    /// Balance should be smaller by specified amount
    Dec(Amount),
    /// No change should happen. `None` and `Some(0)` balances are treated as equal
    NoChange,
    /// No change should happen at all. `None` and `Some(0)` are treated as different
    #[allow(unused)]
    NoChangeExact,
    /// Read actual change from logged events
    #[allow(unused)]
    FromLogs,
}

impl Change {
    fn add(&mut self, other: Change) {
        use std::cmp::Ordering::{Equal, Greater, Less};
        use Change::{Dec, Exact, FromLogs, Inc, NoChange, NoChangeExact};

        *self = match (&self, other) {
            // Should never happen
            (_, FromLogs | NoChangeExact | Exact(_)) => unreachable!(),
            // NoChange and FromLogs behave the same, they change to right-hand operand
            (NoChange | FromLogs, r) => r,
            (NoChangeExact, _) => NoChangeExact,
            // Exact-modes aren't changed
            (Exact(v), _) => Exact(*v),
            // Work with the rest
            (Inc(l), Inc(r)) => Inc(*l + r),
            (Inc(l), Dec(r)) => match l.cmp(&r) {
                Greater => Inc(*l - r),
                Less => Dec(r - *l),
                Equal => NoChange,
            },
            (Inc(l), NoChange) => Inc(*l),
            (Dec(l), Inc(r)) => match l.cmp(&r) {
                Greater => Dec(*l - r),
                Less => Inc(r - *l),
                Equal => NoChange,
            },
            (Dec(l), Dec(r)) => Dec(*l + r),
            (Dec(l), NoChange) => Dec(*l),
        };
    }
}

impl BalanceTracker {
    pub fn new<'a>(
        sandbox: &Sandbox,
        account_id: &AccountId,
        token_ids: impl IntoIterator<Item = &'a TokenId>,
    ) -> Self {
        Self {
            balances: sandbox.call(|dex| {
                token_ids
                    .into_iter()
                    .map(|id| (id.clone(), dex.get_deposit(account_id, id).ok()))
                    .collect()
            }),
            account_id: account_id.clone(),
        }
    }

    #[allow(unused)]
    pub fn new_with_caller<'a>(
        sandbox: &Sandbox,
        token_ids: impl IntoIterator<Item = &'a TokenId>,
    ) -> Self {
        Self::new(sandbox, sandbox.caller_id(), token_ids)
    }

    #[allow(unused)]
    pub fn new_with_initiator<'a>(
        sandbox: &Sandbox,
        token_ids: impl IntoIterator<Item = &'a TokenId>,
    ) -> Self {
        Self::new(sandbox, sandbox.initiator_id(), token_ids)
    }

    #[track_caller]
    pub fn assert_changes(self, sandbox: &Sandbox, changes: impl IntoIterator<Item = Change>) {
        let BalanceTracker {
            balances,
            account_id,
        } = self;

        let mut changes: Vec<_> = changes
            .into_iter()
            .map(|c| (matches!(c, Change::FromLogs), c))
            .collect();
        assert_eq!(
            balances.len(),
            changes.len(),
            "Changes sequence has incorrect length!"
        );

        let mut apply_change =
            |user: &AccountId, token: &TokenId, amount: &Amount, ctor: fn(Amount) -> Change| {
                assert_eq!(&account_id, user);
                let idx = balances
                    .iter()
                    .enumerate()
                    .find_map(|(idx, (tok, _))| if tok == token { Some(idx) } else { None })
                    .map_or_else(
                        || Err(format!("{token:?} was not expected to be touched at all")),
                        Ok,
                    )
                    .unwrap();
                if let (true, ref mut change) = changes[idx] {
                    change.add(ctor(*amount));
                }
            };
        // Gather changes from logs, if needed
        for event in sandbox.latest_logs() {
            match event {
                Event::Deposit {
                    user,
                    token,
                    amount,
                    ..
                } => apply_change(user, token, amount, Change::Inc),
                Event::Withdraw {
                    user,
                    token,
                    amount,
                    ..
                } => apply_change(user, token, amount, Change::Dec),
                Event::Swap {
                    user,
                    tokens,
                    amounts,
                    ..
                } => {
                    apply_change(user, &tokens.0, &amounts.0, Change::Dec);
                    apply_change(user, &tokens.1, &amounts.1, Change::Inc);
                }
                _ => (),
            }
        }
        // Perform actual check
        sandbox.call(|dex| {
            for ((tok, old_bal), (_, change)) in balances.into_iter().zip(changes.into_iter()) {
                let new_bal = dex.get_deposit(&account_id, &tok).ok();
                Self::assert_change(&tok, old_bal, change, new_bal);
            }
        });
    }
    /// Assert balance changes gathered from latest operation logs
    #[track_caller]
    fn assert_change(tok: &TokenId, old: Option<Amount>, change: Change, new: Option<Amount>) {
        use Change::{Dec, Exact, FromLogs, Inc, NoChange, NoChangeExact};

        match (old, change, new) {
            // Increments
            (Some(old), Inc(inc), Some(new)) => {
                assert_eq!(
                    old + inc,
                    new,
                    "{tok:?} balance didn't increase as expected"
                );
            }
            (None, Inc(inc), Some(new)) => {
                assert_eq!(inc, new, "{tok:?} balance didn't increase as expected");
            }
            (Some(old), Inc(inc), None) => {
                assert_eq!(
                    old + inc,
                    new_amount(0),
                    "{tok:?} balance didn't increase as expected"
                );
            }
            (None, Inc(inc), None) => {
                assert_eq!(
                    inc,
                    new_amount(0),
                    "{tok:?} balance didn't increase as expected"
                );
            }
            // Decrements
            (Some(old), Dec(dec), Some(new)) => {
                assert_eq!(
                    old - dec,
                    new,
                    "{tok:?} balance didn't decrease as expected"
                );
            }
            (Some(old), Dec(dec), None) => {
                assert_eq!(old, dec, "{tok:?} balance didn't decrease as expected");
            }
            (None, Dec(dec), Some(new)) => {
                assert_eq!(
                    new_amount(0),
                    dec + new,
                    "{tok:?} balance didn't decrease as expected"
                );
            }
            (None, Dec(dec), None) => {
                assert_eq!(
                    new_amount(0),
                    dec,
                    "{tok:?} balance didn't decrease as expected"
                );
            }
            // No changes
            (Some(old), NoChange, Some(new)) => {
                assert_eq!(old, new, "{tok:?} balance changed unexpectedly");
            }
            (Some(old), NoChange, None) => {
                assert_eq!(old, new_amount(0), "{tok:?} balance changed unexpectedly");
            }
            (None, NoChange, Some(new)) => {
                assert_eq!(new_amount(0), new, "{tok:?} balance changed unexpectedly");
            }
            (None, NoChange, None) => (),
            // Only full equality
            (old, NoChangeExact, new) => assert_eq!(
                old, new,
                "{tok:?} balance should not have been touched at all"
            ),
            // Exact equality
            (_, Exact(left), Some(right)) => assert_eq!(
                left, right,
                "{tok:?} balance should be equal to {left} but instead equals {right}"
            ),
            (_, Exact(left), None) => panic!(
                "{tok:?} balance should be equal to {left} but instead it isn't registered at all"
            ),
            // Should not happen here at all
            (_, FromLogs, _) => unreachable!(),
        }
    }
}
