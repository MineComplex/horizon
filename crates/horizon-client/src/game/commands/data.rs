//! An entity's data as the client has it, for F3 + I's copied command
//! (`KeyboardHandler.copyRecreateCommand` without the server's data).
use horizon_world::nbt::Tag;

impl crate::game::Sim<'_> {
    /// The mob as the client has it, riders under `Passengers` (`saveWithoutId`).
    pub(in crate::game) fn entity_nbt(&mut self, id: u32) -> Option<Tag> {
        let i = self.mobs.iter().position(|m| m.id == id)?;
        Some(self.mob_tree_nbt(i))
    }
    pub(in crate::game) fn data_block_name(&self, p: [i32; 3]) -> String {
        let state = self.world.block(p[0], p[1], p[2]);
        self.pack
            .assets
            .registry
            .state(state)
            .map_or_else(|| "minecraft:air".into(), |s| s.name.to_string())
    }
}
