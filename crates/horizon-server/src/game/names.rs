//! Custom names: `NameTagItem.interactLivingEntity` and the name tag
//! (`EntityRenderer.submitNameTag`): above the head, facing the camera,
//! on a translucent black background, shown when the name is visible or the
//! mob is under the crosshair within 64 blocks, as its team's name tag
//! visibility allows, in the team's colours, with its score in the
//! objective displayed below names. Other players' names (on a LAN host
//! or a server) show whenever `LivingEntityRenderer.shouldShowName` allows.

impl crate::game::Sim<'_> {
    /// A named name tag names the mob under the crosshair.
    pub(super) fn name_mob(&mut self, index: usize) -> bool {
        let Some(name) = self
            .held()
            .filter(|h| &*h.item == "minecraft:name_tag")
            .and_then(|h| h.custom_name().map(str::to_owned))
        else {
            return false;
        };
        let m = &mut self.mobs[index];
        if !m.alive() {
            return false;
        }
        m.custom_name = Some(name);
        m.persistent = true;
        // itemStack.shrink(1), in creative too.
        self.consume_held();
        self.swing_arm();
        true
    }
}
