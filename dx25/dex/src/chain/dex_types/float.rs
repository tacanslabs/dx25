use std::ops::Range;

use multiversx_sc::{
    abi::{TypeAbi, TypeName},
    codec::{NestedDecode, NestedEncode, TopDecode, TopEncode},
};
use softfloat_wrapper::{RoundingMode, SoftFloat, F64};

use super::FixedPoint;
use crate::fp::Error;
use crate::wrap_float;
use crate::UInt;

/// Fee divisor, allowing to provide fee in bps.
const DEFAULT_OPS_ROUNDING: RoundingMode = RoundingMode::TiesToAway;

wrap_float! {
    pub F64 {
        MANTISSA_BITS: F64::MANTISSA_BITS,
        MAX: F64::from_bits(0x7F_EF_FF_FF_FF_FF_FF_FF),
        zero: F64::from_i32(0, RoundingMode::TowardZero),
        one: F64::from_i32(1, RoundingMode::TowardZero),
        cmp: |l, r| l.compare(r),
        classify: |v| v.classify(),
        add: |l, r| SoftFloat::add(&l, r, DEFAULT_OPS_ROUNDING),
        sub: |l, r| SoftFloat::sub(&l, r, DEFAULT_OPS_ROUNDING),
        mul: |l, r| SoftFloat::mul(&l, r, DEFAULT_OPS_ROUNDING),
        div: |l, r| SoftFloat::div(&l, r, DEFAULT_OPS_ROUNDING),
        rem: |l, r| SoftFloat::rem(&l, r, DEFAULT_OPS_ROUNDING),
        sqrt: |v| SoftFloat::sqrt(&v, DEFAULT_OPS_ROUNDING),
        round: |v| SoftFloat::round_to_integral(&v, RoundingMode::TiesToAway),
        floor: |v| SoftFloat::round_to_integral(&v, RoundingMode::TowardNegative),
        ceil: |v| SoftFloat::round_to_integral(&v, RoundingMode::TowardPositive),
        from u64: |v| F64::from_u64(v, RoundingMode::TowardZero),
        from BasisPoints: |v| F64::from_u16(v, RoundingMode::TowardZero),
        from UInt: |v|
            {
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                fn usize_as_i32(value: usize) -> i32 {
                    value as i32
                }

                let mut accum = F64::from_i32(0, RoundingMode::TiesToAway);
                // Add up words in inbound value, offsetting each by 2^N*64
                #[allow(clippy::cast_possible_truncation)]
                for (i, word) in v.0.into_iter().enumerate() {
                    accum = accum.add(f64_pow_2(
                        F64::from_u64(word, RoundingMode::TowardZero),
                        64 * usize_as_i32(i),
                    ), RoundingMode::TiesToAway);
                }
                accum
            },
        try_into UInt: |v| {
            use crate::fp::U128;
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            const N_WORDS: i32 = std::mem::size_of::<UInt>() as i32 / 8;

            #[allow(clippy::cast_sign_loss)]
            fn i32_to_usize(v: i32) -> usize {
                v as usize
            }

            if v.is_nan() {
                Err(Error::NaN)
            } else if v.sign() != 0 {
                Err(Error::NegativeToUnsigned)
            } else {
                let (upper_word, words) = f64_to_aligned_words(v);
                // Most-significant bit is above U128, means overflow
                if upper_word >= N_WORDS {
                    Err(Error::Overflow)
                }
                // Most-significant bit is below dot
                else if upper_word < 0 {
                    Ok(U128::zero())
                } else {
                    let mut result = U128::zero();
                    result.0[i32_to_usize(upper_word)] = words[1];
                    if upper_word > 0 {
                        result.0[i32_to_usize(upper_word - 1)] = words[0];
                    }

                    Ok(result)
                }
            }
        },
        integer_decode: |value| {
            let bits: u64 = value.to_bits();
            let sign: i8 = if bits >> 63 == 0 { 1 } else { -1 };
            let mut exponent: i16 = ((bits >> 52) & 0x7ff) as i16;
            let mantissa = if exponent == 0 {
                (bits & 0x000f_ffff_ffff_ffff) << 1
            } else {
                (bits & 0x000f_ffff_ffff_ffff) | 0x0010_0000_0000_0000
            };
            // Exponent bias + mantissa shift
            exponent -= 1023 + 52;
            (mantissa, exponent, sign)
        },
        try_into_lossy FixedPoint: |v| try_f64_to_fixedpoint(v, true),
    }
}

#[cfg(not(target = "wasm32"))]
impl From<f64> for Float {
    fn from(value: f64) -> Self {
        Self(F64::from_native_f64(value))
    }
}

