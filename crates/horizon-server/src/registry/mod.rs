//! The registries a client is sent in the configuration state, built once at startup and shared
//! read-only (one `Arc` by the server: the configuration of every joining player reads them).
//!
//! - Built-in registries (`BuiltInRegistries`): names and protocol ids from the data generator's
//!   `registries.json` (the prepare step's `cache/vanilla-26.3/reports/`), without the registries
//!   MineComplex's Cleanup patch deletes (game tests and JSON-RPC methods).
//! - The synchronized data-driven registries (`RegistryDataLoader.SYNCHRONIZED_REGISTRIES`, Leaf's
//!   list: no `test_environment`/`test_instance`), read from the datapacks as `RegistryDataLoader`
//!   reads them: every pack's `data/<ns>/<registry dir>/**.json`, the topmost pack's file winning,
//!   registered in `Identifier` order of the file (path, then namespace). The packs are the vanilla
//!   datapack in the admin's jar (known pack `minecraft:core:26.3`) and Paper's built-in `paper` pack
//!   (its `paper:raw` chat type, no known pack, so always sent with its data).
//! - Tags of both (`TagLoader`), resolved to protocol ids.
//!
//! Entries keep their JSON; what is synced is that JSON as `JsonOps.convertTo(NbtOps)` makes it
//! ([`nbt_json`]). Vanilla encodes the parsed value with each registry's network codec instead, so
//! the NBT of entries sent in full (only to a client without the `core` pack, and `paper:raw`)
//! differs from Java's where those codecs drop or normalise fields; typed codecs come with the
//! systems that use these registries.

pub mod nbt_json;
mod tags;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::server::vanilla_jar::{VERSION_ID, VanillaJar, VanillaJarError};

/// `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` as Leaf has it, in its order (the registry data
/// packets go out in this order).
pub const SYNCHRONIZED: [&str; 30] = [
    "minecraft:worldgen/biome",
    "minecraft:chat_type",
    "minecraft:trim_pattern",
    "minecraft:trim_material",
    "minecraft:wolf_variant",
    "minecraft:wolf_sound_variant",
    "minecraft:pig_variant",
    "minecraft:pig_sound_variant",
    "minecraft:frog_variant",
    "minecraft:cat_variant",
    "minecraft:cat_sound_variant",
    "minecraft:cow_sound_variant",
    "minecraft:cow_variant",
    "minecraft:chicken_sound_variant",
    "minecraft:chicken_variant",
    "minecraft:zombie_nautilus_variant",
    "minecraft:painting_variant",
    "minecraft:sulfur_cube_archetype",
    "minecraft:dimension_type",
    "minecraft:damage_type",
    "minecraft:banner_pattern",
    "minecraft:enchantment",
    "minecraft:jukebox_song",
    "minecraft:instrument",
    "minecraft:dialog",
    "minecraft:world_clock",
    "minecraft:timeline",
    "minecraft:decorated_pot_pattern",
    "minecraft:block_transformer",
    "minecraft:worldgen/block_state_provider",
];

/// `RegistryDataLoader.WORLDGEN_REGISTRIES`' data-driven registries that are not synchronized
/// (`worldgen/biome` and `worldgen/block_state_provider` are): what world generation reads from
/// the packs (`worldgen::data`).
pub const WORLDGEN: [&str; 15] = [
    "minecraft:worldgen/noise",
    "minecraft:worldgen/density_function",
    "minecraft:worldgen/noise_settings",
    "minecraft:worldgen/material_rule",
    "minecraft:worldgen/material_condition",
    "minecraft:worldgen/carver",
    "minecraft:worldgen/feature",
    "minecraft:worldgen/placed_feature",
    "minecraft:worldgen/structure",
    "minecraft:worldgen/structure_set",
    "minecraft:worldgen/processor_list",
    "minecraft:worldgen/template_pool",
    "minecraft:worldgen/world_preset",
    "minecraft:worldgen/flat_level_generator_preset",
    "minecraft:worldgen/multi_noise_biome_source_parameter_list",
];

