//! JSON data as the network NBT Java writes for it: `JsonOps.INSTANCE.convertTo(NbtOps.INSTANCE, json)`
//! then `FriendlyByteBuf.writeNbt` (a type byte, then the unnamed payload).
//!
//! Written straight from `serde_json` values, without an NBT tree in between, because the byte order
//! has to be Java's: a `CompoundTag` is a `HashMap`, so its keys come out in `HashMap` iteration order
//! ([`java_hash_order`]), not in the file's order or sorted. DFU's number rule: an integral value is
//! the narrowest of byte/short/int/long that holds it, anything else a float when `(float) d == d`,
//! else a double; booleans are bytes, lists are `ListTag`s (mixed element types wrapped in compounds
//! with a `""` key, as 26.3's `ListTag.write` does).

use serde_json::Value;

use crate::util::random::java_string_hash;

/// The order a `java.util.HashMap` built by `put`ting `items` one by one (from `new HashMap<>()`)
/// iterates them: by bucket (`(h ^ h >>> 16) & (capacity - 1)`), then insertion order within a bucket.
/// The capacity is the table size those puts grow it to (16, doubled whenever the size passes 3/4).
/// Treeified buckets (8+ colliding entries in a table of 64+) would differ; real keys never get there.
pub fn java_hash_order<T>(items: &mut [T], hash: impl Fn(&T) -> i32) {
    let mut capacity = 16usize;
    while items.len() > capacity / 4 * 3 {
        capacity *= 2;
    }
    let mask = capacity - 1;
    items.sort_by_key(|item| {
        let h = hash(item);
        ((h ^ ((h as u32) >> 16) as i32) as u32 as usize) & mask
    });
}

/// `String.hashCode` of a Rust string (over its UTF-16 units).
pub fn string_hash(s: &str) -> i32 {
    java_string_hash(s.encode_utf16())
}

