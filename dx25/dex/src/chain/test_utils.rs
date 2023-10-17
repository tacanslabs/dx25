use std::sync::atomic::{AtomicU64, Ordering};

use multiversx_sc_scenario::DebugApi;

use super::{AccountId, Amount, TokenId};

static ACCOUNT_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
static TOKEN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn init_test_env() {
    // DX25 test env simply doesn't work without this
    let _ = DebugApi::dummy();
}

/// Create new unique account identifier
pub fn new_account_id() -> AccountId {
    // DX25 test env simply doesn't work without this
    let _ = DebugApi::dummy();

    let id_buf = ACCOUNT_ID_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .to_le_bytes();

    let mut bytes = [0u8; 32];
    bytes[..id_buf.len()].copy_from_slice(&id_buf);

    AccountId::new_from_bytes(&bytes)
}

/// Create new unique token identifier
pub fn new_token_id() -> TokenId {
    // DX25 test env simply doesn't work without this
    let _ = DebugApi::dummy();

    let full_name = TOKEN_ID_COUNTER.fetch_add(1, Ordering::SeqCst).to_string();

    TokenId::from_bytes(full_name.as_str())
}

/// Create token amount from u128 literal
///
/// Temporary workaround until numeric types are properly abstracted
pub fn new_amount(value: u128) -> Amount {
    Amount::from(value)
}

pub fn amount_as_u128(value: Amount) -> u128 {
    value.as_u128()
}
