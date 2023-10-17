use crate::chain::Amount;
use crate::dex::{Error, ErrorKind, Float};
use crate::{ensure, error_here};

#[cfg(feature = "near")]
use num_traits::{One, Zero};

pub struct Fraction {
    pub nominator: Amount,
    pub denominator: Amount,
}

impl Fraction {
    fn zero() -> Self {
        Self {
            nominator: Amount::zero(),
            denominator: Amount::zero(),
        }
    }
}

impl From<Fraction> for Float {
    fn from(value: Fraction) -> Self {
        Float::from(value.nominator) / Float::from(value.denominator)
    }
}

impl TryFrom<Float> for Fraction {
    type Error = Error;

    fn try_from(value: Float) -> Result<Self, Self::Error> {
        if value.is_zero() {
            return Ok(Self::zero());
        }

        let (mantissa, exponent, sign) = value.integer_decode();
        // Cross-check: price must be strictly positive
        ensure!(sign == 1, error_here!(ErrorKind::InternalLogicError));

        // Ensure exponent is not too large (or small) to avoid overflow in bit shift.
        // Price may be below 2^-128 or exceed 2^128:
        ensure!(
            -128 < exponent && exponent < 128,
            error_here!(ErrorKind::InternalLogicError)
        );
        if exponent >= 0 {
            Ok(Self {
                nominator: Amount::from(mantissa) << exponent,
                denominator: Amount::one(),
            })
        } else {
            Ok(Self {
                nominator: Amount::from(mantissa),
                denominator: Amount::one() << (-exponent),
            })
        }
    }
}
