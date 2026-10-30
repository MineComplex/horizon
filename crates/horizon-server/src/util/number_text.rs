//! The text Minecraft shows for a `double` or `float`: Java's `Double.toString`/`Float.toString` rules
//! (`1.0`, `0.001`, `1.0E7`, `1.0E-4`, `NaN`, `-Infinity`). Commands' error messages, translated text
//! arguments and SNBT print numbers this way, so players and clients see the same text as on Leaf.
//!
//! Rust's `{}`/`{:e}` already give the shortest digits that round-trip, which is what Java 19+ prints
//! (JDK-4511638); only the layout differs (`1` vs `1.0`, `1e7` vs `1.0E7`). Taken from Horizon's
//! `horizon-client-core/src/snbt.rs`.

/// `Double.toString`.
pub fn double_to_string(v: f64) -> String {
    layout(v, &format!("{v}"), &format!("{v:e}"))
}

/// `Float.toString`.
pub fn float_to_string(v: f32) -> String {
    layout(f64::from(v), &format!("{v}"), &format!("{v:e}"))
}

fn layout(v: f64, plain: &str, sci: &str) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    if (1.0e-3..1.0e7).contains(&v.abs()) {
        if plain.contains('.') {
            plain.to_owned()
        } else {
            format!("{plain}.0")
        }
    } else {
        let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci, "0"));
        if mantissa.contains('.') {
            format!("{mantissa}E{exponent}")
        } else {
            format!("{mantissa}.0E{exponent}")
        }
    }
}

/// `String.format(Locale.ROOT, "%.<precision>f", v)`: Java rounds the shortest round-trip digits of
/// `v` half-up (Rust rounds the exact binary value half-to-even: `0.125` is `0.13` in Java, `0.12` in
/// Rust). A `float` argument is widened to `double` first: pass `f64::from(x)`.
pub fn format_fixed(v: f64, precision: usize) -> String {
    if !v.is_finite() {
        return double_to_string(v);
    }
    let sign = if v.is_sign_negative() { "-" } else { "" };
    let sci = format!("{:e}", v.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let exponent: i64 = exponent.parse().unwrap_or(0);
    // Digits kept: those before the point plus `precision` after it.
    let kept = exponent + 1 + precision as i64;
    let mut scaled: Vec<u8> = if kept < 0 || v == 0.0 {
        Vec::new()
    } else if kept as usize >= digits.len() {
        digits.resize(kept as usize, b'0');
        digits
    } else {
        let round_up = digits[kept as usize] >= b'5';
        digits.truncate(kept as usize);
        if round_up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, b'1');
                    break;
                }
                i -= 1;
                if digits[i] == b'9' {
                    digits[i] = b'0';
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
        digits
    };
    if scaled.len() <= precision {
        let mut padded = vec![b'0'; precision + 1 - scaled.len()];
        padded.append(&mut scaled);
        scaled = padded;
    }
    let (int_part, frac_part) = scaled.split_at(scaled.len() - precision);
    let int_part = String::from_utf8_lossy(int_part);
    let frac_part = String::from_utf8_lossy(frac_part);
    if precision == 0 {
        format!("{sign}{int_part}")
    } else {
        format!("{sign}{int_part}.{frac_part}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `String.format(Locale.ROOT, "%.2f"/"%.1f", x)` on Java 25.
    #[test]
    fn format_fixed_matches_java() {
        let cases: [(f64, &str, &str); 15] = [
            (0.125, "0.13", "0.1"),
            (0.005, "0.01", "0.0"),
            (0.015, "0.02", "0.0"),
            (1.005, "1.01", "1.0"),
            (2.675, "2.68", "2.7"),
            (0.0049, "0.00", "0.0"),
            (999.995, "1000.00", "1000.0"),
            (-0.001, "-0.00", "-0.0"),
            (12.345, "12.35", "12.3"),
            (0.0, "0.00", "0.0"),
            (50.0, "50.00", "50.0"),
            (1e-9, "0.00", "0.0"),
            (123456.785, "123456.79", "123456.8"),
            (0.95, "0.95", "1.0"),
            (9.96, "9.96", "10.0"),
        ];
        for (v, two, one) in cases {
            assert_eq!(format_fixed(v, 2), two, "{v}");
            assert_eq!(format_fixed(v, 1), one, "{v}");
        }
        let floats: [(f32, &str, &str); 6] = [
            (0.05, "0.1", "0.05"),
            (2.25, "2.3", "2.25"),
            (0.15, "0.2", "0.15"),
            (33.35, "33.3", "33.35"),
            (20.0, "20.0", "20.00"),
            (1.45, "1.5", "1.45"),
        ];
        for (v, one, two) in floats {
            assert_eq!(format_fixed(f64::from(v), 1), one, "{v}");
            assert_eq!(format_fixed(f64::from(v), 2), two, "{v}");
        }
    }

    /// Values printed by `Double.toString`/`Float.toString` on Java 25.
    #[test]
    fn matches_java() {
        assert_eq!(double_to_string(1.0), "1.0");
        assert_eq!(double_to_string(-0.0), "-0.0");
        assert_eq!(double_to_string(0.5), "0.5");
        assert_eq!(double_to_string(0.001), "0.001");
        assert_eq!(double_to_string(0.0001), "1.0E-4");
        assert_eq!(double_to_string(1234567.0), "1234567.0");
        assert_eq!(double_to_string(1.0e7), "1.0E7");
        assert_eq!(double_to_string(1.2345e20), "1.2345E20");
        assert_eq!(double_to_string(-3.0e10), "-3.0E10");
        assert_eq!(double_to_string(f64::MAX), "1.7976931348623157E308");
        assert_eq!(double_to_string(f64::NAN), "NaN");
        assert_eq!(double_to_string(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(float_to_string(0.1), "0.1");
        assert_eq!(float_to_string(3.0e-5), "3.0E-5");
        assert_eq!(float_to_string(f32::MAX), "3.4028235E38");
        assert_eq!(float_to_string(-1.5), "-1.5");
    }
}
