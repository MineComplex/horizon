//! Decoders for the serverbound play packets a join needs (`GameProtocols.SERVERBOUND_TEMPLATE`).
//! Other known ids decode to [`Serverbound::Other`] with their body untouched, for the systems that
//! will read them; an unknown id is the codec's `DecoderException`.

use super::ids::{SERVERBOUND, sb};
use crate::network::Fault;
use crate::network::codec::read_var_int;

/// `ClientInformation`, enum ids already mapped as their `ByIdMap` strategies do (chat
/// visibility and particle status wrap, the main hand falls back to 0, left).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInformation {
    pub language: String,
    pub view_distance: i8,
    /// 0 full, 1 system, 2 hidden.
    pub chat_visibility: u8,
    pub chat_colors: bool,
    pub model_customisation: u8,
    /// 0 left, 1 right.
    pub main_hand: u8,
    pub text_filtering_enabled: bool,
    pub allows_listing: bool,
    /// 0 all, 1 decreased, 2 minimal.
    pub particle_status: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Serverbound<'a> {
    AcceptTeleportation {
        id: i32,
        position: [f64; 3],
        y_rot: f32,
        x_rot: f32,
    },
    ChunkBatchReceived {
        desired_chunks_per_tick: f32,
    },
    ClientTickEnd,
    ClientInformation(ClientInformation),
    KeepAlive(i64),
    /// The four `ServerboundMovePlayerPacket`s: `Pos`, `PosRot`, `Rot`, `StatusOnly`.
    MovePlayer {
        position: Option<[f64; 3]>,
        rotation: Option<(f32, f32)>,
        on_ground: bool,
        horizontal_collision: bool,
    },
    PlayerLoaded,
    Other {
        id: i32,
        body: &'a [u8],
    },
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Fault> {
        if self.0.len() < N {
            return Err(Fault(format!(
                "java.lang.IndexOutOfBoundsException: readerIndex + length({N}) exceeds writerIndex"
            )));
        }
        let (head, rest) = self.0.split_at(N);
        self.0 = rest;
        Ok(head.try_into().expect("N bytes"))
    }
    fn u8(&mut self) -> Result<u8, Fault> {
        Ok(self.take::<1>()?[0])
    }
    fn bool(&mut self) -> Result<bool, Fault> {
        Ok(self.u8()? != 0)
    }
    fn f32(&mut self) -> Result<f32, Fault> {
        Ok(f32::from_be_bytes(self.take()?))
    }
    fn f64(&mut self) -> Result<f64, Fault> {
        Ok(f64::from_be_bytes(self.take()?))
    }
    fn i64(&mut self) -> Result<i64, Fault> {
        Ok(i64::from_be_bytes(self.take()?))
    }
    fn xyz(&mut self) -> Result<[f64; 3], Fault> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }
    fn var_int(&mut self) -> Result<i32, Fault> {
        match read_var_int(self.0)? {
            Some((v, n)) => {
                self.0 = &self.0[n..];
                Ok(v)
            }
            None => Err(Fault("java.lang.IndexOutOfBoundsException: VarInt".into())),
        }
    }
    /// `readUtf(max)`.
    fn utf(&mut self, max: usize) -> Result<String, Fault> {
        let len = self.var_int()?;
        if len > (max * 3) as i32 {
            return Err(Fault::decoder(format!(
                "The received encoded string buffer length is longer than maximum allowed ({len} > {})",
                max * 3
            )));
        }
        if len < 0 || len as usize > self.0.len() {
            return Err(Fault::decoder(
                "The received encoded string buffer length is less than zero! Weird string!",
            ));
        }
        let (bytes, rest) = self.0.split_at(len as usize);
        self.0 = rest;
        let s = String::from_utf8_lossy(bytes).into_owned();
        let units = s.encode_utf16().count();
        if units > max {
            return Err(Fault::decoder(format!(
                "The received string length is longer than maximum allowed ({units} > {max})"
            )));
        }
        Ok(s)
    }
    /// A move packet's flags byte: on ground (1), horizontal collision (2).
    fn move_flags(&mut self) -> Result<(bool, bool), Fault> {
        let flags = self.u8()?;
        Ok((flags & 1 != 0, flags & 2 != 0))
    }
}

/// `ByIdMap.OutOfBoundsStrategy.WRAP` over `n` values.
fn wrap(id: i32, n: i32) -> u8 {
    id.rem_euclid(n) as u8
}

/// Decodes a play packet body (VarInt id, then the fields), with the codec's errors: an unknown
/// id, a failed field, or bytes left over.
pub fn decode(body: &[u8]) -> Result<Serverbound<'_>, Fault> {
    let mut r = Reader(body);
    let id = r.var_int()?;
    let Some(name) = usize::try_from(id).ok().and_then(|i| SERVERBOUND.get(i)) else {
        return Err(Fault::decoder(format!("Received unknown packet id {id}")));
    };
    let packet = (|| {
        Ok(match id {
            sb::ACCEPT_TELEPORTATION => Serverbound::AcceptTeleportation {
                id: r.var_int()?,
                position: r.xyz()?,
                y_rot: r.f32()?,
                x_rot: r.f32()?,
            },
            sb::CHUNK_BATCH_RECEIVED => Serverbound::ChunkBatchReceived {
                desired_chunks_per_tick: r.f32()?,
            },
            sb::CLIENT_TICK_END => Serverbound::ClientTickEnd,
            sb::CLIENT_INFORMATION => Serverbound::ClientInformation(ClientInformation {
                language: r.utf(16)?,
                view_distance: r.u8()? as i8,
                chat_visibility: wrap(r.var_int()?, 3),
                chat_colors: r.bool()?,
                model_customisation: r.u8()?,
                main_hand: u8::from(r.var_int()? == 1),
                text_filtering_enabled: r.bool()?,
                allows_listing: r.bool()?,
                particle_status: wrap(r.var_int()?, 3),
            }),
            sb::KEEP_ALIVE => Serverbound::KeepAlive(r.i64()?),
            sb::MOVE_PLAYER_POS..=sb::MOVE_PLAYER_STATUS_ONLY => {
                let position = matches!(id, sb::MOVE_PLAYER_POS | sb::MOVE_PLAYER_POS_ROT)
                    .then(|| r.xyz())
                    .transpose()?;
                let rotation = matches!(id, sb::MOVE_PLAYER_POS_ROT | sb::MOVE_PLAYER_ROT)
                    .then(|| Ok::<_, Fault>((r.f32()?, r.f32()?)))
                    .transpose()?;
                let (on_ground, horizontal_collision) = r.move_flags()?;
                Serverbound::MovePlayer {
                    position,
                    rotation,
                    on_ground,
                    horizontal_collision,
                }
            }
            sb::PLAYER_LOADED => Serverbound::PlayerLoaded,
            _ => {
                let body = r.0;
                r.0 = &[];
                Serverbound::Other { id, body }
            }
        })
    })()
    .map_err(|e: Fault| {
        if e.0.starts_with("io.netty.handler.codec.DecoderException") {
            e
        } else {
            Fault::decoder(format!(
                "Failed to decode packet 'serverbound/minecraft:{name}'"
            ))
        }
    })?;
    if !r.0.is_empty() {
        return Err(Fault::decoder(format!(
            // Java names the packet class too: `... minecraft:{name} (ServerboundXPacket) was larger ...`.
            "java.io.IOException: Packet play/serverbound/minecraft:{name} was larger than I expected, found {} bytes extra whilst reading packet serverbound/minecraft:{name}",
            r.0.len()
        )));
    }
    Ok(packet)
}
