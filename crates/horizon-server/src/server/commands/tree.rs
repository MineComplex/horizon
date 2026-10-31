//! The vanilla command tree for the commands ported so far, as `Commands`' constructor registers it
//! (26.3 sources, Leaf's final tree for a dedicated server): the same literals, arguments,
//! permission levels and registration order, so the commands packet is what the vanilla tree
//! restricted to these commands sends.
//!
//! What Brigadier keeps as lambdas is a [`Key`]: the command to run, a requirement, a suggestion
//! provider. The server matches on it (`Host::run` in the parent module).
//!
//! Not in the tree yet (their commands are not ported): `/time of <clock>` and the timeline queries
//! (they take registry arguments), everything that needs entities or blocks.

use crate::command::{
    ArgumentType, CommandTree, NodeBuilder, NodeId, ResourceRegistry, argument, literal,
};

/// A key of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A requirement: `Commands.hasPermission(LEVEL_x)` with the level's id (0 all .. 4 owners).
    Permission(u8),
    Command(Cmd),
    /// A suggestion provider. Every custom provider is `minecraft:ask_server` on the wire.
    Suggest(Suggest),
}

/// The commands, one per `Command` lambda of vanilla.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    DifficultyQuery,
    /// `setDifficulty` with the difficulty's id.
    DifficultySet(u8),
    List,
    ListUuids,
    /// `locate structure|biome|poi`.
    LocateStructure,
    LocateBiome,
    LocatePoi,
    Seed,
    /// `tick query`, `rate`, `step`, `step <time>`, `step stop`, `sprint <time>`, `sprint stop`,
    /// `unfreeze`, `freeze`.
    TickQuery,
    TickRate,
    TickStep,
    TickStepTime,
    TickStepStop,
    TickSprintTime,
    TickSprintStop,
    TickUnfreeze,
    TickFreeze,
    TimeSetTicks,
    TimeSetMarker,
    TimeAdd,
    TimePause,
    TimeResume,
    TimeRate,
    TimeQueryTime,
    TimeQueryGameTime,
    /// `weather clear|rain|thunder [<duration>]`.
    WeatherClear,
    WeatherClearFor,
    WeatherRain,
    WeatherRainFor,
    WeatherThunder,
    WeatherThunderFor,
    Deop,
    Op,
    SaveAll,
    SaveAllFlush,
    SaveOff,
    SaveOn,
    Stop,
    WhitelistOn,
    WhitelistOff,
    WhitelistList,
    WhitelistAdd,
    WhitelistRemove,
    WhitelistReload,
}

/// The providers of vanilla's `.suggests(...)` lambdas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Suggest {
    WhitelistAdd,
    WhitelistRemove,
    Op,
    Deop,
    TickRate,
    TickStep,
    TickSprint,
    TimeMarkers,
}

const LEVEL_GAMEMASTERS: u8 = 2;
const LEVEL_ADMINS: u8 = 3;
const LEVEL_OWNERS: u8 = 4;

fn requires(level: u8) -> Key {
    Key::Permission(level)
}

fn run(cmd: Cmd) -> Key {
    Key::Command(cmd)
}

fn duration(command: Cmd) -> NodeBuilder<Key> {
    argument("duration", ArgumentType::Time { min: 1 }).executes(run(command))
}

fn targets(command: Cmd, suggest: Suggest) -> NodeBuilder<Key> {
    argument("targets", ArgumentType::GameProfile)
        .suggests(Key::Suggest(suggest))
        .executes(run(command))
}

/// `/time`'s clock nodes (`addClockNodes`), for the default clock.
fn clock_nodes(base: NodeBuilder<Key>) -> NodeBuilder<Key> {
    base.then(
        literal("set")
            .then(argument("time", ArgumentType::time()).executes(run(Cmd::TimeSetTicks)))
            .then(
                argument("timemarker", ArgumentType::Identifier)
                    .suggests(Key::Suggest(Suggest::TimeMarkers))
                    .executes(run(Cmd::TimeSetMarker)),
            ),
    )
    .then(
        literal("add").then(
            argument("time", ArgumentType::Time { min: i32::MIN }).executes(run(Cmd::TimeAdd)),
        ),
    )
    .then(literal("pause").executes(run(Cmd::TimePause)))
    .then(literal("resume").executes(run(Cmd::TimeResume)))
    .then(
        literal("rate").then(
            argument(
                "rate",
                ArgumentType::Float {
                    min: 1.0E-5,
                    max: 1000.0,
                },
            )
            .executes(run(Cmd::TimeRate)),
        ),
    )
    .then(literal("query").then(literal("time").executes(run(Cmd::TimeQueryTime))))
}