#[cfg(not(target = "wasm32"))]
impl From<Float> for f64 {
    fn from(value: Float) -> Self {
        f64::from_bits(value.0.to_f64(RoundingMode::TowardZero).to_bits())
    }
}

impl From<u128> for Float {
    fn from(v: u128) -> Self {
        /// ```
        /// assert_eq!(((1u128 << 64) as f64).to_bits(), 4_895_412_794_951_729_152_u64);
        /// ```
        const FLOAT_TWO_POW_64: Float = Float::from_bits(4_895_412_794_951_729_152_u64);

        #[allow(clippy::cast_possible_truncation)]
        {
            Float::from((v >> 64) as u64) * FLOAT_TWO_POW_64 + Float::from(v as u64)
        }
    }
}

impl TopEncode for Float {
    fn top_encode<O>(&self, output: O) -> Result<(), multiversx_sc_codec::EncodeError>
    where
        O: multiversx_sc_codec::TopEncodeOutput,
    {
        self.0.to_bits().top_encode(output)
    }
}

impl TopDecode for Float {
    fn top_decode<I>(input: I) -> Result<Self, multiversx_sc_codec::DecodeError>
    where
        I: multiversx_sc_codec::TopDecodeInput,
    {
        <F64 as SoftFloat>::Payload::top_decode(input).map(|bits| Self(F64::from_bits(bits)))
    }
}

impl NestedEncode for Float {
    fn dep_encode<O: multiversx_sc_codec::NestedEncodeOutput>(
        &self,
        dest: &mut O,
    ) -> Result<(), multiversx_sc_codec::EncodeError> {
        self.0.to_bits().dep_encode(dest)
    }
}

impl NestedDecode for Float {
    fn dep_decode<I: multiversx_sc_codec::NestedDecodeInput>(
        input: &mut I,
    ) -> Result<Self, multiversx_sc_codec::DecodeError> {
        <F64 as SoftFloat>::Payload::dep_decode(input).map(|bits| Self(F64::from_bits(bits)))
    }
}

impl TypeAbi for Float {
    // Default implementation uses fully qualified name which js lib can't parse
    fn type_name() -> TypeName {
        "Float".into()
    }
}

/// Raises FP value to power of two by changing binary exponent directly
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_sign_loss)]
fn try_f64_pow_2(mut value: F64, pow: i32) -> Result<F64, Error> {
    if !value.is_normal() {
        return Ok(value);
    }
    let new_exp = (value.exponent() as i64) + i64::from(pow);
    if new_exp >= F64::EXPONENT_MASK as i64 {
        Err(Error::Overflow)
    } else if new_exp <= 0 {
        Err(Error::PrecisionLoss)
    } else {
        value.set_exponent(new_exp as u64);
        Ok(value)
    }
}

fn f64_pow_2(value: F64, pow: i32) -> F64 {
    try_f64_pow_2(value, pow).unwrap()
}

/// Convert F64 to a pair of u64 words, and signed index for upper u64 word
///
/// F64 mantissa is mapped to continuous aligned u64 space,
/// where 0th word is the one right above FP dot,
/// and index of upper word in that mapping is returned.
/// Note that lower word may be 0
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::range_plus_one
)]
fn f64_to_aligned_words(value: F64) -> (i32, [u64; 2]) {
    const WORD_BITS: i32 = u64::BITS as i32;
    // 2^6 == 64, we use `i32 >> 6` instead of `i32 / 64` to get correct word index
    const WORD_IDX_SHIFT: i32 = 6;
    const MANTISSA_BITS: i32 = F64::MANTISSA_BITS as i32;
    const UPPER_LSB_IDX: Range<i32> = (MANTISSA_BITS + 1 - WORD_BITS)..(MANTISSA_BITS + 1);
    // Only zero and normal values are supported ATM
    if value.is_zero() {
        return (0, [0, 0]);
    }
    assert!(
        value.is_normal(),
        "Only normal and zero values are supported, but value is {:?}",
        Float::from(value).classify()
    );
    // Get signed exponent, to decide which words will be populated
    // Exponent is always treated as symmetric
    let exp = (value.exponent() as i32) - ((F64::EXPONENT_MASK / 2) as i32);
    // Upper word index where F64 would land
    let upper_word = exp >> WORD_IDX_SHIFT;
    // Extract mantissa directly and turn into normal number
    // by adding first significant bit
    let mantissa = value.mantissa() + (1u64 << MANTISSA_BITS);
    // Exponent of mantissa 0th bit
    let exp_low = exp - MANTISSA_BITS;
    // Index of upper word's least significant bit
    // Please note that mantissa occupies not the whole payload
    // So upper word's least significant bit may be below 0,
    // i.e. mantissa may be offset left, not right, to make up upper word
    let upper_word_lsb = (upper_word << WORD_IDX_SHIFT) - exp_low;
    assert!(
        UPPER_LSB_IDX.contains(&upper_word_lsb),
        "LSB index: {upper_word_lsb} not in allowed range {UPPER_LSB_IDX:?}"
    );

    let words = [
        // Lower word may contain any data only if upper LSB is > 0,
        // otherwise lower word bits all reside in "imaginary" mantissa part
        // below its 0th bit
        if upper_word_lsb > 0 {
            (mantissa << (WORD_BITS - upper_word_lsb)) & u64::MAX
        } else {
            0
        },
        // If LSB index is above zero, upper word occupies only part of mantissa,
        // otherwise it also spans "imaginary" region below zero
        if upper_word_lsb >= 0 {
            (mantissa >> upper_word_lsb) & u64::MAX
        } else {
            (mantissa << -upper_word_lsb) & u64::MAX
        },
    ];
    (upper_word, words)
}

