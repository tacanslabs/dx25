pub use super::{amount_as_u128, new_account_id, new_amount, new_token_id};
/// Checks that any of elements in iterable collection matches specified pattern
///
/// # Parameters:
/// * `$iterable` - expression which resolves into any iterable collection
/// * `$pats` - matching patterns which should follow rules for `std::matches` macro
///
/// Intended usage:
/// ```ignore
/// assert_any_matches!(
///     sandbox.latest_logs(),
///     Event::Deposit { .. }
/// );
/// ```
#[macro_export]
macro_rules! assert_any_matches {
    ($iterable:expr, $($pats:tt)+) => {
        let result = 'outer: loop {
            for item in $iterable {
                if matches!(item, $($pats)+) {
                    break 'outer true;
                }
            }

            break 'outer false;
        };
        if !result {
            panic!("assertion failed: no elements in `{}` matched pattern `{}`",
                stringify!($iterable), stringify!($($pats)+))
        }
    };
}
