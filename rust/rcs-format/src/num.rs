//! Python-compatible rounding.
//!
//! Byte parity with the Python reference needs the same rules: `round(x, 3)` and
//! `round(x)` in Python round **half to even** (banker's rounding), while
//! `f64::round` in Rust rounds away from zero. The difference shows up at exact
//! halves (`0.0005`, `0.5`, `2.5`) and shifts the quantisation by one unit, i.e.
//! the file bytes.
//!
//! `round_digits` uses Rust formatting: like `dtoa` in CPython it produces a
//! **correctly rounded** decimal representation with half-to-even rounding.

/// Round to the nearest integer, halves to even (like `round(x)` in Python).
pub fn round_half_even(value: f64) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let floor = value.floor();
    let diff = value - floor;
    if diff > 0.5 {
        floor + 1.0
    } else if diff < 0.5 {
        floor
    } else if floor % 2.0 == 0.0 {
        floor
    } else {
        floor + 1.0
    }
}

/// Round to `digits` decimal places (like `round(x, digits)` in Python).
pub fn round_digits(value: f64, digits: usize) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let text = format!("{value:.*}", digits);
    text.parse().unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_even_matches_python() {
        // Values where ordinary "away from zero" rounding gives another result.
        assert_eq!(round_half_even(0.5), 0.0);
        assert_eq!(round_half_even(1.5), 2.0);
        assert_eq!(round_half_even(2.5), 2.0);
        assert_eq!(round_half_even(-0.5), -0.0);
        assert_eq!(round_half_even(-1.5), -2.0);
        assert_eq!(round_half_even(-2.5), -2.0);
        assert_eq!(round_half_even(2.4), 2.0);
        assert_eq!(round_half_even(2.6), 3.0);
    }

    #[test]
    fn digits_match_python() {
        assert_eq!(round_digits(0.9444444, 3), 0.944);
        assert_eq!(round_digits(-1.111111111111111, 3), -1.111);
        assert_eq!(round_digits(2.5, 0), 2.0);
        assert_eq!(round_digits(0.0005, 3), 0.001);
    }
}
