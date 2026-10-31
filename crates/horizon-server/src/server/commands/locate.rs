//! `/locate structure|biome|poi` (`LocateCommand`) on the source's level generator
//! ([`crate::worldgen::locate`]).
//!
//! Stand-ins: the arguments' names are looked up when the command runs (their parse is
//! syntactic, see [`ArgumentType::ResourceOrTag`](crate::command::ArgumentType)); every search
//! starts from a fresh structure check and generates the structure starts it needs (there is no
//! chunk cache to read loaded chunks from yet); a level without a generator (a flat or debug
//! stem, not ported) finds nothing; points of interest are not stored yet, so `poi` finds
//! nothing, as vanilla does with none around. Vanilla logs `Locating element ... took N ms`.

use std::time::Instant;

use super::source::{string_arg, text_arg, translatable, with_formatting};
use super::tree::Key;
use super::{CommandSource, Server};
use crate::azalea::chat::FormattedText;
use crate::azalea::chat::click_event::ClickEvent;
use crate::azalea::chat::hover_event::HoverEvent;
use crate::azalea::chat::style::{ChatFormatting, Style};
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;
use crate::command::{CommandContext, CommandError, ResourceRegistry};
use crate::core::position::BlockPos;
use crate::registry::{Registries, Tags};
use crate::worldgen::locate::{Locator, find_closest_biome_3d};

/// `MAX_STRUCTURE_SEARCH_RADIUS`, `MAX_BIOME_SEARCH_RADIUS` and the biome sample resolutions.
const MAX_STRUCTURE_SEARCH_RADIUS: i32 = 100;
const MAX_BIOME_SEARCH_RADIUS: i32 = 6400;
const BIOME_SAMPLE_RESOLUTION_HORIZONTAL: i32 = 32;
const BIOME_SAMPLE_RESOLUTION_VERTICAL: i32 = 64;

/// What a resource argument names, as `asPrintable` writes it: the identifier, `#` before a tag.
fn printable(tag: bool, id: &str) -> String {
    if tag { format!("#{id}") } else { id.to_owned() }
}

/// `BlockPos.containing(source.getPosition())`.
fn source_pos(source: &CommandSource) -> BlockPos {
    let [x, y, z] = source.position.map(|v| v.floor() as i32);
    BlockPos::new(x, y, z)
}

/// `Mth.floor(float)`.
fn floor(value: f32) -> i32 {
    let truncated = value as i32;
    if value < truncated as f32 {
        truncated - 1
    } else {
        truncated
    }
}

/// `showLocateResult`'s message and the distance it returns: `found_name` is the printable name
/// (with the found element's name after a tag), `include_y` is set for biomes.
pub fn locate_message(
    success_key: &str,
    found_name: &str,
    source: BlockPos,
    found: BlockPos,
    include_y: bool,
) -> (FormattedText, i32) {
    let distance = if include_y {
        let (dx, dy, dz) = (
            f64::from(source.x) - f64::from(found.x),
            f64::from(source.y) - f64::from(found.y),
            f64::from(source.z) - f64::from(found.z),
        );
        let dist_sqr = dx * dx + dy * dy + dz * dz;
        floor((f64::from(dist_sqr as f32).sqrt()) as f32)
    } else {
        // Paper's MC-177381 fix: `(float) Math.hypot`.
        let dx = f64::from(found.x.wrapping_sub(source.x));
        let dz = f64::from(found.z.wrapping_sub(source.z));
        floor(dx.hypot(dz) as f32)
    };
    let displayed_y = if include_y {
        found.y.to_string()
    } else {
        "~".to_owned()
    };
    let inner = translatable(
        "chat.coordinates",
        vec![
            PrimitiveOrComponent::Integer(found.x),
            string_arg(displayed_y.clone()),
            PrimitiveOrComponent::Integer(found.z),
        ],
    );
    let mut coordinates = with_formatting(
        translatable("chat.square_brackets", vec![text_arg(inner)]),
        &[ChatFormatting::Green],
    );
    let style = Style::new()
        .click_event(ClickEvent::SuggestCommand {
            command: format!("/tp @s {} {displayed_y} {}", found.x, found.z),
        })
        .hover_event(HoverEvent::ShowText {
            value: Box::new(translatable("chat.coordinates.tooltip", Vec::new())),
        });
    coordinates.get_base_mut().style.apply(&style);
    let message = translatable(
        success_key,
        vec![
            string_arg(found_name),
            text_arg(coordinates),
            PrimitiveOrComponent::Integer(distance),
        ],
    );
    (message, distance)
}

/// `showLocateResult`: tells the source and returns the distance.
fn show_locate_result(
    server: &mut Server,
    source: &CommandSource,
    success_key: &str,
    found_name: &str,
    found: BlockPos,
    include_y: bool,
) -> i32 {
    let (message, distance) = locate_message(
        success_key,
        found_name,
        source_pos(source),
        found,
        include_y,
    );
    server.send_success(source, message, false);
    distance
}

