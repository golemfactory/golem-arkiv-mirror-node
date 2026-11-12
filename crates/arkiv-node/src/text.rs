//! Bytes and numbers as the JSON APIs read and write them.

use alloy_primitives::U256;

/// Lowercase `0x` hex.
pub fn hex(bytes: &[u8]) -> String {
    alloy_primitives::hex::encode_prefixed(bytes)
}

/// The at most `cap` bytes of lowercase `0x` hex.
pub fn unhex(text: &str, cap: usize) -> Option<Vec<u8>> {
    let digits = text.strip_prefix("0x")?;
    if digits.len() % 2 != 0 || digits.len() / 2 > cap {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    digits
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Some(nibble(pair[0])? * 16 + nibble(pair[1])?))
        .collect()
}

/// A decimal `u64` without sign or leading zero.
pub fn decimal(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && (text.len() == 1 || !text.starts_with('0'))
        && text.bytes().all(|b| b.is_ascii_digit());
    canonical.then(|| text.parse().ok())?
}

/// The digits of a JSON-RPC quantity: `0x`, then hex digits with no leading
/// zero (`0x0` is zero).
fn quantity_digits(s: &str) -> Option<&str> {
    let digits = s.strip_prefix("0x")?;
    let minimal = digits == "0" || !digits.starts_with('0');
    (!digits.is_empty() && minimal && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(digits)
}

/// A JSON-RPC quantity that fits `u64`.
pub fn quantity(s: &str) -> Option<u64> {
    u64::from_str_radix(quantity_digits(s)?, 16).ok()
}

/// A JSON-RPC quantity that fits `U256`.
pub fn quantity_u256(s: &str) -> Option<U256> {
    U256::from_str_radix(quantity_digits(s)?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lowercase_prefixed_and_bounded() {
        assert_eq!(hex(&[]), "0x");
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "0x00abff");
        assert_eq!(unhex("0x00abff", 3), Some(vec![0x00, 0xab, 0xff]));
        assert_eq!(unhex("0x", 0), Some(vec![]));
        for refused in ["00ab", "0xAB", "0xa", "0xzz", "0x00abff00"] {
            assert_eq!(unhex(refused, 3), None, "{refused}");
        }
    }

    #[test]
    fn decimals_are_canonical() {
        assert_eq!(decimal("0"), Some(0));
        assert_eq!(decimal("18446744073709551615"), Some(u64::MAX));
        for refused in ["", "01", "+1", "-1", "1 ", "18446744073709551616"] {
            assert_eq!(decimal(refused), None, "{refused}");
        }
    }

    #[test]
    fn quantities_are_minimal_prefixed_hex() {
        assert_eq!(quantity("0x0"), Some(0));
        assert_eq!(quantity("0x1f"), Some(31));
        assert_eq!(quantity("0xFFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(
            quantity_u256("0x10000000000000000"),
            Some(U256::from(1) << 64)
        );
        for refused in [
            "0x", "0x+1", "0x-1", "0x00", "0x01", "1", "0X1", "0x1g", " 0x1",
        ] {
            assert_eq!(quantity(refused), None, "{refused}");
            assert_eq!(quantity_u256(refused), None, "{refused}");
        }
        assert_eq!(quantity("0x10000000000000000"), None);
    }
}
