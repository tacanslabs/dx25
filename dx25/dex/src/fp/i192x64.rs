use super::signed::{self, Signed};
use super::u192x64::U192X64;
use super::Error;
use crate::chain::Float;

pub type I192X64 = Signed<U192X64>;

impl TryFrom<Float> for I192X64 {
    type Error = Error;
    fn try_from(value: Float) -> Result<Self, Self::Error> {
        signed::try_from_float::<U192X64, 4, 1>(value)
    }
}

impl From<I192X64> for Float {
    fn from(v: I192X64) -> Self {
        signed::into_float::<U192X64, 4, 1>(v)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fp::U192X64;
    use crate::fp::U256;
    use float_extras::f64::ldexp;

    #[test]
    fn test_sum() {
        let u_one = U192X64(U256::one());
        let u_two = U192X64(U256::one() * 2);
        let one = I192X64::from(u_one);
        let two = I192X64::from(u_two);
        assert_eq!(one + one, two);

        let neg_one = I192X64::neg_from(one);
        let neg_two = I192X64::neg_from(two);
        assert_eq!(one + neg_two, neg_one);
        assert_eq!(neg_two + one, neg_one);

        let u_three = U192X64(U256::one() * 3);
        let three = I192X64::from(u_three);
        let neg_three = I192X64::neg_from(three);
        assert_eq!(neg_one + neg_two, neg_three);
    }

    #[test]
    fn test_sub() {
        let u_one = U192X64(U256::one());
        let u_two = U192X64(U256::one() * 2);
        let one = I192X64::from(u_one);
        let two = I192X64::from(u_two);
        let neg_one = I192X64::neg_from(one);
        assert_eq!(two - one, one);
        assert_eq!(one - two, neg_one);

        let neg_two = I192X64::neg_from(two);
        let u_three = U192X64(U256::one() * 3);
        let three = I192X64::from(u_three);
        assert_eq!(one - neg_two, three);

        let neg_three = I192X64::neg_from(three);
        assert_eq!(neg_two - one, neg_three);

        assert_eq!(neg_two - neg_one, neg_one);
    }

    #[test]
    fn test_mul() {
        let u_one = U192X64(U256::one() << 64);
        let u_two = U192X64((U256::one() << 64) * 2);
        let one = I192X64::from(u_one);
        let two = I192X64::from(u_two);
        assert_eq!(two * one, two);
    }

    #[test]
    fn test_mul_large() {
        assert_eq!(
            I192X64::from(U192X64::from(1u128 << 100)) * I192X64::from(U192X64::from(1u128 << 26)),
            I192X64::from(U192X64::from(1u128 << 126))
        );
    }

    #[test]
    fn test_div() {
        let u_one = U192X64(U256::one() << 64);
        let u_two = U192X64((U256::one() << 64) * 2);
        let one = I192X64::from(u_one);
        let two = I192X64::from(u_two);
        assert_eq!(two / one, two);

        let neg_one = I192X64::neg_from(one);
        let neg_two = I192X64::neg_from(two);
        assert_eq!(neg_two / neg_one, two);
        assert_eq!(neg_two / one, neg_two);
        assert_eq!(two / neg_one, neg_two);
    }

    #[test]
    fn test_try_f64_to_i192x64_large() {
        assert_eq!(
            I192X64::try_from(Float::from(ldexp(-1_f64, 127))).unwrap(),
            I192X64::from(U192X64::from(1u128 << 127)).neg_from()
        );

        assert_eq!(
            I192X64::try_from(Float::from(ldexp(f64::from(-0b_1111_1111_1111), 128 - 12))).unwrap(),
            I192X64::from(U192X64::from(0b_1111_1111_1111_u128 << (128 - 12))).neg_from()
        );
    }

    #[test]
    fn test_try_f64_to_i192x64_tiny() {
        assert_eq!(
            I192X64::try_from(Float::from(10f64)).unwrap(),
            I192X64::from(U192X64::from(10))
        );

        assert_eq!(
            I192X64::try_from(Float::from(ldexp(-287_f64, -64))).unwrap(),
            I192X64::from(U192X64::from([287_u64, 0_u64, 0_u64, 0_u64])).neg_from()
        );

        assert_eq!(
            I192X64::try_from(Float::from(ldexp(-113_f64, 0))).unwrap(),
            I192X64::from(U192X64::from([0_u64, 113_u64, 0_u64, 0_u64])).neg_from()
        );
    }

    fn assert_eq_errors(e1: &Error, e2: &Error) {
        assert_eq!(format!("{e1:?}"), format!("{e2:?}"));
    }

    #[test]
    fn test_try_f64_to_i192x64_overflow() {
        assert_eq_errors(
            &I192X64::try_from(Float::from(ldexp(1_f64, 192))).unwrap_err(),
            &Error::Overflow,
        );
    }

    #[test]
    fn test_try_f64_to_i192x64_prec_loss() {
        assert_eq_errors(
            &I192X64::try_from(Float::from(ldexp(1_f64, -65))).unwrap_err(),
            &Error::PrecisionLoss,
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_i192x64_to_f64() {
        assert_eq!(
            Float::from(I192X64::from(U192X64::from(0)) / I192X64::from(U192X64::from(1))),
            Float::from(0.)
        );
        assert_eq!(
            Float::from(
                I192X64::from(U192X64::from(217_387)) / I192X64::from(U192X64::from(1_000_000))
            ),
            Float::from(0.217_387)
        );
        assert_eq!(
            Float::from(I192X64::from(U192X64::from(71356)) / I192X64::from(U192X64::from(100))),
            Float::from(713.56)
        );
        assert_eq!(
            Float::from(
                I192X64::from(U192X64::from(211_387_616)) / I192X64::from(U192X64::from(1000))
            ),
            Float::from(211_387.616)
        );
        assert_eq!(
            Float::from(
                I192X64::from(U192X64::from(372_792_773)).neg_from()
                    / I192X64::from(U192X64::from(1))
            ),
            Float::from(-372_792_773.)
        );
    }
}
