//! Players hitting players on the server (`Player.attack` on a player). The hit
//! is worked out as the attacker (weapon, attack strength, critical hit,
//! enchantments, a mace's smash) and lands on the victim like any player
//! attack (blocking, armour, knockback), when the `pvp` game rule allows it
//! (`ServerPlayer.canHarmPlayer`, `isPvpAllowed`). A player's arrow on
//! another player lands the same way.
use super::{net_mobs::player_bounds, server_net::Network};

use crate::hc::{
    Result,
    mobs::Who,
    projectiles::{Hit, Projectile, ProjectileKind},
};
use horizon_core::geometry::{Aabb, Vec3};

/// The player projectiles that meet other players: arrows (their hurt), and
/// snowballs and eggs, which break on them (`Player.hurtServer` takes no 0
/// damage). Pearls, potions and wind charges fly past players as before.
pub(super) fn meets_players(kind: &ProjectileKind) -> bool {
    matches!(
        kind,
        ProjectileKind::Arrow { .. } | ProjectileKind::Snowball | ProjectileKind::Egg
    )
}

/// Who a player's attack lands on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Victim {
    ServerPlayer(u32),
}

/// The attacker's side of the hit.
pub(super) struct Strike {
    /// The attacking player, kept after their turn through `as_player` ends.
    attacker: Option<u32>,
    amount: f32,
    scale: f32,
    crit: bool,
    /// `Player.attack`'s `magicBoost > 0`: the enchantments added damage.
    magic: bool,
    from: [f64; 3],
    /// `getSecondsToDisableBlocking` of the attacker's weapon.
    disable_blocking: f32,
    sweep: Option<[f64; 3]>,
    /// `causeExtraKnockback`: its strength and direction (the attacker's
    /// yaw), and whether the hit is a sprinting knockback attack.
    knockback: (f32, f64, f64),
    knockback_attack: bool,
    /// The attacker's team, for the victim's `Player.canHarmPlayer`.
    team: Option<crate::hc::scoreboard::Team>,
    /// The sweep's damage to the players around the victim.
    sweep_hit: Option<Sweep>,
}

/// A sweep attack's reach to the other players (`Player.doSweepAttack`),
/// worked out as the attacker.
#[derive(Clone)]
pub(super) struct Sweep {
    /// The attacking player (none: the game).
    attacker: Option<u32>,
    from: [f64; 3],
    /// The damage after the enchantments and the attack strength.
    damage: f32,
    /// The push along the attacker's yaw.
    knockback: (f64, f64),
    team: Option<crate::hc::scoreboard::Team>,
    /// The target's box grown by (1, 0.25, 1).
    pub(super) hit_box: Aabb,
    /// The player the sweep started from, which it spares.
    skip: Option<Victim>,
}

impl Strike {
    /// Whether it has extra knockback, which stops the attacker's sprint
    /// once it lands (Player.causeExtraKnockback).
    pub(super) fn stops_sprint(&self) -> bool {
        self.knockback.0 > 0.0
    }
}