/// Paper's patched vanilla data (`paper-server/patches/resources/data/minecraft/worldgen/
/// material_rule/bedrock_{floor,roof}.json.patch`, Leaf's copies under
/// `leaf-server/src/minecraft/resources`): the bedrock rules use
/// `paper:optionally_flat_bedrock_material_condition` (`worldgen::anchor`). Paper patches the
/// files of its jar's vanilla pack, so they replace the jar's files here too.
const PAPER_BEDROCK_FLOOR: &str = r#"{
  "type": "minecraft:condition",
  "if_true": {
    "type": "paper:optionally_flat_bedrock_material_condition",
    "is_roof": false,
    "false_at_and_above": {
      "above_bottom": 5
    },
    "random_name": "minecraft:bedrock_floor",
    "true_at_and_below": {
      "above_bottom": 0
    }
  },
  "then_run": {
    "type": "minecraft:block",
    "result_state": "minecraft:bedrock"
  }
}
"#;
const PAPER_BEDROCK_ROOF: &str = r#"{
  "type": "minecraft:condition",
  "if_true": {
    "type": "minecraft:not",
    "invert": {
      "type": "paper:optionally_flat_bedrock_material_condition",
      "is_roof": true,
      "false_at_and_above": {
        "below_top": 0
      },
      "random_name": "minecraft:bedrock_roof",
      "true_at_and_below": {
        "below_top": 5
      }
    }
  },
  "then_run": {
    "type": "minecraft:block",
    "result_state": "minecraft:bedrock"
  }
}
"#;
/// [`DataPack::vanilla`]'s replaced files (paths below `data/`).
pub const PAPER_BEDROCK_FILES: [(&str, &str); 2] = [
    (
        "minecraft/worldgen/material_rule/bedrock_floor.json",
        PAPER_BEDROCK_FLOOR,
    ),
    (
        "minecraft/worldgen/material_rule/bedrock_roof.json",
        PAPER_BEDROCK_ROOF,
    ),
];

/// Built-in registries MineComplex's Cleanup patch removes from `BuiltInRegistries`.
const REMOVED_BUILTIN: [&str; 5] = [
    "minecraft:test_environment_definition_type",
    "minecraft:test_function",
    "minecraft:test_instance_type",
    "minecraft:incoming_rpc_methods",
    "minecraft:outgoing_rpc_methods",
];

/// Paper's built-in datapack (`data/minecraft/datapacks/paper` in Paper's jar): one chat type.
const PAPER_RAW_CHAT_TYPE: &str = r#"{
    "chat": {"parameters": ["content"], "translation_key": "%s"},
    "narration": {"parameters": ["content"], "translation_key": "%s"}
}"#;

/// `registries.json` in the prepare step's reports (`cache/vanilla-26.3/reports/`).
pub fn default_report_path(server_dir: &Path) -> PathBuf {
    server_dir.join("cache/vanilla-26.3/reports/registries.json")
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Jar(#[from] VanillaJarError),
    #[error("couldn't read {path}: {message}")]
    Report { path: String, message: String },
    /// `RegistryDataLoader`'s errors stop the server, as in vanilla.
    #[error("Failed to parse {file} from pack {pack}: {message}")]
    Element {
        file: String,
        pack: String,
        message: String,
    },
}

/// `KnownPack`: a pack the client may already have.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

impl KnownPack {
    /// `KnownPack.vanilla(id)`.
    pub fn vanilla(id: &str) -> Self {
        Self {
            namespace: "minecraft".into(),
            id: id.into(),
            version: VERSION_ID.into(),
        }
    }
}

/// `Identifier.parse`: `ns:path` or `path` (namespace `minecraft`), as `ns:path`.
pub fn parse_identifier(text: &str) -> Result<String, String> {
    let (namespace, path) = text.split_once(':').unwrap_or(("minecraft", text));
    let namespace = if namespace.is_empty() {
        "minecraft"
    } else {
        namespace
    };
    if !namespace
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
    {
        return Err(format!(
            "Non [a-z0-9_.-] character in namespace of location: {namespace}:{path}"
        ));
    }
    if !path
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'/'))
    {
        return Err(format!(
            "Non [a-z0-9/._-] character in path of location: {namespace}:{path}"
        ));
    }
    Ok(format!("{namespace}:{path}"))
}

