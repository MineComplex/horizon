//! Resource identifiers (`Identifier`): `namespace:path`, the namespace
//! `minecraft` when left out.

/// `Identifier.tryParse`: the namespace and path, or `None` when either holds
/// a character vanilla refuses.
pub fn identifier(id: &str) -> Option<(String, String)> {
    let (ns, path) = match id.split_once(':') {
        Some((ns, path)) => (if ns.is_empty() { "minecraft" } else { ns }, path),
        None => ("minecraft", id),
    };
    let ns_ok = ns
        .bytes()
        .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'));
    let path_ok = path
        .bytes()
        .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'));
    (ns_ok && path_ok).then(|| (ns.to_owned(), path.to_owned()))
}

/// `id` with its namespace written out (`stone` is `minecraft:stone`); an
/// invalid one as it was.
pub fn namespaced(id: &str) -> String {
    match identifier(id) {
        Some((ns, path)) => format!("{ns}:{path}"),
        None => id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn namespaces_default_to_minecraft() {
        assert_eq!(super::namespaced("knockback"), "minecraft:knockback");
        assert_eq!(super::namespaced("a:b/c"), "a:b/c");
        assert_eq!(super::namespaced("Bad Id"), "Bad Id");
    }
}
