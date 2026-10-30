//! `NoiseStack`: octaves summed with their frequencies and amplitudes. 26.3 only stacks
//! `ImprovedNoise` (normal noises, legacy fBm) and its smeared variant (`BlendedNoise`), so the
//! octaves are a closed enum owned by the stack, not shared trait objects.

use super::{DensityError, DensityVolume, NanRange, PerlinNoise, SmearedPerlinNoise};

/// One octave's noise.
#[derive(Clone, Debug)]
pub enum Octave {
    Perlin(PerlinNoise),
    Smeared(SmearedPerlinNoise),
}

impl Octave {
    fn range(&self) -> Result<[f32; 2], NanRange> {
        match self {
            Self::Perlin(noise) => Ok(noise.range()),
            Self::Smeared(noise) => noise.range(),
        }
    }
    fn get2(&self, x: f64, z: f64) -> f32 {
        match self {
            Self::Perlin(noise) => noise.get2(x, z),
            Self::Smeared(noise) => noise.get2(x, z),
        }
    }
    fn get3(&self, x: f64, y: f64, z: f64) -> f32 {
        match self {
            Self::Perlin(noise) => noise.get3(x, y, z),
            Self::Smeared(noise) => noise.get3(x, y, z),
        }
    }
    fn add_to_volume(
        &self,
        buffer: &mut [f32],
        volume: DensityVolume,
        xz_scale: f64,
        y_scale: f64,
        amplitude: f32,
    ) -> Result<(), DensityError> {
        match self {
            Self::Perlin(noise) => {
                noise.add_to_volume(buffer, volume, xz_scale, y_scale, amplitude)
            }
            Self::Smeared(noise) => {
                noise.add_to_volume(buffer, volume, xz_scale, y_scale, amplitude)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct NoiseLayer {
    pub noise: Octave,
    pub frequency: f64,
    pub amplitude: f32,
}

#[derive(Clone, Debug)]
pub struct NoiseStack {
    layers: Vec<NoiseLayer>,
    range: [f32; 2],
}

impl NoiseStack {
    pub fn builder() -> NoiseStackBuilder {
        NoiseStackBuilder::default()
    }
    pub fn layers(&self) -> &[NoiseLayer] {
        &self.layers
    }
    pub fn range(&self) -> [f32; 2] {
        self.range
    }
    pub fn get2(&self, x: f64, z: f64) -> f32 {
        let mut value = 0.0;
        for layer in &self.layers {
            value += layer.amplitude * layer.noise.get2(x * layer.frequency, z * layer.frequency);
        }
        value
    }
    pub fn get3(&self, x: f64, y: f64, z: f64) -> f32 {
        let mut value = 0.0;
        for layer in &self.layers {
            value += layer.amplitude
                * layer.noise.get3(
                    x * layer.frequency,
                    y * layer.frequency,
                    z * layer.frequency,
                );
        }
        value
    }
    /// Adds `amplitude` times the stack over `volume` into `buffer` (Z, X, Y order, Y innermost).
    pub fn add_to_volume(
        &self,
        buffer: &mut [f32],
        volume: DensityVolume,
        xz_scale: f64,
        y_scale: f64,
        amplitude: f32,
    ) -> Result<(), DensityError> {
        if buffer.len() < volume.size() {
            return Err(DensityError::BufferTooShort);
        }
        for layer in &self.layers {
            layer.noise.add_to_volume(
                buffer,
                volume,
                xz_scale * layer.frequency,
                y_scale * layer.frequency,
                amplitude * layer.amplitude,
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct NoiseStackBuilder {
    layers: Vec<NoiseLayer>,
}

impl NoiseStackBuilder {
    pub fn add(&mut self, noise: Octave, frequency: f64, amplitude: f32) -> &mut Self {
        self.layers.push(NoiseLayer {
            noise,
            frequency,
            amplitude,
        });
        self
    }
    /// Flattening rescales each existing layer before subsequent sampling.
    pub fn add_stack(&mut self, stack: NoiseStack, frequency: f64, amplitude: f32) -> &mut Self {
        self.layers
            .extend(stack.layers.into_iter().map(|layer| NoiseLayer {
                noise: layer.noise,
                frequency: layer.frequency * frequency,
                amplitude: layer.amplitude * amplitude,
            }));
        self
    }
    pub fn build(self) -> Result<NoiseStack, NanRange> {
        let mut range = [0.0; 2];
        for layer in &self.layers {
            let bounds = layer.noise.range()?;
            if layer.amplitude.is_nan() {
                return Err(NanRange);
            }
            let scaled = if bounds[0].is_nan() || bounds[1].is_nan() {
                [f32::NAN; 2]
            } else {
                // Interval.mulBound defines zero times infinity as positive zero.
                let product = |v: f32| {
                    if v == 0.0 || layer.amplitude == 0.0 {
                        0.0
                    } else {
                        v * layer.amplitude
                    }
                };
                let a = product(bounds[0]);
                let b = product(bounds[1]);
                if a < b {
                    [a, b]
                } else if a > b {
                    [b, a]
                } else if a == 0.0 {
                    [
                        f32::from_bits(a.to_bits() | b.to_bits()),
                        f32::from_bits(a.to_bits() & b.to_bits()),
                    ]
                } else {
                    [a, b]
                }
            };
            range = [range[0] + scaled[0], range[1] + scaled[1]];
            if range[0].is_nan() || range[1].is_nan() {
                range = [f32::NAN; 2];
            }
            // Interval.of interns [0,1], including a supplied negative zero.
            if range[0] == 0.0 && range[1] == 1.0 {
                range[0] = 0.0;
            }
        }
        Ok(NoiseStack {
            layers: self.layers,
            range,
        })
    }
}
