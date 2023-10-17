use super::{Error, I192X192, U192X192};
use crate::chain::Float;

// T - unsigned fixed point type, e.g. U128X128 or U192X64
// TOT_SIZE - total size in 64-bit words, e.g. 4 for U128X128 or 6 for U192X192
// FRACT_SIZE - size of fractional part, e.g. 2 for U128X128 or 1 for U192X64
pub(crate) fn try_mantissa_exponent_to_ufp<T, const TOT_SIZE: usize, const FRACT_SIZE: usize>(
    mantissa: u64,
    exponent: i16,
) -> Result<T, Error>
where
    T: From<[u64; TOT_SIZE]>,
{
    // Position of the lower word w.r.t. the "comma": -S/2 for least significant word, S/2 - 1 for most significant word.
    // Equal to exponent/64 rounded towards -inf.
    // As integer division is performed with rounding towards zero (which is towards +inf for opposite-sign operands),
    // we offset the exponent by +2048 (arbitrary value, larger than abs of mininmal exponent value, retruned by f64::integer_decode(), which is -1075),
    // so that the operands are positive and the division is performed with rounding towards -inf.
    // Finally we remove the offset.
    let lower_word_position = (exponent + 2048) / 64 - 2048 / 64;

    // By how much we need shift the mantissa left, so that it aligns with U128X128 words
    let upscale_to_align = exponent - lower_word_position * 64;
    // FIXME: either never happens or produces error?
    assert!((0..64).contains(&upscale_to_align));

    let lower_word = mantissa << upscale_to_align;
    let higher_word = if upscale_to_align == 0 {
        0
    } else {
        mantissa >> (64 - upscale_to_align)
    };

    // Indices of the words in the underlying array.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let lower_word_index = lower_word_position + FRACT_SIZE as i16;
    let higher_word_index = lower_word_index + 1;

    let mut as_array = [0_u64; TOT_SIZE];

    for (word, index) in [
        (lower_word, lower_word_index),
        (higher_word, higher_word_index),
    ] {
        if word > 0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            if index >= TOT_SIZE as i16 {
                return Err(Error::Overflow);
            }
            if index < 0 {
                return Err(Error::PrecisionLoss);
            }
            #[allow(clippy::cast_sign_loss)]
            {
                as_array[index as usize] = word;
            }
        }
    }

    Ok(T::from(as_array))
}

pub(crate) fn try_float_to_ufp<T, const TOT_SIZE: usize, const FRACT_SIZE: usize>(
    v: Float,
) -> Result<T, Error>
where
    T: From<[u64; TOT_SIZE]>,
{
    if v.is_nan() {
        return Err(Error::NaN);
    }
    if v.is_infinity() {
        return Err(Error::Overflow);
    }

    let (mantissa, exponent, sign) = v.integer_decode();

    if sign < 0 {
        Err(Error::NegativeToUnsigned)
    } else {
        try_mantissa_exponent_to_ufp::<T, TOT_SIZE, FRACT_SIZE>(mantissa, exponent)
    }
}

// T - symmetric fixed point type, e.g. U128X128 or U192X192
// S - full size in 64-bit words, i.e. 4 for U128X128 or 6 for U192X192
fn try_f64_to_ufp<T, const S: usize, const H: usize>(v: f64) -> Result<T, Error>
where
    T: From<[u64; S]>,
{
    let (mantissa, exponent, sign) = num_traits::Float::integer_decode(v);

    if sign < 0 {
        Err(Error::NegativeToUnsigned)
    } else {
        try_mantissa_exponent_to_ufp::<T, S, H>(mantissa, exponent)
    }
}

#[allow(unused)]
pub fn try_f64_to_u192x192(v: f64) -> Result<U192X192, Error> {
    try_f64_to_ufp::<U192X192, 6, 3>(v)
}

