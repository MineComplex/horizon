//! The players' locator bar (server_net/waypoints.rs): what
//! each player looks like to their level's waypoint manager this tick.
use super::server_net::Network;
use crate::hc::{
    items::ItemStack,
    server_net::waypoints::{self, HIDING_HEADS, Icon, PLAYER_RANGE},
};

/// `waypoint_transmit_range`: hidden by crouching, invisibility or a head worn.
fn transmit_range(crouching: bool, invisible: bool, head: Option<&ItemStack>) -> f64 {
    let hiding_head = head.is_some_and(|s| HIDING_HEADS.contains(&&*s.item));
    if crouching || invisible || hiding_head {
        0.0
    } else {
        PLAYER_RANGE
    }
}

impl crate::game::Sim<'_> {
    /// The player's team and its colour, as the locator bar shows it.
    fn waypoint_team(&self, name: &str) -> Option<(String, Option<i32>)> {
        let team = self.score.board.players_team(name)?;
        let color = team.color.as_deref().and_then(waypoints::team_rgb);
        Some((team.name.clone(), color))
    }
    /// Each level's `ServerWaypointManager` ticked with the players in that
    /// level: a player who changes level leaves the old one's manager
    /// (`ServerLevel.removePlayerImmediately`: untracked for everyone there)
    /// and joins the new one's (`addDuringTeleport`).
    pub(super) fn net_update_waypoints(&mut self, net: &mut Network) {
        let mut players: Vec<(&str, waypoints::Player)> = Vec::new();
        let mut respawned = Vec::new();
        for g in net.hub.players.iter().filter(|g| g.placed) {
            // PlayerList.respawn makes a new ServerPlayer.
            if g.dead {
                net.waypoint_dead.insert(g.uuid);
            } else if net.waypoint_dead.remove(&g.uuid) {
                respawned.push(g.uuid);
            }
            // Player.updatePlayerPose: the shift key crouches unless flying.
            let crouching = g.sneaking && !g.flying && !g.fall_flying && !g.sleeping;
            players.push((
                &g.dimension,
                waypoints::Player {
                    uuid: g.uuid,
                    conn: Some(g.conn),
                    position: g.position,
                    spectator: g.spectator(),
                    transmit_range: transmit_range(
                        crouching,
                        g.effects.get("invisibility").is_some(),
                        g.armor[0].as_ref(),
                    ),
                    receive_range: PLAYER_RANGE,
                    first_tick: g.ticks == 0,
                    view_distance: g.view_distance,
                    team: self.waypoint_team(&g.name),
                    icon: Icon::default(),
                },
            ));
        }
        net.waypoint_dead
            .retain(|uuid| players.iter().any(|(_, p)| p.uuid == *uuid));
        let locator_bar = self.world_data.rules.bool("locator_bar");
        let mut levels: Vec<String> = net.waypoints.keys().cloned().collect();
        for (level, _) in &players {
            if !levels.iter().any(|l| l == level) {
                levels.push((*level).to_owned());
            }
        }
        levels.sort();
        let mut out = Vec::new();
        for level in levels {
            let here: Vec<waypoints::Player> = players
                .iter()
                .filter(|(l, _)| *l == level)
                .map(|(_, p)| p.clone())
                .collect();
            let away: Vec<(u128, u32)> = players
                .iter()
                .filter(|(l, _)| *l != level)
                .filter_map(|(_, p)| Some((p.uuid, p.conn?)))
                .collect();
            let manager = net.waypoints.entry(level.clone()).or_default();
            out.extend(manager.tick_level(&here, &away, &respawned, locator_bar));
            // Nobody left in the level: nothing to remember there.
            if here.is_empty() {
                net.waypoints.remove(&level);
            }
        }
        for (conn, packet) in out {
            if let Some(g) = net.hub.player(conn) {
                g.send(&packet);
            }
        }
    }
}
