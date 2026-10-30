//! Tags (`TagLoader`, `TagFile`, `TagEntry`): every pack's `tags/<registry>/<id>.json` for a tag, in
//! pack order (`replace` clears what lower packs added), then each tag resolved to element ids in
//! its entries' order without duplicates (`LinkedHashSet`), nested `#tag` entries first resolved.
//! An optional entry (`{"id": ..., "required": false}`) whose element or tag is missing is skipped;
//! a missing required one makes the whole tag fail (logged, left out), as does any tag depending on it.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use super::parse_identifier;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TagEntry {
    /// Full identifier (`ns:path`) of the element or tag.
    pub id: String,
    pub tag: bool,
    pub required: bool,
}

impl std::fmt::Display for TagEntry {
    /// `TagEntry.toString`: `#id` for a tag, `?` after an optional entry.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = if self.tag { "#" } else { "" };
        let optional = if self.required { "" } else { "?" };
        write!(f, "{hash}{}{optional}", self.id)
    }
}

fn entry_id(text: &str, required: bool) -> Result<TagEntry, String> {
    let (tag, id) = match text.strip_prefix('#') {
        Some(id) => (true, id),
        None => (false, text),
    };
    Ok(TagEntry {
        id: parse_identifier(id)?,
        tag,
        required,
    })
}

/// `TagFile.CODEC`: `{"values": [entries], "replace": false}`; an entry is `"id"`, `"#tag"` or
/// `{"id": "id or #tag", "required": true}`.
pub(crate) fn parse_tag_file(json: &Value) -> Result<(Vec<TagEntry>, bool), String> {
    let values = json
        .get("values")
        .ok_or("No key values")?
        .as_array()
        .ok_or("values: Not a list")?;
    let replace = match json.get("replace") {
        None => false,
        Some(v) => v.as_bool().ok_or("replace: Not a boolean")?,
    };
    let entries = values
        .iter()
        .map(|v| match v {
            Value::String(s) => entry_id(s, true),
            Value::Object(o) => {
                let id = o.get("id").and_then(Value::as_str).ok_or("No key id")?;
                let required = match o.get("required") {
                    None => true,
                    Some(r) => r.as_bool().ok_or("required: Not a boolean")?,
                };
                entry_id(id, required)
            }
            _ => Err(format!("Not a tag entry: {v}")),
        })
        .collect::<Result<_, _>>()?;
    Ok((entries, replace))
}

enum State {
    Building,
    Built(Option<Vec<i32>>),
}

struct Builder<'a, F> {
    files: &'a BTreeMap<String, Vec<TagEntry>>,
    element: F,
    state: HashMap<&'a str, State>,
}

impl<'a, F: Fn(&str) -> Option<i32>> Builder<'a, F> {
    /// The tag's ids, or `None` when it has no file, failed, or is part of a cycle being built
    /// (`DependencySorter` drops one edge of a cycle, so one side sees the other unbuilt).
    fn build(&mut self, id: &'a str) -> Option<&Vec<i32>> {
        if !self.state.contains_key(id) {
            let entries = self.files.get(id)?;
            self.state.insert(id, State::Building);
            let mut values = Vec::new();
            let mut seen = HashSet::new();
            let mut missing = Vec::new();
            for entry in entries {
                let found = if entry.tag {
                    match self.build(&entry.id) {
                        Some(ids) => {
                            values.extend(ids.iter().filter(|v| seen.insert(**v)));
                            true
                        }
                        None => false,
                    }
                } else {
                    match (self.element)(&entry.id) {
                        Some(v) => {
                            if seen.insert(v) {
                                values.push(v);
                            }
                            true
                        }
                        None => false,
                    }
                };
                if !found && entry.required {
                    missing.push(entry.to_string());
                }
            }
            let result = if missing.is_empty() {
                Some(values)
            } else {
                tracing::error!(
                    "Couldn't load tag {id} as it is missing following references: {}",
                    missing.join(", ")
                );
                None
            };
            self.state.insert(id, State::Built(result));
        }
        match self.state.get(id) {
            Some(State::Built(Some(ids))) => Some(ids),
            _ => None,
        }
    }
}

/// Resolves every tag of `files` (tag id → its merged entries) against `element` (element id →
/// protocol id). The result is in tag id order; tags that failed are left out.
pub(crate) fn build_tags(
    files: &BTreeMap<String, Vec<TagEntry>>,
    element: impl Fn(&str) -> Option<i32>,
) -> Vec<(String, Vec<i32>)> {
    let mut builder = Builder {
        files,
        element,
        state: HashMap::new(),
    };
    files
        .keys()
        .filter_map(|id| builder.build(id).map(|ids| (id.clone(), ids.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(tags: &[(&str, &str)]) -> BTreeMap<String, Vec<TagEntry>> {
        tags.iter()
            .map(|(id, json)| {
                let (entries, _) = parse_tag_file(&serde_json::from_str(json).unwrap()).unwrap();
                (id.to_string(), entries)
            })
            .collect()
    }

    #[test]
    fn nested_optional_and_missing_entries() {
        let ids = ["minecraft:a", "minecraft:b", "minecraft:c"];
        let element = |id: &str| ids.iter().position(|x| *x == id).map(|i| i as i32);
        let tags = files(&[
            (
                "minecraft:all",
                r##"{"values": ["c", "#minecraft:ab", "a", {"id": "zzz", "required": false}, {"id": "#nope", "required": false}]}"##,
            ),
            ("minecraft:ab", r#"{"values": ["a", "minecraft:b"]}"#),
            ("minecraft:broken", r#"{"values": ["a", "missing"]}"#),
            ("minecraft:uses_broken", r##"{"values": ["#broken"]}"##),
        ]);
        assert_eq!(
            build_tags(&tags, element),
            vec![
                ("minecraft:ab".to_string(), vec![0, 1]),
                ("minecraft:all".to_string(), vec![2, 0, 1]),
            ]
        );
        let bad = parse_tag_file(&serde_json::json!({"values": ["Bad Id"]}));
        assert!(bad.is_err());
    }
}
