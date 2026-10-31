//! `ChunkLevel`'s levels and vanilla's ticket types against the real 26.3 server:
//! `LeafChunkTicketsOracle.java` (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) prints
//! them from the jar; this compares every row it knows.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafChunkTicketsOracle.java tickets.txt
//! HORIZON_TICKETS_ORACLE=tickets.txt cargo test -p horizon-server chunk_tickets_match_java -- --ignored
//! ```

use super::ticket::{
    BLOCK_TICKING_LEVEL, ENTITY_TICKING_LEVEL, FULL_LEVEL, FullChunkStatus, LOADED_LEVEL,
    MAX_LEVEL, TicketType,
};

fn status_name(status: FullChunkStatus) -> &'static str {
    match status {
        FullChunkStatus::Inaccessible => "INACCESSIBLE",
        FullChunkStatus::Full => "FULL",
        FullChunkStatus::BlockTicking => "BLOCK_TICKING",
        FullChunkStatus::EntityTicking => "ENTITY_TICKING",
    }
}

#[test]
#[ignore = "needs HORIZON_TICKETS_ORACLE (LeafChunkTicketsOracle.java on the 26.3 jar)"]
fn chunk_tickets_match_java() {
    let path = std::env::var_os("HORIZON_TICKETS_ORACLE").expect("HORIZON_TICKETS_ORACLE");
    let text = std::fs::read_to_string(path).expect("the oracle's output");
    let mut checked = 0;
    for line in text.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        match fields.as_slice() {
            ["level", status, level] => {
                let ours = match *status {
                    "INACCESSIBLE" => MAX_LEVEL,
                    "FULL" => FULL_LEVEL,
                    "BLOCK_TICKING" => BLOCK_TICKING_LEVEL,
                    "ENTITY_TICKING" => ENTITY_TICKING_LEVEL,
                    other => panic!("unknown status {other}"),
                };
                assert_eq!(ours.to_string(), *level, "{line}");
            }
            ["by_status", "empty", level] => assert_eq!(LOADED_LEVEL.to_string(), *level),
            ["by_status", "full", level] => assert_eq!(FULL_LEVEL.to_string(), *level),
            ["full_status", level, status] => {
                let level: u8 = level.parse().unwrap();
                assert_eq!(status_name(FullChunkStatus::from_level(level)), *status);
            }
            // A chunk has a holder exactly where it has a generation status.
            ["generation_status", level, status] => {
                let level: u8 = level.parse().unwrap();
                assert_eq!(level <= MAX_LEVEL, *status != "null", "{line}");
            }
            ["ticket", "player_spawn", timeout, _] => {
                assert_eq!(TicketType::PlayerSpawn.timeout(1).to_string(), *timeout);
            }
            ["ticket", "unknown", timeout, _] => {
                assert_eq!(TicketType::Unknown.timeout(1).to_string(), *timeout);
            }
            _ => continue,
        }
        checked += 1;
    }
    // 4 levels, 2 statuses, 51 + 51 level rows, 2 ticket types.
    assert_eq!(checked, 110);
}
