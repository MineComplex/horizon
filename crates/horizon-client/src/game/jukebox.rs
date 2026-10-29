//! Jukeboxes (`JukeboxBlock`, `JukeboxBlockEntity`, `JukeboxSongPlayer`):
//! inserting a `jukebox_playable` disc, ejecting it, the song with its
//! "Now Playing" overlay, note particles every second, the redstone
//! signal while playing, the comparator output, and saving.
use super::Game;
use horizon_client::{
    containers::BlockEntity,
    mobs::Species};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos};

impl crate::game::Sim<'_> {
    fn stop_sound(&mut self, pos: [i32; 3]) {
        if self.jukebox_sounds.remove(&(pos[0], pos[1], pos[2])) {
            self.sounds.stop_jukebox(pos);
        }
    }
    /// Level event 1011: the song ends, here and for the guests.
    fn stop_song(&mut self, pos: [i32; 3]) {
        self.stop_sound(pos);
        self.notify_record_playing(pos, false);
        if self.sounds.capture.is_some() {
            self.lan_songs.push((pos, None));
        }
    }
    /// `LevelEventHandler.notifyNearbyEntities` (level events 1010 and 1011):
    /// parrots within three blocks of the jukebox start or stop dancing.
    pub(super) fn notify_record_playing(&mut self, pos: [i32; 3], playing: bool) {
        let min = Vec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
        let area = Aabb::new(min, min.add_vector(Vec3::new(1.0, 1.0, 1.0)))
            .inflate(Vec3::new(3.0, 3.0, 3.0));
        for m in self
            .mobs
            .iter_mut()
            .filter(|m| m.species() == Species::Parrot && m.bounds().intersects(area))
        {
            m.extra.party_jukebox = playing.then_some(pos);
        }
    }
    /// A broken jukebox stops its song (its disc drops with the contents).
    pub(super) fn jukebox_removed(&mut self, pos: [i32; 3]) {
        self.stop_song(pos);
    }
}
/// The jukebox's redstone signal (15 while a song plays) and comparator output.
pub(super) fn jukebox_signals(
    world: &horizon_client::world::World,
    items: &horizon_client::items::Items,
    pos: BlockPos,
) -> Option<(i32, i32)> {
    match world.block_entities.get(&(pos.x, pos.y, pos.z)) {
        Some(BlockEntity::Jukebox(j)) => {
            let comparator = j
                .record
                .as_ref()
                .and_then(|r| items.get(&r.item)?.jukebox_song.as_ref())
                .and_then(|s| items.songs.get(s))
                .map_or(0, |s| s.comparator);
            Some((if j.playing { 15 } else { 0 }, comparator))
        }
        _ => None,
    }
}