fn try_f64_to_fixedpoint(v: F64, lossy: bool) -> Result<FixedPoint, Error> {
    #[allow(clippy::cast_sign_loss)]
    fn i32_to_usize(v: i32) -> usize {
        v as usize
    }

    const UPPER_WORD: i32 = 3;
    const LOWER_WORD: i32 = -4;

    let one = F64::from_i32(1, RoundingMode::TowardZero);

    if v.is_nan() {
        Err(Error::NaN)
    } else if v.sign() != 0 {
        Err(Error::NegativeToUnsigned)
    } else {
        let result = v.compare(f64_pow_2(one, 128));
        if result == Some(std::cmp::Ordering::Equal) || result == Some(std::cmp::Ordering::Greater)
        {
            Err(Error::Overflow)
        } else {
            // Convert to fixed-point aligned space
            let (upper_word, words) = f64_to_aligned_words(v);
            // Bits leak above U256
            if upper_word > UPPER_WORD {
                Err(Error::Overflow)
            }
            // Bits are way below 1/U256 bits
            // Edge case, upper bits are in range but lower ones are below 1/U256
            else if upper_word < LOWER_WORD || (upper_word == LOWER_WORD && words[0] != 0) {
                #[cfg(test)]
                eprintln!(
                    "float: {}\nupper_word: {}\nwords[0]:   0b_{:0b}\nwords[1]:   0b_{:0b}",
                    tests::debug_f64(v),
                    upper_word,
                    words[0],
                    words[1]
                );
                if lossy {
                    let mut result = crate::fp::U256::zero();
                    result.0[0] = words[1];
                    Ok(result.0.into())
                } else {
                    Err(Error::PrecisionLoss)
                }
            }
            // In-range or corner case of lower bits leakage
            else {
                let mut result = crate::fp::U256::zero();
                // Re-align to U256 boundary
                let upper_word = upper_word - LOWER_WORD;
                assert!(upper_word >= 0);
                result.0[i32_to_usize(upper_word)] = words[1];
                if upper_word > 0 {
                    result.0[i32_to_usize(upper_word - 1)] = words[0];
                }

                Ok(result.0.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rand::Rng;
    use softfloat_wrapper::{RoundingMode, SoftFloat};

    #[allow(clippy::cast_possible_truncation)]
    pub fn debug_f64(value: F64) -> String {
        format!(
            "{} m: 0x_{:X} exp: {}",
            if value.is_negative() { "-" } else { "+" },
            value.mantissa() + (1u64 << F64::MANTISSA_BITS),
            (value.exponent() as i32) - ((F64::EXPONENT_MASK / 2) as i32),
        )
    }

    #[test]
    fn test_f64_pow_2() {
        // NaN, infinity and 0 to any power aren't changed
        for _ in 0..10 {
            let pow = rand::thread_rng().gen_range(0..64);
            // NaN
            assert!(f64_pow_2(F64::quiet_nan(), pow).is_nan());
            assert!(f64_pow_2(F64::quiet_nan(), -pow).is_nan());
            assert!(f64_pow_2(F64::quiet_nan().neg(), pow).is_nan());
            assert!(f64_pow_2(F64::quiet_nan().neg(), -pow).is_nan());
            // 0
            assert!(f64_pow_2(F64::zero(), pow).eq(F64::zero()));
            assert!(f64_pow_2(F64::zero(), -pow).eq(F64::zero()));
            assert!(f64_pow_2(F64::zero().neg(), pow).eq(F64::zero().neg()));
            assert!(f64_pow_2(F64::zero().neg(), -pow).eq(F64::zero().neg()));
            // Infinity
            assert!(f64_pow_2(F64::infinity(), pow).eq(F64::infinity()));
            assert!(f64_pow_2(F64::infinity(), -pow).eq(F64::infinity()));
            assert!(f64_pow_2(F64::infinity().neg(), pow).eq(F64::infinity().neg()));
            assert!(f64_pow_2(F64::infinity().neg(), -pow).eq(F64::infinity().neg()));
        }
        // Simple positive cases
        for _ in 0..10 {
            let pow = rand::thread_rng().gen_range(0..32);
            let pow_mult_f64 = F64::from_u64(1 << pow, RoundingMode::TiesToAway);

            for _ in 0..10 {
                let num = rand::thread_rng().gen_range(0..0xFF_FF);
                let num_f64 = F64::from_u64(num, RoundingMode::TiesToAway);

                assert!(
                    f64_pow_2(num_f64, pow).eq(F64::from_u64(num, RoundingMode::TiesToAway)
                        .mul(pow_mult_f64, RoundingMode::TiesToAway))
                );
                assert!(
                    f64_pow_2(num_f64, -pow).eq(F64::from_u64(num, RoundingMode::TiesToAway)
                        .div(pow_mult_f64, RoundingMode::TiesToAway))
                );
                assert!(f64_pow_2(num_f64.neg(), pow).eq(F64::from_u64(
                    num,
                    RoundingMode::TiesToAway
                )
                .neg()
                .mul(pow_mult_f64, RoundingMode::TiesToAway)));
                assert!(f64_pow_2(num_f64.neg(), -pow).eq(F64::from_u64(
                    num,
                    RoundingMode::TiesToAway
                )
                .neg()
                .div(pow_mult_f64, RoundingMode::TiesToAway)));
            }
        }
        // Overflow checks
        let number = {
            let mut n = F64::from_i32(1, RoundingMode::TowardZero);
            n.set_exponent(F64::EXPONENT_MASK - 2);
            n
        };

        assert_matches!(try_f64_pow_2(number, 1), Ok(_));
        assert_matches!(try_f64_pow_2(number, 2), Err(Error::Overflow));

        // Underflow
        let number = {
            let mut n = F64::from_i32(1, RoundingMode::TowardZero);
            n.set_exponent(2);
            n
        };

        assert_matches!(try_f64_pow_2(number, -1), Ok(_));
        assert_matches!(try_f64_pow_2(number, -2), Err(Error::PrecisionLoss));
    }

    #[test]
    #[allow(
        clippy::cast_lossless,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap
    )]
    fn test_f64_to_aligned_words() {
        use softfloat_wrapper::SoftFloat;
        // break u64 into 2 aligned words, with specified offset
        fn as_2_words(value: u64, exp: i32, msb: i32) -> [u64; 2] {
            if value == 0 {
                return [0, 0];
            }
            let value = (value as u128) << (64 + ((exp + msb) & 63) - msb);
            [
                (value & (u64::MAX as u128)) as u64,
                ((value >> u64::BITS) & (u64::MAX as u128)) as u64,
            ]
        }
        // Sample numbers converted to F64, then shifted by exponent and broken into 2 aligned words
        let samples = [
            0u64, // zero sould work too
            1u64, // simplest case
            0b_0000_0000_0001_1011_1001_1100_0111_1000_0111_1100_0001_1111_1000_0001_1111_1100_u64,
        ];

        for sample in samples {
            let float = F64::from_u64(sample, RoundingMode::TowardZero);
            let msb = if sample == 0 {
                0
            } else {
                (u64::BITS - sample.leading_zeros() - 1) as i32
            };
            for exp in -512..512 {
                let float = f64_pow_2(float, exp);
                let exp_words = as_2_words(sample, exp, msb);
                let exp_upper_word = if sample == 0 { 0 } else { (exp + msb) >> 6 };

                let details = || {
                    format!(
                        "\nSample: 0b_{:b}\nMSB: {}\nOffset: {}\nFloat: {}",
                        sample,
                        msb,
                        exp,
                        debug_f64(float),
                    )
                };

                let (upper_word, words) = f64_to_aligned_words(float);
                assert_eq!(
                    exp_upper_word,
                    upper_word,
                    "upper_word mismatch{}",
                    details()
                );
                assert_eq!(exp_words[0], words[0], "words[0] mismatch{}", details());
                assert_eq!(exp_words[1], words[1], "words[1] mismatch{}", details());
            }
        }
    }
}
