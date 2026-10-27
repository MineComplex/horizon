//! What a new world is made with (CreateWorldScreen's `WorldCreationUiState` as the server reads
//! it): game mode, difficulty, rules, generation, and the Superflat layers (`FlatLevelGeneratorSettings`).
use horizon_world::nbt::Tag;
use std::{collections::BTreeSet, path::Path};

/// Difficulty ids, in the cycle button's order.
pub const DIFFICULTIES: [&str; 4] = ["peaceful", "easy", "normal", "hard"];

/// WorldCreationUiState beyond the name and seed.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldOptions {
    /// The open tab: Game, World or More.
    pub tab: usize,
    /// Survival locked to Hard, one life (`SelectedGameMode.HARDCORE`).
    pub hardcore: bool,
    /// Index in `DIFFICULTIES`.
    pub difficulty: usize,
    /// Allow Cheats as toggled; unset follows the game mode.
    pub commands: Option<bool>,
    /// The Superflat world preset instead of Default.
    pub flat: bool,
    /// CreateFlatWorldScreen's layers and biome, kept with the world type.
    pub flat_settings: Option<FlatSettings>,
    pub structures: bool,
    pub bonus_chest: bool,
    /// `getGameRules`: the Game Rules screen's values, saved with the world.
    pub rules: crate::gamerules::GameRules,
}
impl Default for WorldOptions {
    fn default() -> Self {
        Self {
            tab: 0,
            hardcore: false,
            difficulty: 2,
            commands: None,
            flat: false,
            flat_settings: None,
            structures: true,
            bonus_chest: false,
            rules: crate::gamerules::GameRules::default(),
        }
    }
}
impl WorldOptions {
    /// `getDifficulty`: Hardcore is always Hard.
    pub fn difficulty(&self) -> usize {
        if self.hardcore { 3 } else { self.difficulty }
    }
    /// `isAllowCommands`: never in Hardcore, else on for Creative until toggled.
    pub fn commands(&self, creative: bool) -> bool {
        !self.hardcore && self.commands.unwrap_or(creative)
    }
    /// `isBonusChest`: never in Hardcore.
    pub fn bonus_chest(&self) -> bool {
        !self.hardcore && self.bonus_chest
    }
    pub fn world_gen(&self) -> crate::save::WorldGen {
        crate::save::WorldGen {
            flat: self.flat,
            structures: self.structures,
            bonus_chest: self.bonus_chest(),
        }
    }
    /// The custom layers, as `write_world_gen_settings_with` takes them.
    pub fn flat_settings(&self) -> Option<horizon_world::nbt::Tag> {
        self.flat_settings.as_ref().map(FlatSettings::to_tag)
    }
}
/// DimensionType.Y_SIZE: the most blocks a flat generator takes.
const Y_SIZE: i32 = 4064;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatLayer {
    /// A block id such as `minecraft:dirt`.
    pub block: String,
    /// How many blocks tall this layer is (0..=4064).
    pub height: i32,
}

