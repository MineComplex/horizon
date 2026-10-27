//! The sounds the game plays, as the simulation decides them: each seeded from the level random
//! (`Level.playSound` and `ClientLevel.playLocalSound` take `random.nextLong()`), recorded for the
//! players who hear them through the network (`capture`) and counted for the control socket. A
//! window plays the queue (`horizon_client::audio::Audio::apply`); a server has no ears.
use horizon_core::random::{LegacyRandom, RandomSource};
use std::collections::BTreeMap;

/// One thing for the window's mixer to do, in the order the game asked.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundCmd {
    /// A sound event at a position (or relative to the listener: `None`), with its seed and the
    /// `SoundSource` index it plays on when not the event's own (`/playsound`).
    Play {
        event: String,
        at: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
        seed: i64,
        source: Option<usize>,
    },
    /// `ClientboundStopSoundPacket`: stop an event and/or a source (`None`: any).
    Stop {
        event: Option<String>,
        source: Option<usize>,
    },
    /// Stop and drop a held instance (a jukebox's song).
    Release(u64),
}

pub struct SoundOut {
    /// Seeds of positional plays (the level random's `nextLong`).
    pub random: LegacyRandom,
    /// Positional sounds played since the last drain, kept while the world is open to the network
    /// so guests hear them too.
    pub capture: Option<Vec<(String, [f64; 3], f32, f32)>>,
    /// How many times each positional sound played (the control socket's `status`).
    pub heard: BTreeMap<String, u32>,
    /// How many times each selection was stopped ("record *", "* minecraft:…", `*` for any).
    pub stopped: BTreeMap<String, u32>,
    /// What the window has yet to play.
    pub queue: Vec<SoundCmd>,
    source_override: Option<usize>,
}

impl SoundOut {
    pub fn new(seed: i64) -> Self {
        Self {
            random: LegacyRandom::new(seed),
            capture: None,
            heard: BTreeMap::new(),
            stopped: BTreeMap::new(),
            queue: Vec::new(),
            source_override: None,
        }
    }
    /// Play a sound event at a world position, as `ClientLevel.playLocalSound` (a
    /// `SimpleSoundInstance` seeded from the level random), or relative to the listener without
    /// attenuation (`None`).
    pub fn play(&mut self, event: &str, at: Option<[f64; 3]>, volume: f32, pitch: f32) {
        let seed = self.random.next_long();
        self.play_seeded(event, at, volume, pitch, seed);
    }
    /// `play` for this client only: a sound a packet's handler plays (`playLocalSound`), which the
    /// guests play from that packet themselves.
    pub fn play_unshared(&mut self, event: &str, at: Option<[f64; 3]>, volume: f32, pitch: f32) {
        let capture = self.capture.take();
        self.play(event, at, volume, pitch);
        self.capture = capture;
    }
    /// `play` with the seed a sound packet carries.
    pub fn play_seeded(
        &mut self,
        event: &str,
        at: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) {
        if let (Some(capture), Some(at)) = (self.capture.as_mut(), at) {
            capture.push((event.to_owned(), at, volume, pitch));
        }
        if at.is_some() {
            *self.heard.entry(event.to_owned()).or_default() += 1;
        }
        self.queue.push(SoundCmd::Play {
            event: event.to_owned(),
            at,
            volume,
            pitch,
            seed,
            source: self.source_override,
        });
    }
    /// Play a sound on a given `SoundSource` index (`/playsound`).
    pub fn play_on(
        &mut self,
        source: usize,
        event: &str,
        at: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
    ) {
        self.source_override = Some(source);
        self.play(event, at, volume, pitch);
        self.source_override = None;
    }
    /// Stop and drop a held instance the window started (a jukebox's song).
    pub fn release(&mut self, instance: u64) {
        self.queue.push(SoundCmd::Release(instance));
    }
    /// `ClientboundStopSoundPacket`: stop the playing sounds of an event and/or a source.
    /// `selection` is how the control socket counts it ("<source or *> <event or *>").
    pub fn stop_sounds(&mut self, event: Option<&str>, source: Option<usize>, selection: String) {
        *self.stopped.entry(selection).or_default() += 1;
        self.queue.push(SoundCmd::Stop {
            event: event.map(str::to_owned),
            source,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_draw_seeds_in_order_and_unshared_ones_skip_the_capture() {
        let mut out = SoundOut::new(42);
        let mut random = LegacyRandom::new(42);
        out.capture = Some(Vec::new());
        out.play(
            "minecraft:block.stone.place",
            Some([1.0, 2.0, 3.0]),
            1.0,
            0.8,
        );
        out.play_unshared("minecraft:block.chest.open", Some([0.0; 3]), 0.5, 1.0);
        out.play_on(2, "minecraft:ui.toast.in", None, 1.0, 1.0);
        let seeds: Vec<i64> = out
            .queue
            .iter()
            .filter_map(|c| match c {
                SoundCmd::Play { seed, .. } => Some(*seed),
                SoundCmd::Stop { .. } | SoundCmd::Release(_) => None,
            })
            .collect();
        assert_eq!(
            seeds,
            vec![random.next_long(), random.next_long(), random.next_long()]
        );
        assert_eq!(out.capture.as_ref().unwrap().len(), 1);
        assert_eq!(out.heard["minecraft:block.chest.open"], 1);
        assert!(matches!(
            out.queue[2],
            SoundCmd::Play {
                source: Some(2),
                ..
            }
        ));
    }
}
