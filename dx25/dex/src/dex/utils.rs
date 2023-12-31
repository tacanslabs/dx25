use crate::dex::{Float, PoolId};
use crate::TokenId;
use std::borrow::Borrow;

/// Swap values in pair if condition is `true`, return unchanged otherwise
pub fn swap_if<T>(condition: bool, pair: (T, T)) -> (T, T) {
    if condition {
        (pair.1, pair.0)
    } else {
        pair
    }
}
/// Similar to `assert!`, but bails out with specified error instead of panicking
///
/// # Parameters
/// * cond - condition which should succeed
/// * error - expression which should resolve to error value
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $error:expr) => {
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !($cond) {
            std::result::Result::Err($error)?;
        }
    };
}

/// Assert float values are equal with given relative tolerance.
///
/// Values are considered equal if relative difference is
/// less than `1/(2^f64::MANTISSA_EXPLICIT_BITS` - `tolerance_bits`)
///
/// # Arguments:
///  * `left` -- first value to compare
///  * `right` -- second value to compare
///  * `tol_bits` -- number of tolerance bits
///
#[macro_export]
#[cfg(not(target_arch = "wasm32"))]
macro_rules! assert_eq_rel_tol {
    ($left:expr, $right:expr, $tol_bits:expr $(,)?) => {{
        let left = Float::try_from($left).unwrap();
        let right = Float::try_from($right).unwrap();

        if !(left.is_zero() && right.is_zero()) {
            let abs_diff = (left - right).abs();
            let abs_mean = (left + right).abs() / Float::from(2u64);
            let rel_diff = abs_diff / abs_mean;
            let rel_tol = Float::from(1u64 << (52 - $tol_bits + 1)).recip();
            assert!(
                rel_diff < rel_tol,
                "Values: {}, {}, rel.diff: {:?}, rel.tol.: {:?}",
                $left,
                $right,
                Float::from(rel_diff),
                Float::from(rel_tol)
            );
        }
    }};
}

pub trait MinSome<T: Ord> {
    fn min_some(self, other: Option<T>) -> Option<T>;
}

impl<T: Ord> MinSome<T> for Option<T> {
    fn min_some(self, other: Option<T>) -> Option<T> {
        match (&self, &other) {
            (_, None) => self,
            (None, _) => other,
            _ => self.min(other),
        }
    }
}

pub trait PairExt<T>: Into<(T, T)> {
    fn as_refs(&self) -> (&T, &T);

    fn map<U, F>(self, f: F) -> (U, U)
    where
        F: Fn(T) -> U,
    {
        let (l, r) = self.into();
        (f(l), f(r))
    }

    fn map_into<U>(self) -> (U, U)
    where
        T: Into<U>,
    {
        self.map(Into::into)
    }

    fn cloned<U>(self) -> (U, U)
    where
        T: Borrow<U>,
        U: Clone,
    {
        let (l, r) = self.into();
        (U::clone(l.borrow()), U::clone(r.borrow()))
    }

    fn try_map<U, E, F>(self, f: F) -> Result<(U, U), E>
    where
        F: Fn(T) -> Result<U, E>,
    {
        let (l, r) = self.into();
        Ok((f(l)?, f(r)?))
    }

    fn try_map_into<U, E>(self) -> Result<(U, U), E>
    where
        T: TryInto<U, Error = E>,
    {
        self.try_map(TryInto::try_into)
    }
}

impl<T> PairExt<T> for (T, T) {
    fn as_refs(&self) -> (&T, &T) {
        (&self.0, &self.1)
    }
}

impl PairExt<TokenId> for PoolId {
    fn as_refs(&self) -> (&TokenId, &TokenId) {
        self.as_refs()
    }
}

pub fn next_down(a: Float) -> Float {
    // We must use strictly integer arithmetic to prevent denormals from
    // flushing to zero after an arithmetic operation on some platforms.
    const NEG_TINY_BITS: u64 = 0x8000_0000_0000_0001; // Smallest (in magnitude) negative f64.
    const CLEAR_SIGN_MASK: u64 = 0x7fff_ffff_ffff_ffff;
    const NEG_INFINITY_BITS: u64 = 0xfff0_0000_0000_0000;

    let bits = a.to_bits();
    if a.is_nan() || bits == NEG_INFINITY_BITS {
        return a;
    }

    let abs = bits & CLEAR_SIGN_MASK;
    let next_bits = if abs == 0 {
        NEG_TINY_BITS
    } else if bits == abs {
        bits - 1
    } else {
        bits + 1
    };
    Float::from_bits(next_bits)
}

pub fn next_up(a: Float) -> Float {
    // We must use strictly integer arithmetic to prevent denormals from
    // flushing to zero after an arithmetic operation on some platforms.
    const TINY_BITS: u64 = 0x1; // Smallest positive f64.
    const CLEAR_SIGN_MASK: u64 = 0x7fff_ffff_ffff_ffff;
    const INFINITY_BITS: u64 = 0x7ff0_0000_0000_0000;

    let bits = a.to_bits();
    if a.is_nan() || bits == INFINITY_BITS {
        return a;
    }

    let abs = bits & CLEAR_SIGN_MASK;
    let next_bits = if abs == 0 {
        TINY_BITS
    } else if bits == abs {
        bits + 1
    } else {
        bits - 1
    };
    Float::from_bits(next_bits)
}
