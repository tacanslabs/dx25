use super::{new_account_id, new_amount, new_token_id, Sandbox};
use crate::{
    chain::{AccountId, Amount, TokenId},
    dex::PositionId,
};
/// Minimal context for testing swap operations
///
/// Includes:
/// * unique account identifier, both creator and owner of contract, registered in contract
/// * two unique token identifiers, registered as tokens for account, with default deposits
/// * single pool with single position, for two tokens, registered by owner account
pub struct SwapTestContext {
    pub sandbox: Sandbox,
    pub owner: AccountId,
    pub token_ids: (TokenId, TokenId),
    pub position_id: PositionId,
}

impl Default for SwapTestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl SwapTestContext {
    /// Construct test context with some default values
    pub fn new() -> Self {
        Self::new_with_amounts(
            (new_amount(500_011), new_amount(5_000_110)),
            (new_amount(500_000), new_amount(5_000_000)),
        )
    }
    /// Create test context with account, two tokens with 10^9 deposit each,
    /// and position opened with 10^9 amounts each
    pub fn new_all_1g() -> Self {
        Self::new_with_amounts(
            (new_amount(1_000_000_000), new_amount(1_000_000_000)),
            (new_amount(1_000_000_000), new_amount(1_000_000_000)),
        )
    }

    pub fn new_with_amounts(
        position: (Amount, Amount),
        deposits: (Amount, Amount),
    ) -> SwapTestContext {
        let acc = new_account_id();
        let mut sandbox = Sandbox::new_default(acc.clone());

        sandbox.call_mut(|dex| dex.register_account()).unwrap();

        let token_0 = new_token_id();
        let token_1 = new_token_id();

        let mut ctx = SwapTestContext {
            sandbox,
            owner: acc.clone(),
            token_ids: (token_0.clone(), token_1.clone()),
            position_id: 0,
        };

        ctx.position_id = ctx.open_position((&token_0, &token_1), position);

        ctx.sandbox
            .call_mut(|dex| dex.deposit(&acc, &token_0, deposits.0))
            .unwrap();
        ctx.sandbox
            .call_mut(|dex| dex.deposit(&acc, &token_1, deposits.1))
            .unwrap();

        ctx
    }

    pub fn open_position(
        &mut self,
        tokens: (&TokenId, &TokenId),
        amounts: (Amount, Amount),
    ) -> PositionId {
        self.sandbox
            .call_mut(|dex| dex.register_tokens(&self.owner, [tokens.0, tokens.1]))
            .unwrap();

        self.sandbox
            .call_mut(|dex| dex.deposit(&self.owner, tokens.0, amounts.0))
            .unwrap();

        self.sandbox
            .call_mut(|dex| dex.deposit(&self.owner, tokens.1, amounts.1))
            .unwrap();

        self.sandbox
            .call_mut(|dex| dex.open_position_full(tokens.0, tokens.1, 1, amounts.0, amounts.1))
            .unwrap()
            .0
    }

    pub fn open_position_1g(&mut self, tokens: (&TokenId, &TokenId)) -> PositionId {
        self.open_position(
            tokens,
            (new_amount(1_000_000_000), new_amount(1_000_000_000)),
        )
    }
}
