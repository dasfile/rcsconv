//! Округление, совместимое с Python.
//!
//! Паритет с Python-эталоном требует тех же правил: `round(x, 3)` и `round(x)`
//! в Python округляют **к чётному** (banker's rounding), а `f64::round` в Rust —
//! от нуля. Разница проявляется на ровно половине (`0.0005`, `0.5`, `2.5`) и
//! сдвигает квантование на единицу, то есть байты файла.
//!
//! `round_digits` использует форматирование Rust: оно, как и `dtoa` в CPython,
//! даёт **правильно округлённое** десятичное представление с округлением к чётному.

/// Округление к ближайшему целому, половина — к чётному (как `round(x)` в Python).
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

/// Округление до `digits` знаков после запятой (как `round(x, digits)` в Python).
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
        // Значения, где обычное округление «от нуля» даёт другой результат.
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
