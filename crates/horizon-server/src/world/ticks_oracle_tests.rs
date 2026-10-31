//! `LevelTicks`, `LevelChunkTicks` and `SavedTick`'s codec against the real 26.3 server:
//! `LeafTicksOracle.java` (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) drives the real
//! classes with synthetic schedules and writes the script with its answers; this replays each
//! `do` line and compares the `= ` lines after it.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafTicksOracle.java ticks.txt
//! HORIZON_TICKS_ORACLE=ticks.txt cargo test -p horizon-server ticks_match_java -- --ignored
//! ```

use std::collections::HashSet;

use super::chunk::serial::identifier;
use super::ticks::{
    LevelChunkTicks, LevelTicks, SavedTick, ScheduledTick, TickArea, TickPriority,
    filter_ticks_for_chunk, read_saved_ticks, write_saved_ticks,
};
use crate::block::BlockId;
use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt::parse_snbt;

const NAMES: [&str; 4] = [
    "minecraft:stone",
    "minecraft:dirt",
    "minecraft:sand",
    "minecraft:gravel",
];

fn resolve(name: &str) -> Result<BlockId, String> {
    let id = identifier(name)?;
    NAMES
        .iter()
        .position(|n| *n == id)
        .map(|i| BlockId(i as u32))
        .ok_or(id)
}

fn pos(f: &[&str]) -> BlockPos {
    let n = |i: usize| f[i].parse::<i32>().unwrap();
    BlockPos::new(n(0), n(1), n(2))
}

fn saved_line(t: &SavedTick<BlockId>, names: bool) -> String {
    let ty = if names {
        NAMES[t.ty.0 as usize].to_owned()
    } else {
        t.ty.0.to_string()
    };
    format!(
        "saved {ty} {} {} {} {} {}",
        t.pos.x,
        t.pos.y,
        t.pos.z,
        t.delay,
        t.priority.value()
    )
}

struct Replay {
    ticks: LevelTicks<BlockId>,
    ticking: HashSet<i64>,
    time: i64,
    sub: i64,
    callback: bool,
}

impl Replay {
    fn new(callback: bool) -> Self {
        Self {
            ticks: LevelTicks::default(),
            ticking: HashSet::new(),
            time: 100,
            sub: 0,
            callback,
        }
    }

    fn schedule(&mut self, f: &[&str]) {
        let n = |i: usize| f[i].parse::<i32>().unwrap();
        self.ticks.schedule(ScheduledTick {
            ty: BlockId(n(0) as u32),
            pos: pos(&f[1..4]),
            trigger_tick: self.time + i64::from(n(4)),
            priority: TickPriority::by_value(n(5)),
            sub_tick_order: self.sub,
        });
        self.sub += 1;
    }

    fn tick(&mut self, max: i32) -> Vec<String> {
        let mut got = Vec::new();
        let (time, callback) = (self.time, self.callback);
        let (ticking, sub) = (&self.ticking, &mut self.sub);
        self.ticks.tick(
            time,
            max,
            |key| ticking.contains(&key),
            |ticks, p, ty| {
                got.push(format!("ran {} {} {} {}", ty.0, p.x, p.y, p.z));
                if !callback {
                    return;
                }
                let east = p.offset(1, 0, 0);
                let will = ticks.will_tick_this_tick(east, ty);
                got.push(format!(
                    "will {} {} {} {} {will}",
                    ty.0, east.x, east.y, east.z
                ));
                if (p.x + p.z).rem_euclid(5) == 0 {
                    ticks.schedule(ScheduledTick {
                        ty,
                        pos: p.offset(0, 1, 0),
                        trigger_tick: time + i64::from(p.x.rem_euclid(3)),
                        priority: TickPriority::by_value(p.y.rem_euclid(7) - 3),
                        sub_tick_order: *sub,
                    });
                    *sub += 1;
                }
            },
        );
        got.push(format!("count {}", self.ticks.count()));
        got
    }
}

