use num_bigint::BigInt;
use num_rational::BigRational;

// ---------------------------------------------------------------------------------------------- //

pub(super) fn parse_rational(source: &str) -> Option<BigRational> {
    let (negative, unsigned) = match source.as_bytes().first() {
        Some(b'-') => (true, &source[1..]),
        Some(b'+') => (false, &source[1..]),
        _ => (false, source),
    };

    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(position) => (&unsigned[..position], parse_signed_decimal(&unsigned[position + 1..])?),
        None => (unsigned, 0),
    };

    let (whole, fractional) = match mantissa.find('.') {
        Some(position) => (&mantissa[..position], &mantissa[position + 1..]),
        None => (mantissa, ""),
    };
    let digits = format!("{whole}{fractional}");
    let mut significand = BigInt::parse_bytes(digits.as_bytes(), 10)?;
    if negative {
        significand = -significand;
    }

    let decimal_scale = i64::try_from(fractional.len()).ok()?;
    let power = exponent.checked_sub(decimal_scale)?;
    let magnitude = u32::try_from(power.unsigned_abs()).ok()?;
    let factor = BigInt::from(10_u8).pow(magnitude);

    if power >= 0 {
        Some(BigRational::from_integer(significand * factor))
    } else {
        Some(BigRational::new(significand, factor))
    }
}

fn parse_signed_decimal(source: &str) -> Option<i64> {
    let (negative, digits) = match source.as_bytes().first() {
        Some(b'-') => (true, &source[1..]),
        Some(b'+') => (false, &source[1..]),
        _ => (false, source),
    };
    let mut value = 0_i64;
    for byte in digits.bytes() {
        let digit = i64::from(byte.checked_sub(b'0')?);
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(digit)?;
    }

    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::*;

    #[test]
    fn parses_decimal_and_exponent_spelling_exactly() {
        assert_eq!(
            parse_rational("-12.50e-1"),
            Some(BigRational::new(BigInt::from(-5), BigInt::from(4)))
        );
        assert_eq!(parse_rational("+3e2"), Some(BigRational::from_integer(BigInt::from(300))));
    }

    #[test]
    fn normalizes_negative_zero() {
        assert_eq!(parse_rational("-0.0"), Some(BigRational::from_integer(0.into())));
    }
}
