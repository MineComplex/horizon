//! `NoiseSpawnFinder`: where a noise level's spawn search starts (`NoiseBasedChunkGenerator.
//! getOrigin`, which `MinecraftServer.setInitialSpawn` spirals out from). The noise settings'
//! `spawn_target` points are sampled around 0 0; the column whose climate fits one of them best
//! wins, the squared distance to 0 0 breaking ties. Computed once when a level is created.
//!
//! Copied from Horizon's `horizon-world::worldgen::spawn_finder`, on the level's compiled
//! spawn-target functions ([`LevelDensity::spawn_target`]).

use crate::util::jmath;
use crate::worldgen::climate::quantize;
use crate::worldgen::data::LevelDensity;
use crate::worldgen::density::DensityContext;

#[derive(Clone, Copy, Debug)]
struct Candidate {
    x: i32,
    z: i32,
    fitness: i64,
}

/// `findSpawnPosition`: the chosen block column `[x, z]` (y is 0), or `None` without spawn
/// targets (vanilla then uses the chunk at 0 0). Uses a caching context, as `getOrigin` does.
pub fn find_spawn_position(density: &LevelDensity) -> Option<[i32; 2]> {
    if density.spawn_target.is_empty() {
        return None;
    }
    let mut context = DensityContext::cached();
    let mut best = fitness(density, &mut context, 0, 0);
    radial_search(density, &mut context, &mut best, 2048.0, 512.0);
    radial_search(density, &mut context, &mut best, 512.0, 32.0);
    Some([best.x, best.z])
}

/// `radialSearch`: rings `increment` apart around the best column so far, sampled every
/// `increment` blocks of arc (float angle and radius, `Math.sin`/`cos` in double).
fn radial_search(
    density: &LevelDensity,
    context: &mut DensityContext,
    best: &mut Candidate,
    max_radius: f32,
    increment: f32,
) {
    let (origin_x, origin_z) = (best.x, best.z);
    let mut angle = 0f32;
    let mut radius = increment;
    while radius <= max_radius {
        let x = origin_x.wrapping_add((jmath::sin(f64::from(angle)) * f64::from(radius)) as i32);
        let z = origin_z.wrapping_add((jmath::cos(f64::from(angle)) * f64::from(radius)) as i32);
        let candidate = fitness(density, context, x, z);
        if candidate.fitness < best.fitness {
            *best = candidate;
        }
        angle += increment / radius;
        if f64::from(angle) > std::f64::consts::PI * 2.0 {
            angle = 0.0;
            radius += increment;
        }
    }
}

/// `getSpawnPositionAndFitness`: the best target's fitness at the column's quart corner (y 0),
/// times 2048², plus the squared distance to 0 0.
fn fitness(density: &LevelDensity, context: &mut DensityContext, x: i32, z: i32) -> Candidate {
    let position = [(x >> 2) << 2, 0, (z >> 2) << 2];
    let best = density
        .spawn_target
        .iter()
        .map(|point| {
            point.iter().fold(0i64, |sum, (function, parameter)| {
                let value = density.program.sample(context, *function, position);
                let distance = parameter.distance(quantize(value));
                sum.wrapping_add(distance.wrapping_mul(distance))
            })
        })
        .fold(i64::MAX, i64::min);
    let bias = i64::from(x) * i64::from(x) + i64::from(z) * i64::from(z);
    Candidate {
        x,
        z,
        fitness: best.wrapping_mul(2048 * 2048).wrapping_add(bias),
    }
}
