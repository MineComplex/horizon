//! `PlayerDataStorage`: `<world>/players/data/<uuid>.dat` (26.3's `LevelResource.PLAYER_DATA_DIR`;
//! the `playerdata/` folder of older versions is gone), gzip NBT, written through a temporary file
//! with the previous file kept as `<uuid>.dat_old` (`Util.safeReplaceFile`), read with `.dat_old`
//! as the fallback. Leaf's `async-playerdata-save` is off by default, so saving runs on the tick
//! thread as here. Spigot's offline-UUID fallback applies to online-mode servers only (a proxy
//! server runs offline), so it is not ported; neither is data fixing (26.3 files only).
//!
//! The player is not ported yet, so only the fields the join needs are typed ([`SavedPlayer`]);
//! saving writes them over the file's existing tag, so everything else in it is kept as it was.

use std::io;
use std::path::{Path, PathBuf};

use tracing::warn;
use uuid::Uuid;

use crate::nbt::{Compound, Tag};
use crate::network::play::clientbound::GameType;
use crate::world::chunk::serial::DATA_VERSION;
use crate::world::level_storage::{read_root, safe_replace, tempfile_in, write_gzip};

/// What the join reads from a saved player (`ServerPlayer.SavedPosition` and the game modes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedPlayer {
    /// `Dimension`.
    pub dimension: Option<String>,
    /// `Pos` (`Vec3.CODEC`: three doubles).
    pub position: Option<[f64; 3]>,
    /// `Rotation` (`Vec2.CODEC`: yaw, pitch).
    pub rotation: Option<(f32, f32)>,
    /// `playerGameType`, `previousPlayerGameType` (`GameType.LEGACY_ID_CODEC`).
    pub game_type: Option<GameType>,
    pub previous_game_type: Option<GameType>,
    /// The whole tag, written back with the fields above replaced.
    pub tag: Compound,
}

fn game_type(tag: Option<&Tag>) -> Option<GameType> {
    // `LEGACY_ID_CODEC`: an id outside 0..=3 is no game mode (`readPlayerMode` then gives null).
    match tag? {
        Tag::Int(0) => Some(GameType::Survival),
        Tag::Int(1) => Some(GameType::Creative),
        Tag::Int(2) => Some(GameType::Adventure),
        Tag::Int(3) => Some(GameType::Spectator),
        _ => None,
    }
}

impl SavedPlayer {
    pub fn parse(tag: Compound) -> Self {
        let get = |k: &str| tag.get(&k.into());
        let position = match get("Pos") {
            Some(Tag::List(l)) => match l.as_slice() {
                [Tag::Double(x), Tag::Double(y), Tag::Double(z)] => {
                    Some([x, y, z].map(|v| f64::from_bits(*v)))
                }
                _ => None,
            },
            _ => None,
        };
        let rotation = match get("Rotation") {
            Some(Tag::List(l)) => match l.as_slice() {
                [Tag::Float(y), Tag::Float(x)] => Some((f32::from_bits(*y), f32::from_bits(*x))),
                _ => None,
            },
            _ => None,
        };
        let dimension = match get("Dimension") {
            Some(Tag::String(s)) => crate::registry::parse_identifier(&s.to_string_lossy()).ok(),
            _ => None,
        };
        Self {
            dimension,
            position,
            rotation,
            game_type: game_type(get("playerGameType")),
            previous_game_type: game_type(get("previousPlayerGameType")),
            tag,
        }
    }

    /// `saveWithoutId` as far as it is ported: the typed fields over the loaded tag.
    pub fn to_tag(&self) -> Compound {
        let mut tag = self.tag.clone();
        let mut put = |k: &str, v: Tag| {
            tag.insert(k.into(), v);
        };
        if let Some(p) = self.position {
            put("Pos", Tag::List(p.map(|v| Tag::Double(v.to_bits())).into()));
        }
        if let Some((y, x)) = self.rotation {
            put(
                "Rotation",
                Tag::List(vec![Tag::Float(y.to_bits()), Tag::Float(x.to_bits())]),
            );
        }
        if let Some(d) = &self.dimension {
            put("Dimension", Tag::String(d.as_str().into()));
        }
        if let Some(g) = self.game_type {
            put("playerGameType", Tag::Int(g as i32));
        }
        if let Some(g) = self.previous_game_type {
            put("previousPlayerGameType", Tag::Int(g as i32));
        }
        put("DataVersion", Tag::Int(DATA_VERSION));
        tag
    }
}

