//! The game side of piglins (`PiglinAi`): spotting and picking up gold,
//! bartering when the admiring is over, and anger at players who open
//! their chests or mine their gold.

impl crate::game::Game<'_> {
    /// `PiglinAi.isWearingSafeArmor`.
    pub(super) fn wearing_gold(&self) -> bool {
        let tag = self
            .res
            .items()
            .item_tags
            .get("minecraft:piglin_safe_armor");
        self.armor
            .iter()
            .flatten()
            .any(|s| tag.is_some_and(|t| t.contains(&*s.item)))
    }
}
