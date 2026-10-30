//! The bits of `java.lang` Brigadier relies on, written so its port behaves as the Java does: strings
//! indexed in UTF-16 code units (`String.length()`, `substring`, `charAt`), `String.hashCode`,
//! `compareTo`/`CASE_INSENSITIVE_ORDER`, `Character.isWhitespace`, `Double.toString`/`Float.toString`,
//! and boxed values as `java.lang.Object` (`JObjectRef`) for the exception types' `Object` arguments.
//!
//! PORT: the boxing impls (`jobject!(String)`, `jobject!(i32)`...) and `string_value_of` belong to
//! java.lang's port; move them there when it exists (they are here so Brigadier's `Object` arguments work).
use crate::prelude::*;

crate::jobject!(String);
crate::jobject!(i32);
crate::jobject!(i64);
crate::jobject!(f32);
crate::jobject!(f64);
crate::jobject!(bool);
crate::jobject!(char);

/// Boxes a value as `java.lang.Object` (Java's autoboxing: `Integer.valueOf(i)`, a `String` as `Object`).
pub fn boxed<T: JObject>(value: T) -> JObjectRef {
    Arc::new(value)
}

/// `String.valueOf(Object)` for the boxed values above and Brigadier's messages; other objects print
/// their class name (Java: `getClass().getName() + "@" + hash`).
pub fn string_value_of(o: &JObjectRef) -> String {
    let any = o.as_any();
    if let Some(s) = any.downcast_ref::<String>() {
        return s.clone();
    }
    if let Some(v) = any.downcast_ref::<i32>() {
        return v.to_string();
    }
    if let Some(v) = any.downcast_ref::<i64>() {
        return v.to_string();
    }
    if let Some(v) = any.downcast_ref::<f32>() {
        return float_to_string(*v);
    }
    if let Some(v) = any.downcast_ref::<f64>() {
        return double_to_string(*v);
    }
    if let Some(v) = any.downcast_ref::<bool>() {
        return v.to_string();
    }
    if let Some(v) = any.downcast_ref::<char>() {
        return v.to_string();
    }
    if let Some(m) = any.downcast_ref::<crate::com::mojang::brigadier::literal_message::LiteralMessage>() {
        return m.to_string();
    }
    o.class_name().to_string()
}

// ---- UTF-16 string indexing (Java's String) ----

/// `s.length()`: UTF-16 code units.
pub fn utf16_len(s: &str) -> i32 {
    s.encode_utf16().count() as i32
}

/// `s.substring(start, end)` in UTF-16 code units (panics out of range, as Java throws).
pub fn substring(s: &str, start: i32, end: i32) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if start < 0 || end > units.len() as i32 || start > end {
        panic!("begin {start}, end {end}, length {}", units.len());
    }
    String::from_utf16_lossy(&units[start as usize..end as usize])
}

/// `s.substring(start)`.
pub fn substring_from(s: &str, start: i32) -> String {
    let len = utf16_len(s);
    substring(s, start, len)
}

/// A UTF-16 code unit as Java's `char` (lone surrogates become U+FFFD).
pub fn unit_to_char(u: u16) -> char {
    char::from_u32(u as u32).unwrap_or('\u{FFFD}')
}

/// `String.hashCode()`.
pub fn string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}

/// `a.compareTo(b)` (lexicographic on UTF-16 code units).
pub fn compare_strings(a: &str, b: &str) -> i32 {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let lim = a.len().min(b.len());
    for k in 0..lim {
        if a[k] != b[k] {
            return a[k] as i32 - b[k] as i32;
        }
    }
    a.len() as i32 - b.len() as i32
}

fn upper_unit(u: u16) -> u16 {
    let c = unit_to_char(u);
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(x), None) if (x as u32) <= 0xFFFF => x as u32 as u16,
        _ => u,
    }
}

fn lower_unit(u: u16) -> u16 {
    let c = unit_to_char(u);
    let mut low = c.to_lowercase();
    match (low.next(), low.next()) {
        (Some(x), None) if (x as u32) <= 0xFFFF => x as u32 as u16,
        _ => u,
    }
}

/// `String.CASE_INSENSITIVE_ORDER.compare(a, b)`.
pub fn compare_strings_ignore_case(a: &str, b: &str) -> i32 {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let lim = a.len().min(b.len());
    for k in 0..lim {
        let mut c1 = a[k];
        let mut c2 = b[k];
        if c1 != c2 {
            c1 = upper_unit(c1);
            c2 = upper_unit(c2);
            if c1 != c2 {
                c1 = lower_unit(c1);
                c2 = lower_unit(c2);
                if c1 != c2 {
                    return c1 as i32 - c2 as i32;
                }
            }
        }
    }
    a.len() as i32 - b.len() as i32
}

/// `s.toLowerCase(Locale.ROOT)`.
pub fn to_lower_case_root(s: &str) -> String {
    s.to_lowercase()
}

/// `Character.isWhitespace(c)`.
pub fn is_whitespace(c: char) -> bool {
    match c {
        '\u{001C}'..='\u{001F}' => true,
        '\u{0085}' | '\u{00A0}' | '\u{2007}' | '\u{202F}' => false,
        _ => c.is_whitespace(),
    }
}

/// Java's `Integer.compare(a, b)`.
pub fn compare_ints(a: i32, b: i32) -> i32 {
    if a < b {
        -1
    } else if a == b {
        0
    } else {
        1
    }
}

// ---- Double.toString / Float.toString ----

fn java_fp_string(sci: String, negative: bool) -> String {
    // `sci` is Rust's shortest round-trip form in scientific notation: "d[.ddd]e[-]n".
    let (mantissa, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..7).contains(&exp) {
        if exp >= 0 {
            let int_len = (exp + 1) as usize;
            if digits.len() <= int_len {
                out.push_str(digits);
                for _ in digits.len()..int_len {
                    out.push('0');
                }
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            for _ in 0..(-exp - 1) {
                out.push('0');
            }
            out.push_str(digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        if digits.len() > 1 {
            out.push_str(&digits[1..]);
        } else {
            out.push('0');
        }
        out.push('E');
        out.push_str(&exp.to_string());
    }
    out
}

/// `Double.toString(d)`.
pub fn double_to_string(d: f64) -> String {
    if d.is_nan() {
        return "NaN".to_string();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    java_fp_string(format!("{:e}", d.abs()), d < 0.0)
}

/// `Float.toString(f)`.
pub fn float_to_string(f: f32) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    java_fp_string(format!("{:e}", f.abs()), f < 0.0)
}