/// `Registries.elementsDirPath`: the registry's path, prefixed by its namespace unless `minecraft`.
fn registry_dir(name: &str) -> String {
    match name.split_once(':') {
        Some(("minecraft", path)) => path.to_owned(),
        Some((ns, path)) => format!("{ns}/{path}"),
        None => name.to_owned(),
    }
}

/// One datapack's `data/` files (key: the path below `data/`, e.g. `minecraft/chat_type/chat.json`).
#[derive(Clone, Debug)]
pub struct DataPack {
    pub id: String,
    pub known_pack: Option<KnownPack>,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl DataPack {
    /// The vanilla datapack in the jar (`ServerPacksSource`'s `vanilla` pack, known pack
    /// `minecraft:core`) as Paper ships it: the `minecraft` namespace's files the server reads
    /// (the synchronized and [`WORLDGEN`] registries' directories and `tags/`), with Paper's
    /// [`PAPER_BEDROCK_FILES`].
    pub fn vanilla(jar: &mut VanillaJar) -> Result<Self, VanillaJarError> {
        let dirs: Vec<String> = SYNCHRONIZED
            .iter()
            .chain(&WORLDGEN)
            .map(|r| format!("minecraft/{}/", registry_dir(r)))
            .chain(["minecraft/tags/".to_owned()])
            .collect();
        let mut files = BTreeMap::new();
        for name in jar.entry_names() {
            let Some(path) = name.strip_prefix("data/") else {
                continue;
            };
            if path.ends_with(".json") && dirs.iter().any(|d| path.starts_with(d.as_str())) {
                let bytes = jar.read(&name)?.unwrap_or_default();
                files.insert(path.to_owned(), bytes);
            }
        }
        for (path, json) in PAPER_BEDROCK_FILES {
            files.insert(path.to_owned(), json.as_bytes().to_vec());
        }
        Ok(Self {
            id: "vanilla".into(),
            known_pack: Some(KnownPack::vanilla("core")),
            files,
        })
    }

    /// Paper's built-in `paper` pack (required, on top, no known pack).
    pub fn paper() -> Self {
        Self {
            id: "paper".into(),
            known_pack: None,
            files: BTreeMap::from([(
                "paper/chat_type/raw.json".to_owned(),
                PAPER_RAW_CHAT_TYPE.as_bytes().to_vec(),
            )]),
        }
    }

    /// `FileToIdConverter.json(dir).listMatchingResources` for this pack: (file `Identifier` as
    /// (path, namespace) for sorting, element id, bytes).
    fn list<'a>(
        &'a self,
        dir: &'a str,
    ) -> impl Iterator<Item = ((String, String), String, &'a [u8])> + 'a {
        self.files.iter().filter_map(move |(key, bytes)| {
            let (ns, path) = key.split_once('/')?;
            let rest = path.strip_prefix(dir)?.strip_prefix('/')?;
            let id = rest.strip_suffix(".json")?;
            Some((
                (path.to_owned(), ns.to_owned()),
                format!("{ns}:{id}"),
                bytes.as_slice(),
            ))
        })
    }
}

/// A registry element's file as `RegistryDataLoader` picks it: the topmost pack's.
#[derive(Clone, Copy, Debug)]
pub struct ElementFile<'a> {
    /// The element id (`ns:path` below the registry directory).
    pub id: &'a str,
    /// The file as an `Identifier` (`ns:<registry dir>/<id>.json`), for messages.
    pub file: &'a str,
    pub pack: &'a DataPack,
    pub bytes: &'a [u8],
}

