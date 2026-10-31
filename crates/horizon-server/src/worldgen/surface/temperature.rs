//! A biome's temperature at a block (`Biome.getTemperature`, `coldEnoughToSnow`,
//! `shouldMeltFrozenOceanIcebergSlightly`), which the temperature condition and the frozen
//! ocean's icebergs ask. Copied from Horizon's `biome_temperature` and adapted: the three fixed
//! noises (`TEMPERATURE_NOISE`, `FROZEN_TEMPERATURE_NOISE`, `BIOME_INFO_NOISE`: Legacy randoms of
//! seeds 1234, 3456 and 2345, the same for every level) are built when the registries load and
//! owned by [`BiomeClimates`], not kept in statics. Moonrise removes vanilla's per-thread
//! temperature cache; the values are the same.

use serde_json::value::RawValue;

use crate::core::position::BlockPos;
use crate::util::random::LegacyRandom;
use crate::worldgen::codec::{DecodeError, boolean, float, object, optional, required, string};
use crate::worldgen::noise::SimplexNoise;

/// A biome's `ClimateSettings` as the temperature reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeClimate {
    pub temperature: f32,
    /// `temperature_modifier: frozen`.
    pub frozen: bool,
}

impl BiomeClimate {
    /// The climate fields of a `worldgen/biome` element (`Biome.ClimateSettings.CODEC`, a map
    /// codec merged into the biome's own).
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let frozen = match optional(&fields, "temperature_modifier")
            .map(string)
            .transpose()?
        {
            None => false,
            Some(name) => match name.as_str() {
                "none" => false,
                "frozen" => true,
                _ => return Err(DecodeError::Field("temperature_modifier")),
            },
        };
        float(required(&fields, "downfall")?)?;
        boolean(required(&fields, "has_precipitation")?)?;
        Ok(Self {
            temperature: float(required(&fields, "temperature")?)?,
            frozen,
        })
    }
}

/// Every biome's climate (by registry id) and the fixed noises the temperature uses.
#[derive(Clone, Debug)]
pub struct BiomeClimates {
    biomes: Vec<BiomeClimate>,
    height: SimplexNoise,
    /// `FROZEN_TEMPERATURE_NOISE`: three simplex octaves, (noise, frequency, amplitude) each.
    frozen: [(SimplexNoise, f64, f32); 3],
    info: SimplexNoise,
}

impl BiomeClimates {
    pub fn new(biomes: Vec<BiomeClimate>) -> Self {
        let mut random = LegacyRandom::new(3456);
        let mut octave = |frequency, amplitude| {
            (
                SimplexNoise::with_skipped_offsets(&mut random, true),
                frequency,
                amplitude,
            )
        };
        let frozen = [
            octave(1.0, 0.142_857_15),
            octave(0.5, 0.285_714_3),
            octave(0.25, 0.571_428_6),
        ];
        Self {
            biomes,
            height: SimplexNoise::with_skipped_offsets(&mut LegacyRandom::new(1234), true),
            frozen,
            info: SimplexNoise::with_skipped_offsets(&mut LegacyRandom::new(2345), true),
        }
    }

    /// `Biome.getTemperature` (`getHeightAdjustedTemperature`): the base temperature, the frozen
    /// modifier's ice patches, and the cooling above the snow line.
    pub fn temperature(&self, biome: u32, pos: BlockPos, sea_level: i32) -> f32 {
        let climate = self.biomes[biome as usize];
        let base = if climate.frozen {
            self.frozen_modifier(pos, climate.temperature)
        } else {
            climate.temperature
        };
        let snow_level = sea_level.wrapping_add(17);
        if pos.y > snow_level {
            let variation = self
                .height
                .get2(f64::from(pos.x as f32 / 8.0), f64::from(pos.z as f32 / 8.0))
                * 8.0;
            base - (variation + pos.y as f32 - snow_level as f32) * 0.05 / 40.0
        } else {
            base
        }
    }

    /// `TemperatureModifier.FROZEN`: 0.2 inside ice patches, else the base.
    fn frozen_modifier(&self, pos: BlockPos, base: f32) -> f32 {
        let (x, z) = (f64::from(pos.x), f64::from(pos.z));
        let mut large = 0.0f32;
        for (noise, frequency, amplitude) in &self.frozen {
            large += amplitude * noise.get2(x * 0.05 * frequency, z * 0.05 * frequency);
        }
        let ice_patches = f64::from(large * 7.0) + f64::from(self.info.get2(x * 0.2, z * 0.2));
        if ice_patches < 0.3 && f64::from(self.info.get2(x * 0.09, z * 0.09)) < 0.8 {
            0.2
        } else {
            base
        }
    }

    pub fn warm_enough_to_rain(&self, biome: u32, pos: BlockPos, sea_level: i32) -> bool {
        self.temperature(biome, pos, sea_level) >= 0.15
    }

    pub fn cold_enough_to_snow(&self, biome: u32, pos: BlockPos, sea_level: i32) -> bool {
        !self.warm_enough_to_rain(biome, pos, sea_level)
    }

    pub fn melts_iceberg_slightly(&self, biome: u32, pos: BlockPos, sea_level: i32) -> bool {
        self.temperature(biome, pos, sea_level) > 0.1
    }
}
