//! The block-change path and the fluids against the real 26.3 server: `LeafFluidOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) builds scripted worlds in a headless
//! level, changes them with `Level.setBlock` and ticks them through `LevelTicks` and
//! `FlowingFluid`, writing every block set, neighbour update, block change sent, level event,
//! and after each tick the changed blocks and the pending ticks. This replays the scenes on a
//! [`MapLevel`] and compares every line.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafFluidOracle.java fluid.txt
//! HORIZON_FLUID_ORACLE=fluid.txt HORIZON_VANILLA_JAR=<jar> HORIZON_SERVER_BLOCKS_JSON=<blocks.json> \
//!     HORIZON_SERVER_BLOCK_STATES=<block_states.bin> cargo test -p horizon-server fluids_match_java -- --ignored
//! ```

use std::fmt::Write as _;

use super::block_update::set_block;
use super::fluid_tests::{MapLevel, real_rules};
use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};

fn pos(f: &[&str]) -> BlockPos {
    let n = |i: usize| f[i].parse::<i32>().unwrap();
    BlockPos::new(n(0), n(1), n(2))
}

/// The oracle's `snapshot`: the changed cells (by x, y, z), then the pending ticks, sorted.
fn snapshot(
    level: &MapLevel,
    start: &std::collections::HashMap<BlockPos, BlockState>,
    out: &mut Vec<String>,
) {
    out.push(format!("tick {}", level.time));
    let mut all: Vec<BlockPos> = start.keys().copied().collect();
    all.extend(level.cells.keys().filter(|p| !start.contains_key(p)));
    all.sort_by_key(|p| (p.x, p.y, p.z));
    for p in all {
        let a = start.get(&p).copied().unwrap_or(BlockState::AIR);
        let b = level.at(p);
        if a != b {
            out.push(format!("state {} {} {} {}", p.x, p.y, p.z, b.0));
        }
    }
    let rules = &level.rules;
    let mut rows = Vec::new();
    for x in -3..=3 {
        for z in -3..=3 {
            let chunk = ChunkPos::new(x, z);
            for t in level
                .ticks
                .block
                .container(chunk)
                .into_iter()
                .flat_map(|c| c.all())
            {
                rows.push(format!(
                    "bt {} {} {} {} {} {}",
                    t.pos.x,
                    t.pos.y,
                    t.pos.z,
                    rules.blocks.block_name(t.ty),
                    t.trigger_tick - level.time,
                    t.priority.value()
                ));
            }
            for t in level
                .ticks
                .fluid
                .container(chunk)
                .into_iter()
                .flat_map(|c| c.all())
            {
                rows.push(format!(
                    "ft {} {} {} {} {} {}",
                    t.pos.x,
                    t.pos.y,
                    t.pos.z,
                    rules.data.fluid_name(t.ty),
                    t.trigger_tick - level.time,
                    t.priority.value()
                ));
            }
        }
    }
    rows.sort();
    out.extend(rows);
}

#[test]
#[ignore = "needs HORIZON_FLUID_ORACLE and the real 26.3 tables (LeafFluidOracle.java)"]
fn fluids_match_java() {
    use crate::util::random::RandomSource;
    let path = std::env::var("HORIZON_FLUID_ORACLE").expect("HORIZON_FLUID_ORACLE");
    let rules = real_rules().expect("the real tables");
    let text = std::fs::read_to_string(path).unwrap();
    let mut scenes: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if line.starts_with("scene ") {
            scenes.push((line.to_owned(), Vec::new()));
        } else {
            scenes
                .last_mut()
                .expect("a scene first")
                .1
                .push(line.to_owned());
        }
    }
    let (mut failures, mut lines, mut ticks, mut sets) = (Vec::new(), 0usize, 0usize, 0usize);
    for (header, body) in &scenes {
        let f: Vec<&str> = header.split(' ').collect();
        let nether = f[2] == "the_nether";
        let seed: i64 = f[3].parse().unwrap();
        let shape = if nether { (0, 256) } else { (-64, 384) };
        let mut level = MapLevel::new(rules.clone(), shape, seed, nether);
        let mut start = None;
        let mut got: Vec<String> = Vec::new();
        for line in body.iter().filter(|l| !l.starts_with("= ")) {
            got.push(line.clone());
            let f: Vec<&str> = line.split(' ').collect();
            match f[0] {
                "cell" => {
                    let s = BlockState(f[4].parse().unwrap());
                    if s != BlockState::AIR {
                        level.cells.insert(pos(&f[1..4]), s);
                    }
                }
                "op" => {
                    let start = start.get_or_insert_with(|| level.cells.clone());
                    if f[1] == "set" {
                        let s = BlockState(f[5].parse().unwrap());
                        set_block(&mut level, pos(&f[2..5]), s, f[6].parse().unwrap(), 512);
                        got.extend(level.log.drain(..).map(|l| format!("= {l}")));
                    } else {
                        for _ in 0..f[2].parse::<usize>().unwrap() {
                            level.tick();
                            let mut rows = std::mem::take(&mut level.log);
                            snapshot(&level, start, &mut rows);
                            got.extend(rows.into_iter().map(|l| format!("= {l}")));
                            ticks += 1;
                        }
                    }
                }
                other => panic!("unknown row {other}"),
            }
        }
        got.push(format!("= rng {}", level.random.next_long()));
        sets += got.iter().filter(|l| l.starts_with("= set ")).count();
        lines += body.len();
        if let Some(i) = (0..body.len().max(got.len())).find(|&i| body.get(i) != got.get(i)) {
            let mut report = format!("{header}: line {i} differs\n");
            for j in i.saturating_sub(3)..(i + 4).min(body.len().max(got.len())) {
                let _ = writeln!(
                    report,
                    "  java {:?}\n  rust {:?}",
                    body.get(j).map(String::as_str).unwrap_or("-"),
                    got.get(j).map(String::as_str).unwrap_or("-")
                );
            }
            failures.push(report);
        }
    }
    eprintln!(
        "{} scenes, {lines} lines, {ticks} game ticks, {sets} block sets; {} differ",
        scenes.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
