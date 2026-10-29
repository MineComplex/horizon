//! Custom names: `NameTagItem.interactLivingEntity` and the name tag
//! (`EntityRenderer.submitNameTag`): above the head, facing the camera,
//! on a translucent black background, shown when the name is visible or the
//! mob is under the crosshair within 64 blocks, as its team's name tag
//! visibility allows, in the team's colours, with its score in the
//! objective displayed below names. Other players' names (on a LAN host
//! or a server) show whenever `LivingEntityRenderer.shouldShowName` allows.
use crate::hc::{mobs::Mob, scoreboard as sb, text::Text};

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
    /// `Entity.belowNameDisplay`: the score holder's score in the objective
    /// shown below names, then a space and the objective's name.
    fn below_name_text(&self, holder: &str) -> Option<Text> {
        let board = &self.score.board;
        let objective = board.display_objective(sb::SLOT_BELOW_NAME)?;
        let score = board.score(holder, &objective.name)?;
        // numberFormatOrDefault(StyledFormat.NO_STYLE).
        let format = score
            .number_format
            .clone()
            .or_else(|| objective.number_format.clone())
            .unwrap_or(sb::NumberFormat::Styled(serde_json::json!({})));
        let mut t = Text::empty();
        t.push(format.format(score.value));
        t.push(Text::literal(" "));
        t.push(objective.display_text());
        Some(t)
    }
    /// The other players' models and names: the guests on a LAN host, the
    /// players a server sends on a client.
    fn other_players(&self) -> Vec<(&Mob, String)> {
        let mut out = Vec::new();
        if let Some(host) = &self.lan {
            for (conn, m) in &host.models {
                if let Some(g) = host.hub.guest(*conn) {
                    out.push((m, g.name.clone()));
                }
            }
        }
        out
    }
}

/// A name tag: where it hangs, its lines (text, lift), the packed light at
/// the entity and whether it is discrete.
struct NameTag {
    at: glam::DVec3,
    lines: Vec<(Text, f64)>,
    light: u8,
    discrete: bool,
}
