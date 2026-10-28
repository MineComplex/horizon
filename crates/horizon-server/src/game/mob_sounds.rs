//! The looping sound instances of mobs: a bee's buzz
//! (`ClientPacketListener.postAddEntitySoundInstance`) and a guardian's
//! attack hum (entity event 21). The sound engine ticks them with what
//! `start_mob_sounds` reads off the mobs.
use crate::hc::mobs::{Species, bee, guardian};