/// FlatLevelGeneratorSettings: the layers (bottom to top), the biome and the
/// decorations CreateFlatWorldScreen edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatSettings {
    pub biome: String,
    pub features: bool,
    pub lakes: bool,
    pub layers: Vec<FlatLayer>,
    pub structure_overrides: Vec<String>,
}
impl Default for FlatSettings {
    fn default() -> Self {
        // worldgen/world_preset/flat.json's overworld.
        Self {
            biome: "minecraft:plains".into(),
            features: false,
            lakes: false,
            layers: vec![
                FlatLayer {
                    block: "minecraft:bedrock".into(),
                    height: 1,
                },
                FlatLayer {
                    block: "minecraft:dirt".into(),
                    height: 2,
                },
                FlatLayer {
                    block: "minecraft:grass_block".into(),
                    height: 1,
                },
            ],
            structure_overrides: vec!["minecraft:strongholds".into(), "minecraft:villages".into()],
        }
    }
}
impl FlatSettings {
    /// A saved world's flat generator settings, for Re-Create.
    pub fn from_world(dir: &Path) -> Option<Self> {
        let settings = crate::save::world_gen_settings(dir)?;
        let generator = settings
            .get("dimensions")?
            .get("minecraft:overworld")?
            .get("generator")?;
        if crate::save::text(generator.get("type"))?.trim_start_matches("minecraft:") != "flat" {
            return None;
        }
        Self::from_tag(generator.get("settings")?)
    }
    pub fn from_tag(tag: &Tag) -> Option<Self> {
        let Some(Tag::List(list)) = tag.get("layers") else {
            return None;
        };
        let layers: Vec<FlatLayer> = list
            .iter()
            .filter_map(|layer| {
                Some(FlatLayer {
                    block: crate::save::text(layer.get("block"))?,
                    height: crate::save::number(layer.get("height"))?.clamp(0, Y_SIZE as i64)
                        as i32,
                })
            })
            .collect();
        // `layers: []` is a valid void flat world; a list whose entries all
        // fail to parse is not.
        if layers.is_empty() && !list.is_empty() {
            return None;
        }
        let flag = |key: &str, default: bool| {
            crate::save::number(tag.get(key)).map_or(default, |value| value != 0)
        };
        let structure_overrides = match tag.get("structure_overrides") {
            Some(Tag::String(text)) => vec![text.to_string_lossy()],
            Some(Tag::List(list)) => list
                .iter()
                .filter_map(|value| crate::save::text(Some(value)))
                .collect(),
            _ => Vec::new(),
        };
        Some(Self {
            biome: crate::save::text(tag.get("biome")).unwrap_or_else(|| "minecraft:plains".into()),
            features: flag("features", false),
            lakes: flag("lakes", false),
            layers,
            structure_overrides,
        })
    }
    /// FlatLevelGeneratorSettings.CODEC, as world_gen_settings stores it.
    pub fn to_tag(&self) -> Tag {
        let string = |text: &str| Tag::String(text.into());
        Tag::compound([
            ("biome", string(&self.biome)),
            ("features", Tag::Byte(i8::from(self.features))),
            ("lakes", Tag::Byte(i8::from(self.lakes))),
            (
                "layers",
                Tag::List(
                    self.layers
                        .iter()
                        .map(|layer| {
                            Tag::compound([
                                ("block", string(&layer.block)),
                                ("height", Tag::Int(layer.height)),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "structure_overrides",
                Tag::List(
                    self.structure_overrides
                        .iter()
                        .map(|id| string(id))
                        .collect(),
                ),
            ),
        ])
    }
    /// `CreateFlatWorldScreen.save`: the text the preset screen shows.
    pub fn to_share(&self) -> String {
        let layers = self
            .layers
            .iter()
            .map(|layer| {
                if layer.height == 1 {
                    layer.block.clone()
                } else {
                    format!("{}*{}", layer.height, layer.block)
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{layers};{}", self.biome)
    }
    /// `PresetFlatWorldScreen.fromString`: `layers;biome`, keeping the
    /// decorations the screen was opened with. `None` for a malformed list,
    /// as vanilla falls back to the preset's own settings.
    pub fn parse(
        text: &str,
        blocks: &BTreeSet<String>,
        biomes: &BTreeSet<String>,
        base: &Self,
    ) -> Option<Self> {
        let (layer_text, biome_text) = text.split_once(';').unwrap_or((text, ""));
        let mut layers = Vec::new();
        let mut total = 0;
        for part in layer_text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (height, block) = match part.split_once('*') {
                Some((height, block)) => (height.trim().parse::<i32>().ok()?, block.trim()),
                None => (1, part),
            };
            let block = namespaced(block);
            if !blocks.is_empty() && !blocks.contains(&block) {
                return None;
            }
            let height = height.max(0).min(Y_SIZE - total);
            if height == 0 {
                continue;
            }
            total += height;
            layers.push(FlatLayer { block, height });
        }
        if layers.is_empty() {
            return None;
        }
        let mut biome = namespaced(biome_text.trim());
        if biome == "minecraft:" || biome.is_empty() {
            biome = "minecraft:plains".into();
        }
        if !biomes.is_empty() && !biomes.contains(&biome) {
            biome = "minecraft:plains".into();
        }
        Some(Self {
            biome,
            layers,
            ..base.clone()
        })
    }
}
/// `Identifier.parse`: bare names get the default namespace.
fn namespaced(id: &str) -> String {
    if id.is_empty() || id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}
