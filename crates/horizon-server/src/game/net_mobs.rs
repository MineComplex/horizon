//! Mobs and the players of an open world. The mob code sees one player
//! (`MobWorld.player`, `Who::Player`); with players on, each mob sees the
//! player vanilla's `Level.getNearestPlayer` would give it, and keeps the
//! player it targets until it lets go. What it then does to `Who::Player`
//! (hits, pushes, fire, its projectiles) runs with that player swapped in.
use super::server_net::Network;
use crate::hc::{
    mobs::{Mob, MobEvent, PlayerView, Species, Who},
    projectiles::{Hit, Projectile},
};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// The players mobs can see this tick, by connection.
pub(super) type PlayerViews = Vec<(u32, PlayerView)>;

/// The player a mob sees this tick: `None` for the host, else a player.
/// A mob that targets a player, or that a player hurt in the last 100 ticks
/// (`LivingEntity.getLastHurtByMob`), keeps them while they are still a
/// valid target; otherwise it sees the nearest player
/// (`TargetingConditions`: alive, neither creative nor spectator), or the
/// nearest one that is not a spectator if none qualifies.
pub(super) fn player_for(
    m: &Mob,
    net: Option<&PlayerView>,
    players: &PlayerViews,
    bound: Option<u32>,
) -> Option<u32> {
    // A tame cat reads its owner wherever they are while they sleep, or just
    // woke up (`CatRelaxOnOwnerGoal` uses `getOwner`, not the nearest player).
    if m.species() == Species::Cat
        && m.extra.tame
        && let Some(owner) = m.extra.owner
    {
        let asleep = |v: &PlayerView| v.uuid == owner && (v.sleeping || v.sleep_timer > 0);
        if net.is_some_and(asleep) {
            return None;
        }
        if let Some((conn, _)) = players.iter().find(|(_, v)| asleep(v)) {
            return Some(*conn);
        }
    }
    let targeting = m.ai.target == Some(Who::Player)
        || m.ai.last_hurt_by == Some(Who::Player) && m.age as i32 - m.ai.last_hurt_by_time <= 100;
    // A tame wolf, cat or parrot sees its owner (`TamableAnimal.getOwner`), while they
    // are in play, whoever is nearer. Not while it targets or was just hurt by
    // a player: then `Who::Player` is that attacker, whoever it is, and the
    // owner goals ignore it until the wolf lets go.
    if !targeting
        && matches!(m.species(), Species::Wolf | Species::Cat | Species::Parrot)
        && m.extra.tame
        && let Some(owner) = m.extra.owner
    {
        if net.is_some_and(|v| v.alive && v.uuid == owner) {
            return None;
        }
        if let Some((conn, _)) = players.iter().find(|(_, v)| v.alive && v.uuid == owner) {
            return Some(*conn);
        }
    }
    choose(m.position, targeting, net, players, bound)
}

fn choose(
    at: Vec3,
    targeting: bool,
    net: Option<&PlayerView>,
    players: &PlayerViews,
    bound: Option<u32>,
) -> Option<u32> {
    let targetable = |v: &PlayerView| v.alive && !v.creative && !v.spectator;
    if targeting
        && let Some(conn) = bound
        && players.iter().any(|(c, v)| *c == conn && targetable(v))
    {
        return Some(conn);
    }
    let d = |v: &PlayerView| at.distance_squared(v.position);
    let nearest = |ok: &dyn Fn(&PlayerView) -> bool| {
        let mut best = net.filter(|v| ok(v)).map(|v| (None, d(v)));
        for (conn, v) in players.iter().filter(|(_, v)| ok(v)) {
            if best.is_none_or(|(_, b)| d(v) < b) {
                best = Some((Some(*conn), d(v)));
            }
        }
        best
    };
    nearest(&targetable)
        .or_else(|| nearest(&|v: &PlayerView| v.alive && !v.spectator))
        .and_then(|(conn, _)| conn)
}

/// The events of a mob's tick that act on the player it sees.
pub(super) fn acts_on_player(e: &MobEvent) -> bool {
    matches!(
        e,
        MobEvent::AttackPlayer(..)
            | MobEvent::AttackPlayerWithEffect { .. }
            | MobEvent::DamagePlayer { .. }
            | MobEvent::SonicBoom {
                target: Who::Player,
                ..
            }
            | MobEvent::Push {
                target: Who::Player,
                ..
            }
            | MobEvent::Knockback {
                target: Who::Player,
                ..
            }
            | MobEvent::Ignite {
                target: Who::Player,
                ..
            }
            | MobEvent::Launch {
                target: Who::Player,
                ..
            }
            | MobEvent::EffectPlayer { .. }
            | MobEvent::AxolotlSupportPlayer
            | MobEvent::ParrotToShoulder { .. }
    )
}

