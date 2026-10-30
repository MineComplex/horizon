use super::*;
use crate::core::geometry::Axis;
use crate::util::random::LegacyRandom;

#[test]
fn vanilla_seed_zero_samples_and_derivatives() {
    // Values captured by Horizon's original adapter from the pinned server JAR.
    let mut random = LegacyRandom::new(0);
    let noise = PerlinNoise::new(&mut random);
    assert_eq!(random.next_long(), -6_819_959_017_335_327_829);
    let mut derivative = [0.0; 3];
    assert_eq!(
        noise
            .noise_with_derivative(0.0, 0.0, 0.0, &mut derivative)
            .to_bits(),
        3_183_733_582
    );
    assert_eq!(
        derivative.map(f32::to_bits),
        [3_189_117_465, 3_214_556_526, 1_017_326_887]
    );
    let mut random = LegacyRandom::new(0);
    let simplex = SimplexNoise::with_skipped_offsets(&mut random, true);
    assert_eq!(simplex.table().offsets(), &[0.0; 3]);
    assert_eq!(simplex.table().permutations(), noise.table().permutations());
    assert_eq!(random.next_long(), -6_819_959_017_335_327_829);
    assert_eq!(simplex.get2(0.25, 0.75).to_bits(), 3_146_180_247);
    assert_eq!(simplex.get3(0.25, 0.5, 0.75).to_bits(), 1_062_011_824);
    assert!(simplex.get3(f64::NAN, 0.0, 0.0).is_nan());
    assert_eq!(
        SmearedPerlinNoise::new(&mut random, f64::NAN).range(),
        Err(NanRange)
    );
}

#[test]
fn bulk_accumulation_preserves_tail_and_rejects_short_buffers_before_writes() {
    let noise = PerlinNoise::new(&mut LegacyRandom::new(0));
    let volume = DensityVolume::new([1; 3], [0; 3], [1; 3]).unwrap();
    let mut random = LegacyRandom::new(0);
    let mut values = std::array::from_fn::<_, 5, _>(|_| f32::from_bits(random.next_int() as u32));
    let tail = values[1..].iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    noise
        .add_to_volume(&mut values, volume, 1.0, 1.0, 1.0)
        .unwrap();
    assert_eq!(values[0].to_bits(), 3_184_062_705);
    assert_eq!(
        values[1..].iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        tail
    );
    let before = values.map(f32::to_bits);
    let too_large = DensityVolume::new([2; 3], [0; 3], [1; 3]).unwrap();
    assert_eq!(
        noise.add_to_volume(&mut values, too_large, 1.0, 1.0, 1.0),
        Err(DensityError::BufferTooShort)
    );
    assert_eq!(values.map(f32::to_bits), before);
}

#[test]
fn density_dimensions_are_bounded_and_coordinates_wrap() {
    assert_eq!(
        DensityVolume::new([0, 1, 1], [0; 3], [1; 3]),
        Err(DensityError::NonpositiveDimensions)
    );
    assert_eq!(
        DensityVolume::new([1; 3], [0; 3], [1, 0, 1]),
        Err(DensityError::NonpositiveSteps)
    );
    assert_eq!(
        DensityVolume::new([i32::MAX; 3], [0; 3], [1; 3]),
        Err(DensityError::VolumeTooLarge)
    );
    let volume = DensityVolume::new([2, 3, 4], [i32::MAX, 0, -1], [2, 4, 8]).unwrap();
    assert_eq!(volume.block(Axis::X, 1), Some(i32::MIN + 1));
    assert_eq!(volume.block(Axis::X, 2), None);
    assert_eq!(volume.index([1, 2, 3]), Some(23));
    assert_eq!(volume.index([1, 3, 3]), None);
}

#[test]
fn normal_noise_reference_values_and_legacy_failure_draws() {
    let mut builder = NormalNoise::builder();
    builder.set_base_octave(-3);
    assert_eq!(
        builder.set_amplitude_modifier(-1, 0.0).unwrap_err(),
        NoiseBuildError::ModifierIndex
    );
    builder.set_octave_count(3).unwrap();
    builder.set_amplitude_modifier(1, 0.0).unwrap();
    assert_eq!(
        builder.set_octave_count(4).unwrap_err(),
        NoiseBuildError::CountAfterModifiers
    );
    let definition = builder.build().unwrap();
    assert_eq!(
        definition.normalization_factor().to_bits(),
        4_607_442_513_526_237_848
    );
    assert_eq!(
        definition
            .octaves()
            .iter()
            .map(|o| o.octave_index)
            .collect::<Vec<_>>(),
        [-3, -1]
    );
    let mut random = LegacyRandom::new(0);
    let modern = definition.create(&mut random).unwrap();
    assert_eq!(modern.get3(0.0, 0.0, 0.0).to_bits(), 1_042_650_710);
    assert_eq!(random.next_long(), -6_688_467_811_848_818_630);
    let mut random = LegacyRandom::new(0);
    let legacy = definition
        .create_for_legacy_nether_biome(&mut random)
        .unwrap();
    assert_eq!(legacy.get3(0.0, 0.0, 0.0).to_bits(), 3_199_632_936);
    assert_eq!(random.next_long(), 4_966_550_093_728_345_520);
    let mut empty = NormalNoise::builder();
    empty.set_base_octave(2).set_octave_count(0).unwrap();
    let mut random = LegacyRandom::new(0);
    assert_eq!(
        empty
            .build()
            .unwrap()
            .create_for_legacy_nether_biome(&mut random)
            .unwrap_err(),
        NoiseBuildError::PositiveLegacyOctaves
    );
    assert_eq!(random.next_long(), -6_819_959_017_335_327_829);
    let mut random = LegacyRandom::new(0);
    assert_eq!(
        create_legacy_fbm(&mut random, i32::MIN, &[1.0]).unwrap_err(),
        NoiseBuildError::LegacyWorkLimit
    );
    assert_eq!(random.next_long(), LegacyRandom::new(0).next_long());
}

#[test]
fn stack_ranges_keep_zero_amplitude_and_flattening_semantics() {
    let octave = || Octave::Perlin(PerlinNoise::new(&mut LegacyRandom::new(0)));
    let mut builder = NoiseStack::builder();
    builder.add(octave(), 1.0, 0.0);
    let zero = builder.build().unwrap();
    assert_eq!(zero.layers().len(), 1);
    assert_eq!(zero.range(), [0.0; 2]);
    let mut flattened = NoiseStack::builder();
    flattened.add_stack(zero.clone(), 1.0, f32::INFINITY);
    // 0 * infinity is NaN: Interval.of refuses it.
    assert_eq!(flattened.build().unwrap_err(), NanRange);
    let mut scaled = NoiseStack::builder();
    scaled.add(octave(), 2.0, 0.5).add_stack(zero, 4.0, 3.0);
    let stack = scaled.build().unwrap();
    assert_eq!(stack.range(), [-1.0, 1.0]);
    assert_eq!(stack.layers()[1].frequency, 4.0);
    let single = PerlinNoise::new(&mut LegacyRandom::new(0));
    assert_eq!(
        stack.get3(1.5, 2.5, 3.5).to_bits(),
        (0.5 * single.get3(3.0, 5.0, 7.0) + 0.0 * single.get3(6.0, 10.0, 14.0)).to_bits()
    );
}