impl crate::game::Sim<'_> {
    /// `Player.attack`'s `causeFoodExhaustion(0.1F)`, charged to the attacker
    /// (the game, or a player swapped in) only once the hit hurt the victim.
    pub(super) fn pvp_exhaustion(&mut self) {
        if let Some(p) = self.player.as_mut()
            && !self.creative
        {
            p.cause_food_exhaustion(0.1);
        }
    }
    /// The attacker's side of `Player.attack` (the game, or a player swapped
    /// in): the damage, the charge reset and the swing.
    pub(super) fn pvp_strike(&mut self) -> Option<Strike> {
        let p = self.player.as_ref()?;
        let from = [p.position.x, p.position.y, p.position.z];
        let (amount, scale, crit, magic) = self.melee_damage("minecraft:player");
        let sweep = self.sweep_attack_position(scale, crit);
        let sweep_hit = sweep.map(|_| {
            let base = self.sweep_base_damage(scale);
            self.player_sweep(base, scale)
        });
        let (strength, knockback_attack) = self.extra_knockback("minecraft:player", scale);
        let (xd, zd) = self.extra_knockback_direction();
        self.attack_ticks = 0;
        self.swing_arm();
        // LivingEntity.getSecondsToDisableBlocking: the held weapon's value,
        // none while another item is in use.
        let disable_blocking = if self.using.is_some() {
            0.0
        } else {
            self.attack_item_info()
                .map_or(0.0, |i| i.disable_blocking_seconds)
        };
        Some(Strike {
            attacker: self.net_acting,
            amount,
            scale,
            crit,
            magic,
            from,
            disable_blocking,
            sweep,
            knockback: (strength, xd, zd),
            knockback_attack,
            team: self.player_team().cloned(),
            sweep_hit,
        })
    }
    /// The players by connection, with their entity ids.
    pub(super) fn net_player_entities(&self) -> Vec<(u32, u32)> {
        self.network.as_ref().map_or_else(Vec::new, |net| {
            net.hub
                .players
                .iter()
                .map(|g| (g.conn, g.entity_id as u32))
                .collect()
        })
    }
    /// The player a player's projectile hit, when it hit one: the game for a
    /// player's shot, or a player.
    pub(super) fn projectile_pvp_victim(&self, p: &Projectile, hit: Hit) -> Option<Victim> {
        if p.owner != Some(Who::Player) || !meets_players(&p.kind) {
            return None;
        }
        let Hit::Entity { who, .. } = hit else {
            return None;
        };
        match who {
            Who::Player => None,
            Who::Mob(id) => self
                .network
                .as_ref()?
                .hub
                .players
                .iter()
                .find(|g| g.entity_id as u32 == id)
                .map(|g| Victim::ServerPlayer(g.conn)),
        }
    }
    /// A player's projectile on a player: the hit runs as the victim, whose
    /// hurt refuses it when the `pvp` game rule or their teams forbid it
    /// (`ServerPlayer.canHarmPlayer`; `AbstractArrow.onHitEntity` then turns
    /// the arrow back).
    pub(super) fn projectile_hits_player(
        &mut self,
        p: &Projectile,
        hit: Hit,
        victim: Victim,
    ) -> Result<()> {
        let Hit::Entity { at, .. } = hit else {
            return Ok(());
        };
        let Some(mut net) = self.network.take() else {
            return self.projectile_hit(p, hit);
        };
        let team_of = |game: &Self, name: &str| game.score.board.players_team(name).cloned();
        let attacker = match p.player {
            Some(conn) => net.hub.player(conn).and_then(|g| team_of(self, &g.name)),
            None => self.player_team().cloned(),
        };
        let target = match victim {
            Victim::ServerPlayer(conn) => net.hub.player(conn).and_then(|g| team_of(self, &g.name)),
        };
        let refused = !(self.world_data.rules.bool("pvp")
            && crate::hc::scoreboard::teams_allow_damage(target.as_ref(), attacker.as_ref()));
        let on_player = Hit::Entity {
            who: Who::Player,
            at,
        };
        let result = match victim {
            Victim::ServerPlayer(conn) => self.player_hit_by(&mut net, conn, |g| {
                g.combat.refuse_player_hit = refused;
                let r = g.projectile_hit(p, on_player);
                g.combat.refuse_player_hit = false;
                r
            }),
        };
        self.network = Some(net);
        result.unwrap_or(Ok(()))
    }
    /// The hit landing on the victim with its extra knockback, then the
    /// attacker's attack sound. Whether the victim was hurt.
    pub(super) fn pvp_land(&mut self, net: &mut Network, victim: Victim, s: &Strike) -> bool {
        // ServerPlayer.canHarmPlayer: the pvp game rule, then the victim's
        // team (Player.canHarmPlayer: allies only with friendly fire on).
        let team = match victim {
            Victim::ServerPlayer(conn) => net
                .hub
                .player(conn)
                .and_then(|g| self.score.board.players_team(&g.name)),
        };
        let harm = crate::hc::scoreboard::teams_allow_damage(team, s.team.as_ref());
        if s.knockback_attack {
            self.sounds.play(
                "minecraft:entity.player.attack.knockback",
                Some(s.from),
                1.0,
                1.0,
            );
        }
        // Player.attack: the hurt (with its knockback away from the
        // attacker), then causeExtraKnockback along the attacker's yaw.
        let land = |g: &mut Self| {
            g.combat.player_attacker = Some((s.from, s.disable_blocking));
            let hurt = g.mob_hits_player(s.amount, s.from, "minecraft:player_attack");
            g.combat.player_attacker = None;
            let (strength, xd, zd) = s.knockback;
            if hurt && strength > 0.0 {
                g.knock_back_player(f64::from(strength), xd, zd);
            }
            // The victim's thorns, dealt to the attacker below.
            let thorns = if hurt { g.thorns_damage() } else { Vec::new() };
            (hurt, thorns)
        };
        let (hurt, thorns) = if self.world_data.rules.bool("pvp") && harm {
            match victim {
                Victim::ServerPlayer(conn) => {
                    self.player_struck_by(net, conn, land).unwrap_or_default()
                }
            }
        } else {
            Default::default()
        };
        // ServerPlayer.awardKillScore: the blow that kills a player.
        if hurt {
            let died = match victim {
                Victim::ServerPlayer(conn) => net
                    .hub
                    .player(conn)
                    .and_then(|g| g.motion.as_deref())
                    .is_some_and(|p| p.health.dead_or_dying() && !p.health.totem_used),
            };
            if died {
                match s.attacker {
                    Some(conn) => {
                        self.as_player(net, conn, |g| g.stats.custom("player_kills", 1));
                    }
                    None => self.stats.custom("player_kills", 1),
                }
            }
        }
        if hurt && (s.crit || s.magic) {
            let target = match victim {
                Victim::ServerPlayer(conn) => net
                    .hub
                    .player(conn)
                    .and_then(|g| g.motion.as_deref().map(|p| (g.entity_id, p))),
            };
            if let Some((id, p)) = target {
                let d = p.dimensions;
                let (feet, width, height) = (
                    [p.position.x, p.position.y, p.position.z],
                    f64::from(d.width),
                    f64::from(d.height),
                );
                if s.crit {
                    self.critical_hit(s.attacker, id, feet, width, height, false);
                }
                if s.magic {
                    self.critical_hit(s.attacker, id, feet, width, height, true);
                }
            }
        }
        // `Enchantment.doPostAttack`: thorns hurt the attacking player.
        for damage in thorns {
            match s.attacker {
                None => self.hurt_player_by("minecraft:thorns", damage),
                Some(conn) => {
                    self.player_hit_by(net, conn, |g| g.hurt_player_by("minecraft:thorns", damage));
                }
            }
        }
        let sound = match (hurt, s.crit, s.scale > 0.9) {
            (false, ..) => "minecraft:entity.player.attack.nodamage",
            (true, true, _) => "minecraft:entity.player.attack.crit",
            (true, false, true) => "minecraft:entity.player.attack.strong",
            (true, false, false) => "minecraft:entity.player.attack.weak",
        };
        if let Some(at) = s.sweep.filter(|_| hurt) {
            self.sweep_attack(at, Some(s.from));
            // The other players next to the victim (Player.doSweepAttack).
            let bounds = match victim {
                Victim::ServerPlayer(conn) => net.hub.player(conn).map(player_bounds),
            };
            if let (Some(sweep), Some(bounds)) = (&s.sweep_hit, bounds) {
                self.sweep_players(
                    net,
                    &Sweep {
                        hit_box: bounds.inflate(Vec3::new(1.0, 0.25, 1.0)),
                        skip: Some(victim),
                        ..sweep.clone()
                    },
                );
            }
        } else {
            self.sounds.play(sound, Some(s.from), 1.0, 1.0);
        }
        hurt
    }
    /// The sweep of the player in place, for the players it reaches: the
    /// damage (`1 + ratio * baseDamage`, plus the enchantments against a
    /// player, scaled by the attack strength) and who hits. The box is set
    /// by the caller.
    pub(super) fn player_sweep(&mut self, base: f32, scale: f32) -> Sweep {
        let from = self
            .player
            .as_ref()
            .map_or([0.0; 3], |p| [p.position.x, p.position.y, p.position.z]);
        let damage = (base + self.enchant_damage_bonus("minecraft:player")) * scale;
        Sweep {
            attacker: self.net_acting,
            from,
            damage,
            knockback: self.extra_knockback_direction(),
            team: self.player_team().cloned(),
            hit_box: Aabb::new(Vec3::ZERO, Vec3::ZERO),
            skip: None,
        }
    }
    /// A sweep from a mob hit reaches the other players. Inside a player's
    /// turn the game holds the other players (`as_player`), so it waits for
    /// the turn to end.
    pub(super) fn sweep_other_players(&mut self, sweep: Sweep) {
        if self.net_acting.is_some() {
            self.creatures.sweeps.push(sweep);
        } else if let Some(mut net) = self.network.take() {
            self.sweep_players(&mut net, &sweep);
            self.network = Some(net);
        }
    }
    /// The sweeps players' turns put off.
    pub(super) fn flush_sweeps(&mut self) {
        for sweep in std::mem::take(&mut self.creatures.sweeps) {
            self.sweep_other_players(sweep);
        }
    }
    /// `Player.doSweepAttack` on the players: each other player in the box,
    /// closer than 3 blocks (`distanceToSqr < 9`) and not an ally
    /// (`isAlliedTo`), is hurt (`ServerPlayer.canHarmPlayer`: the `pvp` rule)
    /// and pushed 0.4 along the attacker's yaw.
    pub(super) fn sweep_players(&mut self, net: &mut Network, sweep: &Sweep) {
        let from = Vec3::new(sweep.from[0], sweep.from[1], sweep.from[2]);
        let mut victims = Vec::new();
        for g in &net.hub.players {
            if net.hub.here(g)
                && g.loaded
                && !g.dead
                && !g.spectator()
                && sweep.attacker != Some(g.conn)
                && player_bounds(g).intersects(sweep.hit_box)
                && Vec3::new(g.position[0], g.position[1], g.position[2]).distance_squared(from)
                    < 9.0
            {
                let team = self.score.board.players_team(&g.name).cloned();
                victims.push((Victim::ServerPlayer(g.conn), team));
            }
        }
        let (xd, zd) = sweep.knockback;
        let land = |g: &mut Self| {
            let hurt = g.mob_hits_player(sweep.damage, sweep.from, "minecraft:player_attack");
            if hurt {
                g.knock_back_player(0.4, xd, zd);
            }
        };
        for (victim, team) in victims {
            if sweep.skip == Some(victim)
                || crate::hc::scoreboard::teams_allied(team.as_ref(), sweep.team.as_ref())
                || !self.world_data.rules.bool("pvp")
            {
                continue;
            }
            match victim {
                Victim::ServerPlayer(conn) => {
                    self.player_struck_by(net, conn, land);
                }
            }
        }
    }
}

impl crate::game::Sim<'_> {
    /// The player `id` that player `conn` attacks, if it is one.
    pub(crate) fn pvp_victim(net: &Network, conn: u32, id: i32) -> Option<Victim> {
        net.hub
            .players
            .iter()
            .find(|g| g.entity_id == id && g.conn != conn && net.hub.here(g) && !g.dead)
            .map(|g| Victim::ServerPlayer(g.conn))
    }
}
