//! Players' saved state: `players/data/<uuid>.dat` in the vanilla player
//! compound, through the same code that saves the host.
use super::player::{SavedBody, ServerPlayer};
use crate::{
    items::ItemStack,
    save::{Level, read_dat, write_dat},
};
use horizon_protocol::crypto::uuid_string;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn path(world: &Path, uuid: u128) -> PathBuf {
    world
        .join("players")
        .join("data")
        .join(format!("{}.dat", uuid_string(uuid)))
}

/// `UUIDUtil.uuidToIntArray`.
pub fn uuid_ints(uuid: u128) -> [i32; 4] {
    [
        (uuid >> 96) as i32,
        (uuid >> 64) as i32,
        (uuid >> 32) as i32,
        uuid as i32,
    ]
}
/// `UUIDUtil.uuidFromIntArray`.
pub fn ints_uuid(ints: [i32; 4]) -> u128 {
    ints.iter()
        .fold(0, |acc, &i| acc << 32 | u128::from(i as u32))
}

/// Whether a saved file exists for this player.
pub fn exists(world: &Path, uuid: u128) -> bool {
    path(world, uuid).is_file()
}

/// Load a player's saved state, if they have been here before.
pub fn load(player: &mut ServerPlayer, world: &Path) -> bool {
    let Some(tag) = read_dat(&path(world, player.uuid)) else {
        return false;
    };
    let mut level = Level {
        health: 20.0,
        food: 20,
        saturation: 5.0,
        ..Level::default()
    };
    level.read_player(&tag);
    level.player_extra = Some(tag);
    apply(player, &level);
    true
}

/// Give a player the player state read into `level`.
pub fn apply(player: &mut ServerPlayer, level: &Level) {
    if let Some(p) = level.position {
        player.position = p;
        player.previous_position = p;
    }
    player.yaw = level.rotation[0];
    player.pitch = level.rotation[1];
    player.head_yaw = player.yaw;
    player.on_ground = level.on_ground;
    player.health = level.health;
    player.saved_absorption = Some(level.absorption);
    player.food = level.food;
    player.saturation = level.saturation;
    player.xp_level = level.xp_level;
    player.xp_progress = level.xp_progress;
    player.xp_total = level.xp_total;
    player.xp_seed = level.xp_seed;
    player.effects = crate::effects::PlayerEffects::from_saved(&level.active_effects);
    player.game_mode = level.mode();
    player.flying = level.flying;
    player.selected = (level.selected_slot.max(0) as usize).min(8);
    // A point that is the world spawn is none of the player's own: it
    // respawns wherever the world spawn is then (`setworldspawn`).
    player.spawn = level
        .spawn
        .filter(|s| Some(s.map(|v| v.floor() as i32)) != level.world_spawn)
        .map(|s| [s[0] + 0.5, s[1], s[2] + 0.5]);
    player.respawn_elsewhere = level.respawn_elsewhere.clone();
    player.saved = level.player_extra.clone();
    player.seen_credits = level.seen_credits;
    player.last_death = level.last_death.clone();
    let extra = level.player_extra.as_ref();
    let short = |key: &str, default: i64| {
        extra
            .and_then(|t| crate::save::number(t.get(key)))
            .unwrap_or(default) as i32
    };
    player.loaded_body = Some(SavedBody {
        fire_ticks: short("Fire", -20),
        air: short("Air", 300),
        fall_distance: level.fall_distance,
    });
    player.recipe_book = level.recipe_book.clone();
    player.dimension = match level.dimension.as_str() {
        d @ ("minecraft:the_nether" | "minecraft:the_end") => d.to_owned(),
        _ => "minecraft:overworld".to_owned(),
    };
    if level.ender_items.len() == 27 {
        player.ender_items = level.ender_items.clone();
    }
    for (slot, item, count, damage) in &level.inventory {
        let mut stack = ItemStack {
            item: Arc::from(item.as_str()),
            count: *count,
            damage: *damage,
            extra: level.inventory_components.get(slot).cloned(),
        };
        if stack.count == 0 {
            stack.count = 1;
        }
        match *slot {
            0..=35 => player.inventory[usize::from(*slot)] = Some(stack),
            100 => player.armor[3] = Some(stack),
            101 => player.armor[2] = Some(stack),
            102 => player.armor[1] = Some(stack),
            103 => player.armor[0] = Some(stack),
            150 => player.offhand = Some(stack),
            _ => {}
        }
    }
}