/// Every element of a registry across `packs` (bottom to top), the topmost pack's file for each
/// id, in `Identifier` order of the files (path, then namespace) as `RegistryDataLoader` registers
/// them.
pub fn for_each_element<E>(
    packs: &[DataPack],
    registry: &str,
    mut element: impl FnMut(ElementFile<'_>) -> Result<(), E>,
) -> Result<(), E> {
    let dir = registry_dir(registry);
    let mut files = BTreeMap::new();
    for pack in packs {
        for (key, id, bytes) in pack.list(&dir) {
            files.insert(key, (id, pack, bytes));
        }
    }
    for ((path, ns), (id, pack, bytes)) in files {
        element(ElementFile {
            id: &id,
            file: &format!("{ns}:{path}"),
            pack,
            bytes,
        })?;
    }
    Ok(())
}

/// An entry of a synchronized registry.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    /// The known pack of the pack its file came from (`RegistrationInfo.knownPackInfo`).
    pub known_pack: Option<KnownPack>,
    pub json: Value,
    /// `json` as network NBT (what `registry_data` carries when the client lacks the pack).
    pub nbt: Box<[u8]>,
}

/// A registry's tags as the client gets them: in Java's `HashMap` order, ids in tag order.
#[derive(Clone, Debug, Default)]
pub struct Tags(pub Vec<(String, Vec<i32>)>);

impl Tags {
    fn new(mut tags: Vec<(String, Vec<i32>)>) -> Self {
        nbt_json::java_hash_order(&mut tags, |(id, _)| nbt_json::identifier_hash(id));
        Self(tags)
    }

    pub fn get(&self, id: &str) -> Option<&[i32]> {
        self.0
            .iter()
            .find(|(t, _)| t == id)
            .map(|(_, ids)| ids.as_slice())
    }
}

#[derive(Clone, Debug)]
pub struct SyncedRegistry {
    pub name: &'static str,
    /// In registration order: the index is the protocol id.
    pub entries: Vec<Entry>,
    pub tags: Tags,
}

#[derive(Clone, Debug)]
pub struct BuiltinRegistry {
    pub name: String,
    /// Element ids by protocol id.
    pub entries: Vec<String>,
    ids: HashMap<String, i32>,
    pub tags: Tags,
}

impl BuiltinRegistry {
    pub fn id(&self, element: &str) -> Option<i32> {
        self.ids.get(element).copied()
    }
}

/// All registries the configuration state sends. Immutable once built.
#[derive(Clone, Debug)]
pub struct Registries {
    /// The packs' known pack infos, in pack order (what `select_known_packs` offers).
    pub known_packs: Vec<KnownPack>,
    pub synced: Vec<SyncedRegistry>,
    pub builtin: Vec<BuiltinRegistry>,
}

impl Registries {
    /// Leaf's registries: `registries.json` from `reports`, the jar's vanilla pack and Paper's pack.
    pub fn load(jar: &mut VanillaJar, registries_json: &Path) -> Result<Self, RegistryError> {
        let report =
            std::fs::read_to_string(registries_json).map_err(|e| RegistryError::Report {
                path: registries_json.display().to_string(),
                message: e.to_string(),
            })?;
        let builtin = parse_builtin(&report).map_err(|message| RegistryError::Report {
            path: registries_json.display().to_string(),
            message,
        })?;
        Self::build(builtin, &[DataPack::vanilla(jar)?, DataPack::paper()])
    }