/// The registry's element ids and tags (`resource_or_tag`'s lookup).
fn lookup(registries: &Registries, registry: ResourceRegistry) -> Option<(Vec<&str>, &Tags)> {
    match registry {
        ResourceRegistry::Biome => {
            let synced = registries.synced(registry.identifier())?;
            Some((
                synced.entries.iter().map(|e| e.id.as_str()).collect(),
                &synced.tags,
            ))
        }
        ResourceRegistry::PoiType => {
            let builtin = registries.builtin(registry.identifier())?;
            Some((
                builtin.entries.iter().map(String::as_str).collect(),
                &builtin.tags,
            ))
        }
        ResourceRegistry::Structure => None,
    }
}

/// `ResourceOrTagArgument.parse`'s lookup, done when the command runs: the element or tag must
/// exist. Returns the element ids it covers (one for an element).
fn resolve(
    server: &Server,
    registry: ResourceRegistry,
    tag: bool,
    id: &str,
) -> Result<Vec<usize>, CommandError> {
    let Some((ids, tags)) = lookup(&server.registries, registry) else {
        return Err(CommandError::resource_not_found(id, registry.identifier()));
    };
    if tag {
        let members = tags
            .get(id)
            .ok_or_else(|| CommandError::resource_tag_not_found(id, registry.identifier()))?;
        Ok(members.iter().map(|&m| m as usize).collect())
    } else {
        let index = ids
            .iter()
            .position(|e| *e == id)
            .ok_or_else(|| CommandError::resource_not_found(id, registry.identifier()))?;
        Ok(vec![index])
    }
}

/// `locateStructure`.
pub(super) fn structure(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let (tag, id) = context
        .resource("structure")
        .expect("the structure argument");
    let name = printable(tag, id);
    let generator = server.world.levels[source.level].generator.clone();
    // `getHolders`: a missing element or tag is `ERROR_STRUCTURE_INVALID`.
    let invalid = || CommandError::locate("commands.locate.structure.invalid", &name);
    let Some(generator) = generator else {
        return Err(invalid());
    };
    let registry = generator.data().structures();
    let target = if tag {
        registry.tag(id).ok_or_else(invalid)?.to_vec()
    } else {
        vec![registry.structure_id(id).ok_or_else(invalid)?]
    };
    let at = source_pos(source);
    let started = Instant::now();
    let nearest = match Locator::new(&generator) {
        Ok(mut locator) => {
            locator.find_nearest_map_structure(&target, at, MAX_STRUCTURE_SEARCH_RADIUS, false)
        }
        Err(error) => {
            tracing::warn!("Locating structures: {error}");
            None
        }
    };
    let elapsed = started.elapsed();
    let Some((found, structure)) = nearest else {
        return Err(CommandError::locate(
            "commands.locate.structure.not_found",
            &name,
        ));
    };
    let registered = registry.structure(structure).id.clone();
    let found_name = if tag {
        format!("{name} ({registered})")
    } else {
        id.to_owned()
    };
    tracing::info!(
        "Locating element {found_name} took {} ms",
        elapsed.as_millis()
    );
    Ok(show_locate_result(
        server,
        source,
        "commands.locate.structure.success",
        &found_name,
        found,
        false,
    ))
}

/// `locateBiome`.
pub(super) fn biome(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let (tag, id) = context.resource("biome").expect("the biome argument");
    let allowed = resolve(server, ResourceRegistry::Biome, tag, id)?;
    let name = printable(tag, id);
    let not_found = || CommandError::locate("commands.locate.biome.not_found", &name);
    let Some(generator) = server.world.levels[source.level].generator.clone() else {
        return Err(not_found());
    };
    let at = source_pos(source);
    let started = Instant::now();
    let nearest = find_closest_biome_3d(
        &generator,
        at,
        MAX_BIOME_SEARCH_RADIUS,
        (
            BIOME_SAMPLE_RESOLUTION_HORIZONTAL,
            BIOME_SAMPLE_RESOLUTION_VERTICAL,
        ),
        |biome| allowed.contains(&(biome as usize)),
    );
    let elapsed = started.elapsed();
    let Some((found, biome)) = nearest else {
        return Err(not_found());
    };
    let registered = server
        .registries
        .synced(ResourceRegistry::Biome.identifier())
        .and_then(|r| r.entries.get(biome as usize))
        .map_or_else(String::new, |e| e.id.clone());
    let found_name = if tag {
        format!("{name} ({registered})")
    } else {
        name.clone()
    };
    tracing::info!(
        "Locating element {found_name} took {} ms",
        elapsed.as_millis()
    );
    Ok(show_locate_result(
        server,
        source,
        "commands.locate.biome.success",
        &found_name,
        found,
        true,
    ))
}

/// `locatePoi`: there is no point-of-interest storage yet, so nothing is ever found
/// (`PoiManager.findClosestWithType` over no stored points).
pub(super) fn poi(
    server: &mut Server,
    _source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let (tag, id) = context.resource("poi").expect("the poi argument");
    resolve(server, ResourceRegistry::PoiType, tag, id)?;
    Err(CommandError::locate(
        "commands.locate.poi.not_found",
        &printable(tag, id),
    ))
}
