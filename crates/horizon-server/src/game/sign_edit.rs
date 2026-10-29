//! The sign editing screen (`SignEditScreen` / `HangingSignEditScreen`):
//! opened when a sign is placed, four lines limited to the sign's line
//! width, Up/Down/Enter move between lines, Escape or Done closes.
use super::server_net::Network;
use crate::hc::{containers::BlockEntity, items::ItemStack, signs::DYE_TEXT};
use horizon_protocol::packets::play::{self, Clientbound as C};

pub(super) struct SignEditor {
    pub pos: (i32, i32, i32),
    pub front: bool,
}

impl crate::game::Server<'_> {
    /// `SignBlockEntity.getSlotPlayerIsFacing`: whether the player stands in
    /// front of the sign's text (within 90 degrees of its facing).
    fn facing_sign_front(&self, pos: (i32, i32, i32)) -> bool {
        let Some(player) = self.player.as_ref() else {
            return true;
        };
        let state = self.world.block(pos.0, pos.1, pos.2);
        let Some(state) = self.pack.assets.registry.state(state) else {
            return true;
        };
        let prop = |key: &str| state.properties.get(key).map(String::as_str);
        // getYRotationDegrees: a wall sign's `Direction.toYRot`, a standing or
        // ceiling sign's rotation segment.
        let y_rot = if state.name.contains("_wall_") {
            match prop("facing") {
                Some("west") => 90.0,
                Some("north") => 180.0,
                Some("east") => 270.0,
                _ => 0.0,
            }
        } else {
            prop("rotation")
                .and_then(|r| r.parse::<f32>().ok())
                .unwrap_or(0.0)
                * 22.5
        };
        // getSignHitboxCenterPosition: only a wall sign's board is off-centre,
        // against the wall it hangs on.
        let centre = if state.name.ends_with("_wall_sign") {
            match y_rot as i32 {
                90 => (0.9375, 0.5),
                180 => (0.5, 0.9375),
                270 => (0.0625, 0.5),
                _ => (0.5, 0.0625),
            }
        } else {
            (0.5, 0.5)
        };
        let xd = player.position.x - (f64::from(pos.0) + centre.0);
        let zd = player.position.z - (f64::from(pos.2) + centre.1);
        let player_y_rot = (zd.atan2(xd).to_degrees() - 90.0) as f32;
        // Mth.degreesDifferenceAbs.
        let diff = (player_y_rot - y_rot).rem_euclid(360.0);
        let diff = if diff >= 180.0 { 360.0 - diff } else { diff };
        diff <= 90.0
    }
    /// `SignBlock.useWithoutItem` (the block's `OpenMenu` effect): a waxed
    /// sign plays the failed-interaction sound; otherwise a player who may
    /// build edits the side they face.
    pub(super) fn use_sign_empty_hand(&mut self, pos: [i32; 3]) {
        let key = (pos[0], pos[1], pos[2]);
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&key) else {
            return;
        };
        if sign.waxed {
            // `getSignInteractionFailedSoundEvent`: a hanging sign has its own.
            let sound = if sign.hanging {
                "minecraft:block.hanging_sign.waxed_interact_fail"
            } else {
                "minecraft:block.sign.waxed_interact_fail"
            };
            self.sounds
                .play(sound, Some(pos.map(|v| f64::from(v) + 0.5)), 1.0, 1.0);
        } else if self.may_build() {
            let front = self.facing_sign_front(key);
            self.open_sign_editor(key, front);
        }
    }
    /// `SignBlock.useItemOn` with a `SignApplicator` in hand (dyes, ink sacs,
    /// honeycomb): the item changes the side the player faces and is used up.
    /// Whether it did; a sign it does not apply to goes on to the empty hand.
    pub(super) fn use_sign_applicator(&mut self, pos: [i32; 3], held: Option<&ItemStack>) -> bool {
        let Some(stack) = held else {
            return false;
        };
        let key = (pos[0], pos[1], pos[2]);
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get(&key) else {
            return false;
        };
        if sign.waxed || !self.may_build() {
            return false;
        }
        let item = &*stack.item;
        let dye = if item.ends_with("_dye") {
            self.res
                .items()
                .get(&stack.item)
                .and_then(|i| i.dye.clone())
        } else {
            None
        };
        let colour = dye.and_then(|d| DYE_TEXT.iter().position(|(n, _)| *n == d));
        if !matches!(
            item,
            "minecraft:honeycomb" | "minecraft:glow_ink_sac" | "minecraft:ink_sac"
        ) && colour.is_none()
        {
            return false;
        }
        let front = self.facing_sign_front(key);
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get_mut(&key) else {
            return false;
        };
        let centre = pos.map(|v| f64::from(v) + 0.5);
        let sound = if item == "minecraft:honeycomb" {
            // HoneycombItem.tryApplyToSign: waxes any sign, empty or not.
            sign.waxed = true;
            "minecraft:item.honeycomb.wax_on"
        } else {
            let side = if front {
                &mut sign.front
            } else {
                &mut sign.back
            };
            // SignApplicator.canApplyToSign: the side must have text.
            if side.is_empty() {
                return false;
            }
            match colour {
                Some(c) if side.color != c as u8 => {
                    side.color = c as u8;
                    "minecraft:item.dye.use"
                }
                None if item == "minecraft:glow_ink_sac" && !side.glowing => {
                    side.glowing = true;
                    "minecraft:item.glow_ink_sac.use"
                }
                None if item == "minecraft:ink_sac" && side.glowing => {
                    side.glowing = false;
                    "minecraft:item.ink_sac.use"
                }
                _ => return false,
            }
        };
        if item == "minecraft:honeycomb" {
            self.level_event_at(3003, pos, 0);
        }
        self.sounds.play(sound, Some(centre), 1.0, 1.0);
        self.mark_block_entity_updated(key);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        self.post_block_game_event(
            "minecraft:block_change",
            pos,
            self.sculk_actor(),
            Some(state),
        );
        self.stats.add("used", &stack.item, 1);
        self.consume_held();
        self.trigger_block("minecraft:item_used_on_block", pos, stack);
        true
    }
    /// `ServerPlayer.openTextEdit` for the player whose action opened a sign:
    /// the block, then the editor, on its client, and the player may write on it.
    pub(super) fn net_open_sign_editor(&mut self, net: &mut Network, conn: u32, edit: SignEditor) {
        let pos = [edit.pos.0, edit.pos.1, edit.pos.2];
        let state = self.world.block(pos[0], pos[1], pos[2]).0 as i32;
        net.hub
            .send_to(conn, &C::BlockUpdate(play::BlockUpdate { pos, state }));
        net.hub.send_to(
            conn,
            &C::OpenSignEditor(play::OpenSignEditor {
                pos,
                front: edit.front,
            }),
        );
        net.play.entry(conn).or_default().sign_editing = Some(pos);
    }
    /// `ServerGamePacketListenerImpl.handleSignUpdate`: the lines, formatting
    /// codes stripped, go on the sign the player may write on unless it is
    /// waxed (`SignBlockEntity.updateSignText`); a player gone out of reach
    /// may no longer write (`playerIsTooFarAwayToEdit`).
    pub(super) fn net_sign_update(&mut self, net: &mut Network, conn: u32, p: play::SignUpdate) {
        let Some(g) = net.hub.player(conn) else {
            return;
        };
        let near = g.within_block_range(p.pos, 4.0);
        let Some(play) = net.play.get_mut(&conn) else {
            return;
        };
        if !near {
            play.sign_editing = None;
        }
        if play.sign_editing != Some(p.pos) {
            log::warn!("{} just tried to change non-editable sign", g.name);
            return;
        }
        let key = (p.pos[0], p.pos[1], p.pos[2]);
        let Some(BlockEntity::Sign(sign)) = self.world.block_entities.get_mut(&key) else {
            return;
        };
        if sign.waxed {
            return;
        }
        let side = if p.front {
            &mut sign.front
        } else {
            &mut sign.back
        };
        for (line, new) in side.lines.iter_mut().zip(&p.lines) {
            *line = strip_formatting(new);
        }
        play.sign_editing = None;
        self.mark_block_entity_updated(key);
    }
}

/// `ChatFormatting.stripFormatting`: a sign line as a client may write it.
fn strip_formatting(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '§'
            && chars.peek().is_some_and(
                |n| matches!(n.to_ascii_lowercase(), '0'..='9' | 'a'..='f' | 'k'..='o' | 'r'),
            )
        {
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

impl crate::game::Server<'_> {
    /// `SignBlockEntity.openTextEdit`'s turn: the player edits this side (the
    /// server then opens the editor on their client, `net_open_sign_editor`).
    pub(super) fn open_sign_editor(&mut self, pos: (i32, i32, i32), front: bool) {
        let _lines = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Sign(sign)) if front => sign.front.lines.clone(),
            Some(BlockEntity::Sign(sign)) => sign.back.lines.clone(),
            _ => Default::default(),
        };
        self.sign_edit = Some(SignEditor { pos, front });
    }
}