impl crate::game::Sim<'_> {
    /// How mobs see each player in play (`player_view` with the player swapped
    /// in; what the host alone tracks, such as its last attacker, stays out).
    pub(super) fn net_player_views(&mut self) -> PlayerViews {
        let Some(mut net) = self.network.take() else {
            return Vec::new();
        };
        let conns: Vec<(u32, u128, [f64; 3])> = net
            .hub
            .players
            .iter()
            .filter(|g| net.hub.here(g) && g.loaded && !g.dead)
            .map(|g| {
                let moved = [0, 1, 2].map(|k| g.position[k] - g.previous_position[k]);
                (g.conn, g.uuid, moved)
            })
            .collect();
        let mut views = Vec::new();
        for (conn, uuid, moved) in conns {
            if let Some(Some(mut v)) = self.as_player(&mut net, conn, |g| g.player_view()) {
                v.hurt_by = None;
                v.last_hurt = None;
                v.hero = false;
                v.velocity = Vec3::new(moved[0], moved[1], moved[2]);
                v.uuid = crate::hc::server_net::playerdata::uuid_ints(uuid);
                v.allies = Default::default();
                v.remote = true;
                views.push((conn, v));
            }
        }
        self.network = Some(net);
        views
    }
    /// Runs `f` as each player in play here, for what strikes every player in
    /// an area (`Level.getEntities`), not only the window's own.
    pub(super) fn each_player_hit(&mut self, f: impl FnMut(&mut Self)) {
        self.each_player_hit_except(None, f);
    }
    /// [`Self::each_guest_hit`] leaving out the player `except` (the one a
    /// firework rocket is attached to, which it hurts on its own).
    pub(super) fn each_player_hit_except(
        &mut self,
        except: Option<u32>,
        mut f: impl FnMut(&mut Self),
    ) {
        let Some(mut net) = self.network.take() else {
            return;
        };
        let conns: Vec<u32> = net
            .hub
            .players
            .iter()
            .filter(|g| net.hub.here(g) && g.loaded && !g.dead && Some(g.conn) != except)
            .map(|g| g.conn)
            .collect();
        for conn in conns {
            self.player_hit_by(&mut net, conn, &mut f);
        }
        self.network = Some(net);
    }
    /// Mob events aimed at a player, run as that player (`player_hit_by`).
    pub(super) fn net_player_mob_events(&mut self, events: Vec<(u32, MobEvent)>) {
        if events.is_empty() {
            return;
        }
        let Some(mut net) = self.network.take() else {
            return;
        };
        for (conn, e) in events {
            self.player_hit_by(&mut net, conn, |g| {
                let _ = g.handle_mob_events(vec![e]);
            });
        }
        self.network = Some(net);
    }
    /// The players' boxes for the blocks entities trigger (pressure plates,
    /// tripwires, detector rails): `Entity.checkInsideBlocks` and
    /// `Level.getEntities` see every player in the level, but not spectators
    /// (`Entity.isIgnoringBlockTriggers`).
    pub(super) fn net_block_trigger_boxes(&self) -> Vec<([f64; 3], [f64; 3])> {
        let Some(net) = self.network.as_ref() else {
            return Vec::new();
        };
        net.hub
            .players
            .iter()
            .filter(|g| net.hub.here(g) && g.loaded && !g.dead && !g.spectator())
            .map(|g| {
                let b = g.bounds();
                let (lo, hi) = (b.min_position(), b.max_position());
                ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z])
            })
            .collect()
    }
    /// The same players' feet and whether they stand on the ground, for the
    /// blocks that also ask that (`BigDripleafBlock.canEntityTilt`).
    pub(super) fn net_player_feet(&self) -> Vec<([f64; 3], bool)> {
        let Some(net) = self.network.as_ref() else {
            return Vec::new();
        };
        net.hub
            .players
            .iter()
            .filter(|g| net.hub.here(g) && g.loaded && !g.dead && !g.spectator())
            .map(|g| (g.position, g.on_ground))
            .collect()
    }
    /// The players a projectile can hit, under their entity ids.
    pub(super) fn net_projectile_targets(&self) -> Vec<(Who, Aabb)> {
        let Some(net) = self.network.as_ref() else {
            return Vec::new();
        };
        net.hub
            .players
            .iter()
            .filter(|g| net.hub.here(g) && g.loaded && !g.dead && !g.spectator())
            .map(|g| (Who::Mob(g.entity_id as u32), player_bounds(g)))
            .collect()
    }
    /// A projectile that hit a player: its hit runs as theirs. False when
    /// `id` is not a player.
    pub(super) fn net_projectile_hit(&mut self, p: &Projectile, id: u32, at: Vec3) -> bool {
        let Some(mut net) = self.network.take() else {
            return false;
        };
        let conn = net
            .hub
            .players
            .iter()
            .find(|g| g.entity_id as u32 == id)
            .map(|g| g.conn);
        if let Some(conn) = conn {
            self.player_hit_by(&mut net, conn, |g| {
                let hit = Hit::Entity {
                    who: Who::Player,
                    at,
                };
                let _ = g.projectile_hit(p, hit);
            });
        }
        self.network = Some(net);
        conn.is_some()
    }
}