/// `Identifier.hashCode`: `31 * namespace.hashCode() + path.hashCode()`, for `"ns:path"`.
pub fn identifier_hash(id: &str) -> i32 {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    string_hash(namespace)
        .wrapping_mul(31)
        .wrapping_add(string_hash(path))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NbtJsonError {
    #[error("null has no NBT form")]
    Null,
    #[error("string longer than 65535 bytes of modified UTF-8")]
    TooLong,
}

/// `DataOutput.writeUTF`: a big-endian length, then modified UTF-8 (`\0` as two bytes, characters
/// past the BMP as two 3-byte surrogates).
pub fn put_modified_utf8(out: &mut Vec<u8>, s: &str) -> Result<(), NbtJsonError> {
    let len: usize = s
        .encode_utf16()
        .map(|c| match c {
            1..=0x7f => 1,
            0 | 0x80..=0x7ff => 2,
            _ => 3,
        })
        .sum();
    let len = u16::try_from(len).map_err(|_| NbtJsonError::TooLong)?;
    out.extend_from_slice(&len.to_be_bytes());
    for c in s.encode_utf16() {
        match c {
            1..=0x7f => out.push(c as u8),
            0 | 0x80..=0x7ff => {
                out.extend_from_slice(&[0xc0 | (c >> 6) as u8, 0x80 | (c & 0x3f) as u8])
            }
            _ => out.extend_from_slice(&[
                0xe0 | (c >> 12) as u8,
                0x80 | ((c >> 6) & 0x3f) as u8,
                0x80 | (c & 0x3f) as u8,
            ]),
        }
    }
    Ok(())
}

/// The tag a JSON number becomes (`JsonOps.convertTo`'s `BigDecimal` rule).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Number {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

fn number(n: &serde_json::Number) -> Number {
    let integral = if let Some(l) = n.as_i64() {
        Some(l)
    } else if n.is_u64() {
        None // above Long.MAX_VALUE: `longValueExact` throws
    } else {
        let d = n.as_f64().unwrap_or(0.0);
        // `longValueExact` succeeds for an integral value in range (-0.0 included, as 0).
        (d.fract() == 0.0 && (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d))
            .then_some(d as i64)
    };
    match integral {
        Some(l) if i8::try_from(l).is_ok() => Number::Byte(l as i8),
        Some(l) if i16::try_from(l).is_ok() => Number::Short(l as i16),
        Some(l) if i32::try_from(l).is_ok() => Number::Int(l as i32),
        Some(l) => Number::Long(l),
        None => {
            let d = n.as_f64().unwrap_or(f64::INFINITY);
            if f64::from(d as f32) == d {
                Number::Float(d as f32)
            } else {
                Number::Double(d)
            }
        }
    }
}

fn type_id(value: &Value) -> Result<u8, NbtJsonError> {
    Ok(match value {
        Value::Null => return Err(NbtJsonError::Null),
        Value::Bool(_) => 1,
        Value::Number(n) => match number(n) {
            Number::Byte(_) => 1,
            Number::Short(_) => 2,
            Number::Int(_) => 3,
            Number::Long(_) => 4,
            Number::Float(_) => 5,
            Number::Double(_) => 6,
        },
        Value::String(_) => 8,
        Value::Array(_) => 9,
        Value::Object(_) => 10,
    })
}

/// The network NBT of `value`: its type byte, then its payload.
pub fn write_network(value: &Value, out: &mut Vec<u8>) -> Result<(), NbtJsonError> {
    out.push(type_id(value)?);
    write_payload(value, out)
}

fn write_payload(value: &Value, out: &mut Vec<u8>) -> Result<(), NbtJsonError> {
    match value {
        Value::Null => return Err(NbtJsonError::Null),
        Value::Bool(b) => out.push(u8::from(*b)),
        Value::Number(n) => match number(n) {
            Number::Byte(v) => out.push(v as u8),
            Number::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
            Number::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
            Number::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
            Number::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
            Number::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
        },
        Value::String(s) => put_modified_utf8(out, s)?,
        Value::Array(items) => {
            // `identifyRawElementType`: the shared type, compound when mixed, end when empty.
            let mut element = 0u8;
            for item in items {
                let id = type_id(item)?;
                if element == 0 {
                    element = id;
                } else if element != id {
                    element = 10;
                    break;
                }
            }
            out.push(element);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                let is_wrapper =
                    matches!(item, Value::Object(m) if m.len() == 1 && m.contains_key(""));
                if element == 10 && (!item.is_object() || is_wrapper) {
                    // `wrapElement`: `{"": item}`.
                    out.push(type_id(item)?);
                    out.extend_from_slice(&[0, 0]);
                    write_payload(item, out)?;
                    out.push(0);
                } else {
                    write_payload(item, out)?;
                }
            }
        }
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            java_hash_order(&mut entries, |(k, _)| string_hash(k));
            for (key, item) in entries {
                out.push(type_id(item)?);
                put_modified_utf8(out, key)?;
                write_payload(item, out)?;
            }
            out.push(0);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::login::tests::hex;

    fn nbt(json: &str) -> Vec<u8> {
        let mut out = Vec::new();
        write_network(&serde_json::from_str(json).unwrap(), &mut out).unwrap();
        out
    }

    #[test]
    fn matches_json_ops_convert_to_nbt_ops() {
        // Paper's `paper:raw` chat type, as Java 25 writes `JsonOps.convertTo(NbtOps)` of it: the
        // compound keys in HashMap order (`translation_key` before `parameters`).
        let raw = r#"{"chat": {"parameters": ["content"], "translation_key": "%s"},
            "narration": {"parameters": ["content"], "translation_key": "%s"}}"#;
        assert_eq!(
            nbt(raw),
            hex(
                "0a0a00046368617408000f7472616e736c6174696f6e5f6b65790002257309000a706172616d6574657273\
                 08000000010007636f6e74656e74000a00096e6172726174696f6e08000f7472616e736c6174696f6e5f6b6579\
                 0002257309000a706172616d657465727308000000010007636f6e74656e740000"
            )
        );
        // Numbers: narrowest integral type, then float when exact, else double; booleans as bytes.
        assert_eq!(nbt("1.0"), hex("0101"));
        assert_eq!(nbt("300"), hex("02012c"));
        assert_eq!(nbt("70000"), hex("0300011170"));
        assert_eq!(nbt("5000000000"), hex("04000000012a05f200"));
        assert_eq!(nbt("0.5"), hex("053f000000"));
        assert_eq!(nbt("0.1"), hex("063fb999999999999a"));
        assert_eq!(nbt("true"), hex("0101"));
        // Mixed lists are compound lists with `""`-wrapped elements; empty lists have type end.
        assert_eq!(
            nbt(r#"[1, "a"]"#),
            hex("090a000000020100000100080000000161 00"
                .replace(' ', "")
                .as_str())
        );
        assert_eq!(nbt("[]"), hex("090000000000"));
    }

    #[test]
    fn hash_order_grows_like_a_hash_map() {
        // "k0".hashCode() = 3365 (bucket 5 of 16 and of 32), "k10" = 104394 (spread 104395: bucket
        // 11 of 16 and of 32), "k9" = 3374 (bucket 14): 12 keys stay in 16 buckets, 13 need 32.
        let mut keys: Vec<String> = (0..12).map(|i| format!("k{i}")).collect();
        java_hash_order(&mut keys, |k| string_hash(k));
        let at = |keys: &[String], k: &str| keys.iter().position(|x| x == k).unwrap();
        assert!(at(&keys, "k0") < at(&keys, "k10") && at(&keys, "k10") < at(&keys, "k9"));
        assert_eq!(string_hash("k10"), 104_394);
    }
}