/// Save a player (PlayerDataStorage.save).
pub fn save(player: &ServerPlayer, world: &Path) {
    let mut level = Level {
        player_uuid: Some(uuid_ints(player.uuid)),
        ..Level::default()
    };
    store(player, &mut level);
    let file = path(world, player.uuid);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = write_dat(&file, level.player_tag());
}

/// Put a player's state in `level`'s player fields.
pub fn store(player: &ServerPlayer, level: &mut Level) {
    level.position = Some(player.position);
    level.dimension = player.dimension.clone();
    level.rotation = [player.yaw, player.pitch];
    level.health = player.health;
    level.absorption = player
        .motion
        .as_ref()
        .map_or(player.saved_absorption.unwrap_or(0.0), |m| {
            m.health.absorption()
        });
    level.food = player.food;
    level.saturation = player.saturation;
    level.xp_level = player.xp_level;
    level.xp_progress = player.xp_progress;
    level.xp_total = player.xp_total;
    level.xp_seed = player.xp_seed;
    level.active_effects = player.effects.saved();
    level.game_mode = player.game_mode;
    level.creative = player.game_mode == 1;
    level.flying = player.flying;
    level.selected_slot = player.selected as i32;
    level.spawn = player.spawn.map(|s| [s[0] - 0.5, s[1], s[2] - 0.5]);
    level.respawn_elsewhere = player.respawn_elsewhere.clone();
    level.on_ground = player.on_ground;
    level.seen_credits = player.seen_credits;
    level.last_death = player.last_death.clone();
    level.player_extra = player.saved.clone();
    // The live body's fire, air and fall distance (Entity.saveWithoutId),
    // or what the file had while the player has no body yet.
    let body = player
        .motion
        .as_deref()
        .map(SavedBody::of)
        .or(player.loaded_body);
    if let Some(body) = body {
        level.fall_distance = body.fall_distance;
        let extra = level
            .player_extra
            .get_or_insert_with(|| horizon_world::nbt::Tag::Compound(Default::default()));
        if let horizon_world::nbt::Tag::Compound(map) = extra {
            let short = |v: i32| horizon_world::nbt::Tag::Short(v.clamp(-32768, 32767) as i16);
            map.insert("Fire".into(), short(body.fire_ticks));
            map.insert("Air".into(), short(body.air));
        }
    }
    level.recipe_book = player.recipe_book.clone();
    level.ender_items = player.ender_items.clone();
    level.inventory.clear();
    level.inventory_components.clear();
    let mut put = |slot: u8, stack: &Option<ItemStack>| {
        if let Some(s) = stack {
            level
                .inventory
                .push((slot, s.item.to_string(), s.count, s.damage));
            if let Some(extra) = &s.extra {
                level.inventory_components.insert(slot, extra.clone());
            }
        }
    };
    for (i, s) in player.inventory.iter().enumerate() {
        put(i as u8, s);
    }
    put(103, &player.armor[0]);
    put(102, &player.armor[1]);
    put(101, &player.armor[2]);
    put(100, &player.armor[3]);
    put(150, &player.offhand);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_net::{Joined, PlayerSender};
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::{Arc, atomic::AtomicI32, mpsc};

    fn player() -> ServerPlayer {
        let (tx, _rx) = mpsc::channel();
        ServerPlayer::new(
            Joined {
                conn: 2,
                uuid: 2,
                name: "Alex".into(),
                properties: Vec::new(),
                info: ClientInformation::new(8),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
            },
            2,
            [0.0, 64.0, 0.0],
            8,
        )
    }

    #[test]
    fn a_saved_burn_and_fall_come_back() {
        // A player saved burning in mid-fall (#1323, #1327): Entity.saveWithoutId's
        // Fire, Air and fall_distance, read back for the next body.
        let mut g = player();
        g.loaded_body = Some(SavedBody {
            fire_ticks: 96,
            air: 250,
            fall_distance: 9.5,
        });
        let mut level = Level::default();
        store(&g, &mut level);
        let tag = level.player_tag();
        let mut read = Level::default();
        read.read_player(&tag);
        read.player_extra = Some(tag);
        let mut back = player();
        apply(&mut back, &read);
        assert_eq!(back.loaded_body, g.loaded_body);
    }
}
