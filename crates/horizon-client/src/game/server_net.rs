//! `horizon-ctl lan ...`, answered by the test's server, and the entity data
//! indices the client reads a mob's baby flag from.
use horizon_client::Result;

/// The SynchedEntityData index of the baby flag: AgeableMob.DATA_BABY_ID and
/// Zombie.DATA_BABY_ID are both the first entry after Mob's (16); Piglin's
/// comes after AbstractPiglin's zombification flag (17).
pub(super) fn baby_index(kind: &str) -> Option<u8> {
    match kind {
        "minecraft:piglin" => Some(17),
        "minecraft:cow"
        | "minecraft:mooshroom"
        | "minecraft:pig"
        | "minecraft:sheep"
        | "minecraft:chicken"
        | "minecraft:horse"
        | "minecraft:donkey"
        | "minecraft:mule"
        | "minecraft:skeleton_horse"
        | "minecraft:zombie_horse"
        | "minecraft:llama"
        | "minecraft:trader_llama"
        | "minecraft:cat"
        | "minecraft:wolf"
        | "minecraft:fox"
        | "minecraft:rabbit"
        | "minecraft:panda"
        | "minecraft:polar_bear"
        | "minecraft:bee"
        | "minecraft:frog"
        | "minecraft:sniffer"
        | "minecraft:armadillo"
        | "minecraft:villager"
        | "minecraft:zombie"
        | "minecraft:husk"
        | "minecraft:drowned"
        | "minecraft:zombie_villager"
        | "minecraft:zombified_piglin"
        | "minecraft:hoglin"
        | "minecraft:zoglin"
        | "minecraft:goat"
        | "minecraft:ocelot"
        | "minecraft:turtle"
        | "minecraft:strider"
        | "minecraft:camel"
        | "minecraft:axolotl"
        | "minecraft:dolphin"
        | "minecraft:squid"
        | "minecraft:glow_squid" => Some(16),
        _ => None,
    }
}

pub const HELP: &[&str] = &[
    "network open [PORT] [commands] [force]  the test server's port; commands lets every player run commands",
    "network status                     the test server's port and players",
];

impl crate::game::Game<'_> {
    /// `horizon-ctl lan ...`: a test's client asks its server
    /// (`--server-control`), which is always open, on its own port.
    pub(super) fn net_control(&mut self, words: &[&str]) -> Result<Option<serde_json::Value>> {
        if words[0] != "network" {
            return Ok(None);
        }
        match self.ask_server(&words.join(" ")) {
            Some(reply) => reply.map(Some),
            None => {
                Err("network: this client has no server control endpoint (--server-control)".into())
            }
        }
    }
    /// `Level.blockEvent` for the players (`ClientboundBlockEventPacket`).
    pub(super) fn net_block_event(&mut self, pos: [i32; 3], b0: i32, b1: i32) {
        if self.sounds.capture.is_some() {
            let state = self.world.block(pos[0], pos[1], pos[2]).0;
            self.net_block_events.push((pos, b0, b1, state));
        }
    }
    /// The bed at `head` as a sleeping model lies in it (`ints[1]`,
    /// `villager::sleep_pose`): its `facing`.
    pub(super) fn bed_facing(&self, head: [i32; 3]) -> i32 {
        let state = self.world.block(head[0], head[1], head[2]);
        let facing = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("facing"))
            .map(String::as_str);
        horizon_client::mobs::villager::bed_facing_code(facing)
    }
}