impl crate::game::Sim<'_> {
    /// Hurt a player: `f` runs as them, and the knockback it gave reaches
    /// their client. A hurt marks the velocity (Entity.markHurt), which goes
    /// out once the player's server copy has travelled (`player_server_tick`);
    /// a push with no hurt goes out at once.
    pub(super) fn player_hit_by<R>(
        &mut self,
        net: &mut Network,
        conn: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let (result, pushed) = self.as_player(net, conn, |g| {
            let before = g.player.as_ref().map(|p| p.velocity);
            let result = f(g);
            let pushed = g
                .player
                .as_ref()
                .filter(|p| !p.health.sync_velocity && Some(p.velocity) != before)
                .map(|p| p.velocity);
            (result, pushed)
        })?;
        self.send_player_motion(net, conn, pushed);
        Some(result)
    }
    /// A player's melee hit on a player (Player.causeExtraKnockback on a
    /// ServerPlayer): a hurt that marked the velocity sends it at once, and
    /// the server's copy goes back to the velocity it had before the hit.
    pub(super) fn player_struck_by<R>(
        &mut self,
        net: &mut Network,
        conn: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let (result, sent) = self.as_player(net, conn, |g| {
            let before = g.player.as_ref().map(|p| p.velocity);
            let result = f(g);
            let sent = g.player.as_mut().zip(before).and_then(|(p, before)| {
                std::mem::take(&mut p.health.sync_velocity)
                    .then(|| std::mem::replace(&mut p.velocity, before))
            });
            (result, sent)
        })?;
        self.send_player_motion(net, conn, sent);
        Some(result)
    }
    /// ClientboundSetEntityMotionPacket to a player about themselves.
    pub(super) fn send_player_motion(&self, net: &Network, conn: u32, velocity: Option<Vec3>) {
        if let (Some(v), Some(g)) = (velocity, net.hub.player(conn)) {
            g.send(&C::SetEntityMotion(play::SetEntityMotion {
                id: g.entity_id,
                velocity: [v.x, v.y, v.z],
            }));
        }
    }
}

/// A player as the blocks that look for players see them
/// (`PlayerDetector`, `level.getPlayers`: the dead ones too).
pub(super) struct Present {
    pub uuid: [i32; 4],
    pub position: [f64; 3],
    pub eye: [f64; 3],
    pub height: f32,
    pub creative: bool,
    pub spectator: bool,
    pub alive: bool,
}

impl crate::game::Sim<'_> {
    /// Every player in the world (`ServerLevel.getPlayers` past the host).
    pub(super) fn net_present(&self) -> Vec<Present> {
        let players = self
            .network
            .iter()
            .flat_map(|h| h.hub.players.iter().filter(|g| h.hub.here(g)));
        players
            .filter(|g| g.loaded)
            .map(|g| {
                let [x, y, z] = g.position;
                Present {
                    uuid: crate::hc::server_net::playerdata::uuid_ints(g.uuid),
                    position: g.position,
                    eye: [x, y + g.eye_height(), z],
                    height: g.dimensions().height,
                    creative: g.creative(),
                    spectator: g.spectator(),
                    alive: !g.dead,
                }
            })
            .collect()
    }
    /// The UUID of the player acting: the player `as_player` runs, else the host.
    pub(super) fn acting_uuid(&self) -> [i32; 4] {
        self.net_acting_uuid
            .map(crate::hc::server_net::playerdata::uuid_ints)
            .or(self.level.player_uuid)
            .unwrap_or_default()
    }
}

/// A player's bounding box, by its pose (`Avatar.POSES`).
pub(super) fn player_bounds(g: &crate::hc::server_net::player::ServerPlayer) -> Aabb {
    g.bounds()
}
