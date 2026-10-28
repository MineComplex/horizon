//! Sound sources (`SoundSource`), by name.

/// SoundSource names in vanilla order.
pub const SOUND_SOURCES: [&str; 11] = [
    "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
    "voice", "ui",
];

/// The SoundSource an event plays on. Vanilla callers pick the source; this
/// follows their conventions by event family.
pub fn sound_source(event: &str) -> usize {
    let e = event.trim_start_matches("minecraft:");
    let index = |name: &str| SOUND_SOURCES.iter().position(|n| *n == name).unwrap_or(0);
    const HOSTILE: [&str; 25] = [
        "zombie",
        "skeleton",
        "creeper",
        "spider",
        "cave_spider",
        "enderman",
        "witch",
        "slime",
        "magma_cube",
        "blaze",
        "ghast",
        "drowned",
        "husk",
        "stray",
        "phantom",
        "pillager",
        "vindicator",
        "evoker",
        "illusioner",
        "ravager",
        "guardian",
        "elder_guardian",
        "silverfish",
        "zombified_piglin",
        "wither_skeleton",
    ];
    if e == "block.potent_sulfur.noxious_gas" {
        // PotentSulfurBlock.animateTick, its only caller, plays it as AMBIENT.
        index("ambient")
    } else if e.starts_with("music_disc.") {
        index("record")
    } else if e.starts_with("music.") {
        index("music")
    } else if e.starts_with("weather.") || e.starts_with("entity.lightning_bolt") {
        index("weather")
    } else if e.starts_with("block.") {
        index("block")
    } else if e.starts_with("ambient.") {
        index("ambient")
    } else if e.starts_with("ui.") {
        index("ui")
    } else if e.starts_with("entity.player.")
        || e.starts_with("item.")
        || e.starts_with("entity.item.")
        || e.starts_with("entity.experience_orb")
        || e.starts_with("entity.arrow")
    {
        index("player")
    } else if let Some(rest) = e.strip_prefix("entity.") {
        let mob = rest.split('.').next().unwrap_or("");
        if HOSTILE.contains(&mob) {
            index("hostile")
        } else {
            index("neutral")
        }
    } else {
        index("master")
    }
}