#[test]
#[ignore = "needs HORIZON_TICKS_ORACLE (LeafTicksOracle.java on the 26.3 jar)"]
fn ticks_match_java() {
    let path = std::env::var_os("HORIZON_TICKS_ORACLE").expect("HORIZON_TICKS_ORACLE");
    let text = std::fs::read_to_string(path).expect("the oracle's output");
    let mut lines = text.lines().peekable();
    let mut replay = Replay::new(true);
    let mut tolerant = false;
    let (mut ops, mut compared, mut ran) = (0, 0, 0);
    while let Some(line) = lines.next() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("scenario ") {
            tolerant = rest.ends_with("tolerant");
            replay = Replay::new(!tolerant);
            continue;
        }
        let op = line.strip_prefix("do ").expect("a do line");
        let (name, args) = op.split_once(' ').unwrap_or((op, ""));
        let f: Vec<&str> = args.split(' ').collect();
        let n = |i: usize| f[i].parse::<i32>().unwrap();
        let mut got: Vec<String> = Vec::new();
        match name {
            "container" => {
                let mut pending = Vec::new();
                for _ in 0..n(2) {
                    let row = lines.next().unwrap();
                    let p: Vec<&str> = row.strip_prefix("pend ").unwrap().split(' ').collect();
                    pending.push(SavedTick {
                        ty: BlockId(p[0].parse().unwrap()),
                        pos: pos(&p[1..4]),
                        delay: p[4].parse().unwrap(),
                        priority: TickPriority::by_value(p[5].parse().unwrap()),
                    });
                }
                let container = if pending.is_empty() {
                    LevelChunkTicks::default()
                } else {
                    LevelChunkTicks::from_pending(pending)
                };
                replay
                    .ticks
                    .add_container(ChunkPos::new(n(0), n(1)), container);
            }
            "ticking" => {
                let key = ChunkPos::new(n(0), n(1)).pack();
                if n(2) == 1 {
                    replay.ticking.insert(key);
                } else {
                    replay.ticking.remove(&key);
                }
            }
            "unpack" => replay
                .ticks
                .unpack_container(ChunkPos::new(n(0), n(1)), replay.time),
            "schedule" => replay.schedule(&f),
            "has" => got.push(format!(
                "has {}",
                replay
                    .ticks
                    .has_scheduled_tick(pos(&f[1..4]), BlockId(n(0) as u32))
            )),
            "tick" => got = replay.tick(n(0)),
            "advance" => replay.time += 1,
            "remove" => {
                let removed = replay.ticks.remove_container(ChunkPos::new(n(0), n(1)));
                got.push(format!("removed {}", removed.is_some()));
            }
            "pack" => {
                let p = ChunkPos::new(n(0), n(1));
                let time = replay.time;
                let packed = replay
                    .ticks
                    .container_mut(p)
                    .map(|c| c.pack(time))
                    .unwrap_or_default();
                got.extend(packed.iter().map(|t| saved_line(t, false)));
                got.push(format!("packed {}", packed.len()));
            }
            "clear" => replay.ticks.clear_area(&TickArea {
                min: pos(&f[0..3]),
                max: pos(&f[3..6]),
            }),
            "copy" => replay.ticks.copy_area(
                &TickArea {
                    min: pos(&f[0..3]),
                    max: pos(&f[3..6]),
                },
                (n(6), n(7), n(8)),
            ),
            "count" => got.push(format!("count {}", replay.ticks.count())),
            "codec_read" => {
                let chunk = ChunkPos::new(n(0), n(1));
                let snbt = args.splitn(3, ' ').nth(2).unwrap();
                let tag = parse_snbt(&format!("{{v:{snbt}}}")).unwrap().0;
                let mut read = read_saved_ticks(tag.get("v"), resolve);
                filter_ticks_for_chunk(&mut read, chunk);
                got.extend(read.iter().map(|t| saved_line(t, true)));
                got.push(format!("read {}", read.len()));
            }
            "codec_write" => {
                let tag = parse_snbt(&format!("{{v:{args}}}")).unwrap().0;
                let read = read_saved_ticks(tag.get("v"), resolve);
                let written = write_saved_ticks(&read, |b| NAMES[b.0 as usize].to_owned());
                got.push(format!("tag {written}"));
            }
            other => panic!("unknown op {other}"),
        }
        let mut want: Vec<String> = Vec::new();
        while let Some(next) = lines.peek() {
            if next.starts_with('#') {
                lines.next();
                continue;
            }
            let Some(rest) = next.strip_prefix("= ") else {
                break;
            };
            want.push(rest.to_owned());
            lines.next();
        }
        if tolerant && name == "tick" {
            got.sort();
            want.sort();
        }
        assert_eq!(got, want, "after `do {op}` (op {ops})");
        ran += want.iter().filter(|w| w.starts_with("ran ")).count();
        compared += want.len();
        ops += 1;
    }
    println!("{ops} ops, {compared} answers, {ran} ticks run");
    assert!(ops > 1000 && ran > 400, "{ops} ops, {ran} ticks run");
}