/// The folder of a world's player files.
#[derive(Debug)]
pub struct PlayerDataStorage {
    dir: PathBuf,
}

impl PlayerDataStorage {
    /// `new PlayerDataStorage(access)`: the folder is created.
    pub fn new(level_dir: &Path) -> io::Result<Self> {
        let dir = level_dir.join("players").join("data");
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn path(&self, id: Uuid, suffix: &str) -> PathBuf {
        self.dir.join(format!("{id}{suffix}"))
    }

    /// `load(nameAndId)`: `.dat`, else `.dat_old`; a broken file is logged and skipped.
    pub fn load(&self, id: Uuid, name: &str) -> Option<SavedPlayer> {
        [".dat", ".dat_old"]
            .iter()
            .find_map(|suffix| {
                let path = self.path(id, suffix);
                if !path.is_file() {
                    return None;
                }
                read_root(&path)
                    .inspect_err(|_| warn!("Failed to load player data for {name}"))
                    .ok()
            })
            .map(SavedPlayer::parse)
    }

    /// `save(player)`: a temporary `<uuid>-*.dat`, then `.dat` becomes `.dat_old` and the new file
    /// `.dat`. Failures are logged, as Paper does.
    pub fn save(&self, id: Uuid, name: &str, player: &SavedPlayer) {
        let result = tempfile_in(&self.dir, &format!("{id}-"), ".dat").and_then(|temp| {
            let written = write_gzip(&temp, player.to_tag()).and_then(|()| {
                safe_replace(
                    &self.path(id, ".dat"),
                    &temp,
                    &self.path(id, ".dat_old"),
                    false,
                )
            });
            if written.is_err() {
                let _ = std::fs::remove_file(&temp);
            }
            written
        });
        if let Err(e) = result {
            warn!("Failed to save player data for {name}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_over_the_existing_tag_and_keeps_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = PlayerDataStorage::new(dir.path()).unwrap();
        let id = Uuid::from_u128(7);
        assert_eq!(storage.load(id, "Notch"), None);
        let mut tag = Compound::new();
        tag.insert("Health".into(), Tag::Float(20f32.to_bits()));
        tag.insert("playerGameType".into(), Tag::Int(9));
        let mut player = SavedPlayer::parse(tag);
        assert_eq!(player.game_type, None);
        player.position = Some([1.5, 64.0, -3.25]);
        player.rotation = Some((90.0, -10.0));
        player.dimension = Some("minecraft:overworld".into());
        player.game_type = Some(GameType::Creative);
        storage.save(id, "Notch", &player);
        let loaded = storage.load(id, "Notch").unwrap();
        assert_eq!(loaded.position, Some([1.5, 64.0, -3.25]));
        assert_eq!(loaded.rotation, Some((90.0, -10.0)));
        assert_eq!(loaded.dimension.as_deref(), Some("minecraft:overworld"));
        assert_eq!(loaded.game_type, Some(GameType::Creative));
        assert_eq!(
            loaded.tag.get(&"Health".into()),
            Some(&Tag::Float(20f32.to_bits()))
        );
        storage.save(id, "Notch", &loaded);
        assert!(storage.path(id, ".dat_old").is_file());
        // A broken `.dat` falls back to `.dat_old`.
        std::fs::write(storage.path(id, ".dat"), b"broken").unwrap();
        assert_eq!(
            storage.load(id, "Notch").unwrap().position,
            Some([1.5, 64.0, -3.25])
        );
    }
}