    /// Builds the synchronized registries and every tag from `packs` (bottom to top).
    pub fn build(
        mut builtin: Vec<BuiltinRegistry>,
        packs: &[DataPack],
    ) -> Result<Self, RegistryError> {
        let mut synced = Vec::with_capacity(SYNCHRONIZED.len());
        for name in SYNCHRONIZED {
            let dir = registry_dir(name);
            let mut entries = Vec::new();
            for_each_element(packs, name, |file| {
                let error = |message: String| RegistryError::Element {
                    file: file.file.to_owned(),
                    pack: file.pack.id.clone(),
                    message,
                };
                let json: Value =
                    serde_json::from_slice(file.bytes).map_err(|e| error(e.to_string()))?;
                let mut nbt = Vec::new();
                nbt_json::write_network(&json, &mut nbt).map_err(|e| error(e.to_string()))?;
                entries.push(Entry {
                    id: file.id.to_owned(),
                    known_pack: file.pack.known_pack.clone(),
                    json,
                    nbt: nbt.into(),
                });
                Ok::<(), RegistryError>(())
            })?;
            let ids: HashMap<&str, i32> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.id.as_str(), i as i32))
                .collect();
            let tags = Tags::new(load_tags(packs, &dir, |id| ids.get(id).copied()));
            synced.push(SyncedRegistry {
                name,
                entries,
                tags,
            });
        }
        for registry in &mut builtin {
            let dir = registry_dir(&registry.name);
            let tags = load_tags(packs, &dir, |id| registry.ids.get(id).copied());
            registry.tags = Tags::new(tags);
        }
        let known_packs = packs.iter().filter_map(|p| p.known_pack.clone()).collect();
        Ok(Self {
            known_packs,
            synced,
            builtin,
        })
    }

    pub fn synced(&self, name: &str) -> Option<&SyncedRegistry> {
        self.synced.iter().find(|r| r.name == name)
    }

    pub fn builtin(&self, name: &str) -> Option<&BuiltinRegistry> {
        self.builtin.iter().find(|r| r.name == name)
    }
}

/// `TagLoader.load` + `build` for one registry directory.
fn load_tags(
    packs: &[DataPack],
    dir: &str,
    element: impl Fn(&str) -> Option<i32>,
) -> Vec<(String, Vec<i32>)> {
    let tag_dir = format!("tags/{dir}");
    // Files in `Identifier` order, each with its tag id and stack (bottom pack first).
    type Stack<'a> = (String, Vec<(&'a DataPack, &'a [u8])>);
    let mut stacks: BTreeMap<(String, String), Stack<'_>> = BTreeMap::new();
    for pack in packs {
        for (key, id, bytes) in pack.list(&tag_dir) {
            stacks
                .entry(key)
                .or_insert_with(|| (id, Vec::new()))
                .1
                .push((pack, bytes));
        }
    }
    let mut merged: BTreeMap<String, Vec<tags::TagEntry>> = BTreeMap::new();
    for ((path, ns), (id, stack)) in stacks {
        for (pack, bytes) in stack {
            let parsed = serde_json::from_slice::<Value>(bytes)
                .map_err(|e| e.to_string())
                .and_then(|json| tags::parse_tag_file(&json));
            match parsed {
                Ok((entries, replace)) => {
                    let list = merged.entry(id.clone()).or_default();
                    if replace {
                        list.clear();
                    }
                    list.extend(entries);
                }
                Err(e) => tracing::error!(
                    "Couldn't read tag list {id} from {ns}:{path} in data pack {}: {e}",
                    pack.id
                ),
            }
        }
    }
    tags::build_tags(&merged, element)
}

