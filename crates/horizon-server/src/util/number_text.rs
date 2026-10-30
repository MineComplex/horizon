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

#[cfg(test)]
mod tests {
    use super::*;

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
