//! JavaScript number semantics used by ported paint.js code (spec §6.2).

/// `Math.round(x)`: nearest integer, ties toward +∞; keeps JS's `-0` for x in [-0.5, 0).
pub fn math_round(x: f64) -> f64 {
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    if r == 0.0 && x < 0.0 { -0.0 } else { r }
}

/// `String(x)` for the values the UI prints (integers and short decimals): Rust's shortest
/// round-trip form agrees with JS's below 1e21; `-0` prints as "0".
pub fn number_to_string(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    format!("{x}")
}

/// ECMAScript ToInt32 (`x | 0`).
pub fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4294967296.0) as u32) as i32
}

/// ECMAScript ToUint32 (`x >>> 0`).
pub fn to_uint32(x: f64) -> u32 {
    to_int32(x) as u32
}

/// ECMAScript ToUint8 (store into `Uint8Array`).
pub fn to_uint8(x: f64) -> u8 {
    to_int32(x) as u8
}

/// ECMAScript ToUint8Clamp (store into `Uint8ClampedArray`): clamp, then round half to even.
pub fn clamped_u8(x: f64) -> u8 {
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    if x >= 255.0 {
        return 255;
    }
    let f = x.floor();
    // Above the midpoint rounds up; at the tie, to the even neighbour.
    let r = if f + 0.5 < x || (x == f + 0.5 && f % 2.0 != 0.0) {
        f + 1.0
    } else {
        f
    };
    r as u8
}

/// `Number(string)`: trimmed; empty is 0; decimal literals, `Infinity` and `0x`/`0o`/`0b`
/// integers; anything else NaN.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    let radix = match t.get(..2) {
        Some("0x" | "0X") => 16,
        Some("0o" | "0O") => 8,
        Some("0b" | "0B") => 2,
        _ => 10,
    };
    if radix != 10 {
        return u64::from_str_radix(&t[2..], radix).map_or(f64::NAN, |v| v as f64);
    }
    let unsigned = t.strip_prefix(['+', '-']).unwrap_or(t);
    if unsigned == "Infinity" {
        return if t.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    // Rust also reads "inf"/"nan"; JS only digits, '.', exponents and signs.
    if !unsigned
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_to_number_matches_js() {
        assert_eq!(string_to_number(" 12 "), 12.0);
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number(".5"), 0.5);
        assert_eq!(string_to_number("5."), 5.0);
        assert_eq!(string_to_number("-1e2"), -100.0);
        assert_eq!(string_to_number("0x10"), 16.0);
        assert_eq!(string_to_number("-Infinity"), f64::NEG_INFINITY);
        for nan in ["abc", "5px", "inf", "nan", "1e", ".", "+-5", "0x"] {
            assert!(string_to_number(nan).is_nan(), "{nan}");
        }
    }

    #[test]
    fn number_to_string_matches_js() {
        assert_eq!(number_to_string(75.0), "75");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(0.5), "0.5");
    }

    #[test]
    fn math_round_matches_js() {
        // Values checked in Chromium: Math.round(x)
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(1.4999999999999998), 1.0);
        assert_eq!(math_round(0.49999999999999994), 0.0);
        assert!(math_round(-0.4).is_sign_negative() && math_round(-0.4) == 0.0); // JS: -0
        assert_eq!(math_round(-0.5), 0.0);
        assert!(math_round(f64::NAN).is_nan());
        assert_eq!(math_round(f64::INFINITY), f64::INFINITY);
        assert_eq!(math_round(4503599627370497.0), 4503599627370497.0);
    }

    #[test]
    fn to_int32_matches_js() {
        // x | 0
        assert_eq!(to_int32(2147483648.0), -2147483648);
        assert_eq!(to_int32(4294967296.0), 0);
        assert_eq!(to_int32(-1.9), -1);
        assert_eq!(to_int32(1.9), 1);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(f64::INFINITY), 0);
        assert_eq!(to_int32(-4294967297.0), -1);
    }

    #[test]
    fn to_uint32_matches_js() {
        // x >>> 0
        assert_eq!(to_uint32(-1.0), 4294967295);
        assert_eq!(to_uint32(4294967296.5), 0);
    }

    #[test]
    fn to_uint8_matches_uint8array_store() {
        // new Uint8Array([x])[0]
        assert_eq!(to_uint8(256.0), 0);
        assert_eq!(to_uint8(-1.0), 255);
        assert_eq!(to_uint8(257.9), 1);
        assert_eq!(to_uint8(f64::NAN), 0);
    }

    #[test]
    fn clamped_u8_matches_uint8clampedarray_store() {
        // new Uint8ClampedArray([x])[0] — round half to even
        assert_eq!(clamped_u8(0.5), 0);
        assert_eq!(clamped_u8(1.5), 2);
        assert_eq!(clamped_u8(2.5), 2);
        assert_eq!(clamped_u8(254.5), 254);
        assert_eq!(clamped_u8(253.5), 254);
        assert_eq!(clamped_u8(2.5000001), 3);
        assert_eq!(clamped_u8(-1.0), 0);
        assert_eq!(clamped_u8(300.0), 255);
        assert_eq!(clamped_u8(f64::NAN), 0);
    }
}