/// `registries.json`: `{"minecraft:block": {"entries": {"minecraft:air": {"protocol_id": 0}, ...},
/// ...}, ...}`, without Leaf's removed registries.
pub fn parse_builtin(report: &str) -> Result<Vec<BuiltinRegistry>, String> {
    let json: serde_json::Map<String, Value> =
        serde_json::from_str(report).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (name, registry) in json {
        if REMOVED_BUILTIN.contains(&name.as_str()) {
            continue;
        }
        let entries = registry
            .get("entries")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{name} has no entries"))?;
        let mut by_id = vec![None; entries.len()];
        for (element, e) in entries {
            let id = e
                .get("protocol_id")
                .and_then(Value::as_u64)
                .filter(|&id| (id as usize) < by_id.len())
                .ok_or_else(|| format!("{name}: {element} has no valid protocol_id"))?;
            by_id[id as usize] = Some(element.clone());
        }
        let entries: Vec<String> = by_id
            .into_iter()
            .collect::<Option<_>>()
            .ok_or_else(|| format!("{name}: protocol ids are not 0..{}", entries.len()))?;
        let ids = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.clone(), i as i32))
            .collect();
        out.push(BuiltinRegistry {
            name,
            entries,
            ids,
            tags: Tags::default(),
        });
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn pack(id: &str, known: Option<KnownPack>, files: &[(&str, &str)]) -> DataPack {
        DataPack {
            id: id.into(),
            known_pack: known,
            files: files
                .iter()
                .map(|(p, j)| (p.to_string(), j.as_bytes().to_vec()))
                .collect(),
        }
    }

    pub const REPORT: &str = r#"{
        "minecraft:block": {"entries": {"minecraft:stone": {"protocol_id": 1}, "minecraft:air": {"protocol_id": 0}}},
        "minecraft:test_function": {"entries": {"minecraft:always_pass": {"protocol_id": 0}}}
    }"#;

    /// A small world: two dimension types in the vanilla pack, a third and an override in a
    /// world datapack on top, tags merged across both.
    pub fn sample() -> Registries {
        let vanilla = pack(
            "vanilla",
            Some(KnownPack::vanilla("core")),
            &[
                (
                    "minecraft/dimension_type/overworld_caves.json",
                    r#"{"height": 384}"#,
                ),
                (
                    "minecraft/dimension_type/overworld.json",
                    r#"{"height": 384, "min_y": -64}"#,
                ),
                (
                    "minecraft/tags/block/base.json",
                    r#"{"values": ["stone", "air"]}"#,
                ),
                (
                    "minecraft/tags/dimension_type/all.json",
                    r#"{"values": ["overworld"]}"#,
                ),
                (
                    "minecraft/worldgen/biome/plains.json",
                    r#"{"temperature": 0.8}"#,
                ),
            ],
        );
        let world = pack(
            "file/custom",
            None,
            &[
                (
                    "minecraft/dimension_type/overworld.json",
                    r#"{"height": 256}"#,
                ),
                ("custom/dimension_type/moon.json", r#"{"height": 16}"#),
                (
                    "minecraft/tags/dimension_type/all.json",
                    r#"{"values": ["custom:moon"]}"#,
                ),
                (
                    "minecraft/tags/block/base.json",
                    r#"{"replace": true, "values": ["air"]}"#,
                ),
            ],
        );
        Registries::build(
            parse_builtin(REPORT).unwrap(),
            &[vanilla, world, DataPack::paper()],
        )
        .unwrap()
    }

    #[test]
    fn loads_in_registry_order_with_the_top_pack_winning() {
        let r = sample();
        assert_eq!(r.known_packs, vec![KnownPack::vanilla("core")]);
        assert_eq!(r.builtin.len(), 1, "Leaf removes the test registries");
        let dims = r.synced("minecraft:dimension_type").unwrap();
        let ids: Vec<_> = dims.entries.iter().map(|e| e.id.as_str()).collect();
        // `Identifier` order of the files: path first ("moon.json" < "overworld.json" <
        // "overworld_caves.json", '.' sorting before '_'), then namespace.
        assert_eq!(
            ids,
            [
                "custom:moon",
                "minecraft:overworld",
                "minecraft:overworld_caves"
            ]
        );
        assert_eq!(
            dims.entries[1].known_pack, None,
            "overridden by the world pack"
        );
        assert_eq!(dims.entries[2].known_pack, Some(KnownPack::vanilla("core")));
        assert_eq!(dims.entries[1].json, serde_json::json!({"height": 256}));
        assert_eq!(dims.tags.get("minecraft:all"), Some(&[1, 0][..]));
        assert_eq!(
            r.builtin("minecraft:block")
                .unwrap()
                .tags
                .get("minecraft:base"),
            Some(&[0][..])
        );
        let chat = r.synced("minecraft:chat_type").unwrap();
        assert_eq!(chat.entries[0].id, "paper:raw");
        assert_eq!(r.synced.len(), 30);
    }
}