/// The tree: vanilla's commands in `Commands`' registration order, then Paper's `minecraft:`
/// alias of each (a literal that redirects to the flattened target and carries its command and
/// requirement).
pub fn build_tree() -> CommandTree<Key> {
    let mut tree = CommandTree::new();

    let mut difficulty = literal("difficulty");
    for (id, name) in ["peaceful", "easy", "normal", "hard"]
        .into_iter()
        .enumerate()
    {
        difficulty = difficulty.then(literal(name).executes(run(Cmd::DifficultySet(id as u8))));
    }
    tree.register(
        difficulty
            .requires(requires(LEVEL_GAMEMASTERS))
            .executes(run(Cmd::DifficultyQuery)),
    );

    tree.register(
        literal("list")
            .executes(run(Cmd::List))
            .then(literal("uuids").executes(run(Cmd::ListUuids))),
    );

    tree.register(
        literal("locate")
            .requires(requires(LEVEL_GAMEMASTERS))
            .then(
                literal("structure").then(
                    argument(
                        "structure",
                        ArgumentType::ResourceOrTagKey(ResourceRegistry::Structure),
                    )
                    .executes(run(Cmd::LocateStructure)),
                ),
            )
            .then(
                literal("biome").then(
                    argument(
                        "biome",
                        ArgumentType::ResourceOrTag(ResourceRegistry::Biome),
                    )
                    .executes(run(Cmd::LocateBiome)),
                ),
            )
            .then(
                literal("poi").then(
                    argument(
                        "poi",
                        ArgumentType::ResourceOrTag(ResourceRegistry::PoiType),
                    )
                    .executes(run(Cmd::LocatePoi)),
                ),
            ),
    );

    tree.register(
        literal("seed")
            .requires(requires(LEVEL_GAMEMASTERS))
            .executes(run(Cmd::Seed)),
    );

    tree.register(
        literal("tick")
            .requires(requires(LEVEL_ADMINS))
            .then(literal("query").executes(run(Cmd::TickQuery)))
            .then(
                literal("rate").then(
                    argument(
                        "rate",
                        ArgumentType::Float {
                            min: 1.0,
                            max: 10000.0,
                        },
                    )
                    .suggests(Key::Suggest(Suggest::TickRate))
                    .executes(run(Cmd::TickRate)),
                ),
            )
            .then(
                literal("step")
                    .executes(run(Cmd::TickStep))
                    .then(literal("stop").executes(run(Cmd::TickStepStop)))
                    .then(
                        argument("time", ArgumentType::Time { min: 1 })
                            .suggests(Key::Suggest(Suggest::TickStep))
                            .executes(run(Cmd::TickStepTime)),
                    ),
            )
            .then(
                literal("sprint")
                    .then(literal("stop").executes(run(Cmd::TickSprintStop)))
                    .then(
                        argument("time", ArgumentType::Time { min: 1 })
                            .suggests(Key::Suggest(Suggest::TickSprint))
                            .executes(run(Cmd::TickSprintTime)),
                    ),
            )
            .then(literal("unfreeze").executes(run(Cmd::TickUnfreeze)))
            .then(literal("freeze").executes(run(Cmd::TickFreeze))),
    );

    // Registered twice and merged: the clock nodes, then `query gametime`.
    tree.register(clock_nodes(
        literal("time").requires(requires(LEVEL_GAMEMASTERS)),
    ));
    tree.register(
        literal("time")
            .requires(requires(LEVEL_GAMEMASTERS))
            .then(literal("query").then(literal("gametime").executes(run(Cmd::TimeQueryGameTime)))),
    );

    tree.register(
        literal("weather")
            .requires(requires(LEVEL_GAMEMASTERS))
            .then(
                literal("clear")
                    .executes(run(Cmd::WeatherClear))
                    .then(duration(Cmd::WeatherClearFor)),
            )
            .then(
                literal("rain")
                    .executes(run(Cmd::WeatherRain))
                    .then(duration(Cmd::WeatherRainFor)),
            )
            .then(
                literal("thunder")
                    .executes(run(Cmd::WeatherThunder))
                    .then(duration(Cmd::WeatherThunderFor)),
            ),
    );

    // `commandSelection.includeDedicated`.
    tree.register(
        literal("deop")
            .requires(requires(LEVEL_ADMINS))
            .then(targets(Cmd::Deop, Suggest::Deop)),
    );
    tree.register(
        literal("op")
            .requires(requires(LEVEL_ADMINS))
            .then(targets(Cmd::Op, Suggest::Op)),
    );
    tree.register(
        literal("save-all")
            .requires(requires(LEVEL_OWNERS))
            .executes(run(Cmd::SaveAll))
            .then(literal("flush").executes(run(Cmd::SaveAllFlush))),
    );
    tree.register(
        literal("save-off")
            .requires(requires(LEVEL_OWNERS))
            .executes(run(Cmd::SaveOff)),
    );
    tree.register(
        literal("save-on")
            .requires(requires(LEVEL_OWNERS))
            .executes(run(Cmd::SaveOn)),
    );
    tree.register(
        literal("stop")
            .requires(requires(LEVEL_OWNERS))
            .executes(run(Cmd::Stop)),
    );
    tree.register(
        literal("whitelist")
            .requires(requires(LEVEL_ADMINS))
            .then(literal("on").executes(run(Cmd::WhitelistOn)))
            .then(literal("off").executes(run(Cmd::WhitelistOff)))
            .then(literal("list").executes(run(Cmd::WhitelistList)))
            .then(literal("add").then(targets(Cmd::WhitelistAdd, Suggest::WhitelistAdd)))
            .then(literal("remove").then(targets(Cmd::WhitelistRemove, Suggest::WhitelistRemove)))
            .then(literal("reload").executes(run(Cmd::WhitelistReload))),
    );

    add_namespaced_aliases(&mut tree);
    tree
}

/// Paper's "legacy minecraft namespace commands" (`Commands` constructor, `modern` false): for each
/// root command `minecraft:<name>`, executing the flattened target's command with its requirement,
/// redirecting to the flattened target.
fn add_namespaced_aliases(tree: &mut CommandTree<Key>) {
    let roots: Vec<NodeId> = tree.node(NodeId::ROOT).children().to_vec();
    for id in roots {
        let mut target = id;
        while let Some(next) = tree.node(target).redirect() {
            target = next;
        }
        let node = tree.node(target);
        let mut alias = literal(&format!("minecraft:{}", tree.node(id).name()));
        if let Some(command) = node.command() {
            alias = alias.executes(command);
        }
        if let Some(requirement) = node.requirement() {
            alias = alias.requires(requirement);
        }
        tree.register(alias.redirect(target));
    }
}