#[allow(unused)]
pub fn try_f64_to_i192x192(v: f64) -> Result<I192X192, Error> {
    let (_, _, sign) = num_traits::Float::integer_decode(v);
    let unsigned = v * f64::from(sign);
    Ok(I192X192 {
        value: try_f64_to_u192x192(unsigned)?,
        non_negative: sign >= 0,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fp::U128X128;
    use float_extras::f64::ldexp;

    /// Shift the value left until the first digit becomes '1'
    fn shift_left_max(mut value: u128) -> u128 {
        assert_ne!(value, 0);
        while value & (1 << 127) == 0 {
            value <<= 1;
        }
        value
    }

    #[test]
    fn test_try_f64_to_u128x128_large() {
        assert_eq!(
            U128X128::try_from(Float::from(ldexp(1_f64, 127))).unwrap(),
            U128X128::from(1u128 << 127)
        );

        assert_eq!(
            U128X128::try_from(Float::from(ldexp(f64::from(0b_1111_1111_1111), 128 - 12))).unwrap(),
            U128X128::from(0b_1111_1111_1111_u128 << (128 - 12))
        );
    }

    #[test]
    fn test_try_f64_to_u128x128_zero() {
        assert_eq!(
            U128X128::try_from(Float::zero()).unwrap(),
            U128X128::from(0u128)
        );
    }

    #[test]
    fn test_try_f64_to_u192x192_zero() {
        assert_eq!(try_f64_to_u192x192(0f64).unwrap(), U192X192::from(0u128));
    }

    #[test]
    fn test_try_f64_to_u128x128_tiny() {
        assert_eq!(
            U128X128::try_from(Float::from(ldexp(1_f64, -128)))
                .unwrap()
                .0
                 .0,
            [1_u64, 0_u64, 0_u64, 0_u64]
        );

        assert_eq!(
            U128X128::try_from(Float::from(ldexp(287_f64, -128)))
                .unwrap()
                .0
                 .0,
            [287_u64, 0_u64, 0_u64, 0_u64]
        );

        assert_eq!(
            U128X128::try_from(Float::from(ldexp(113_f64, -64)))
                .unwrap()
                .0
                 .0,
            [0_u64, 113_u64, 0_u64, 0_u64]
        );
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn test_try_f64_to_u128x128_from_leading_and_trailing_parts() {
        let leading_part = 0b1_1010_1011_u128; // digits leading the dot
        let trailing_part = 0b100_1001_u128; // digits trailing the dot
        let value_f64 = (leading_part as f64) + ldexp(shift_left_max(trailing_part) as f64, -128);
        let value_u128x128 = U128X128::try_from(Float::from(value_f64)).unwrap();
        assert_eq!(value_u128x128.upper_part(), leading_part);
        assert_eq!(value_u128x128.lower_part(), shift_left_max(trailing_part));
    }

    fn assert_eq_errors(e1: &Error, e2: &Error) {
        assert_eq!(format!("{e1:?}"), format!("{e2:?}"));
    }

    #[test]
    fn test_try_f64_to_u128x128_overflow() {
        assert_eq_errors(
            &U128X128::try_from(Float::from(ldexp(1_f64, 128))).unwrap_err(),
            &Error::Overflow,
        );
    }

    #[test]
    fn test_try_f64_to_u128x128_prec_loss() {
        assert_eq_errors(
            &U128X128::try_from(Float::from(ldexp(1_f64, -129))).unwrap_err(),
            &Error::PrecisionLoss,
        );
    }

    #[test]
    fn test_try_f64_to_u128x128_negative() {
        assert_eq_errors(
            &U128X128::try_from(Float::from(-0.15)).unwrap_err(),
            &Error::NegativeToUnsigned,
        );
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn test_try_f64_to_u192x192() {
        assert_eq!(
            try_f64_to_u192x192(1.).unwrap().0 .0,
            [0u64, 0u64, 0u64, 1u64, 0u64, 0u64]
        );
    }
}
