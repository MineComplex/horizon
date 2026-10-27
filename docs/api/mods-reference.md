<!-- Generated from crates/horizon-mod-sdk/wit/horizon.wit and the manifest's capabilities by crates/horizon-mods/tests/api_reference.rs; a test fails when this file is out of date. Update it with: HORIZON_WRITE_DOCS=1 cargo test -p horizon-mods --features pack --test api_reference -->
# Mod API reference

The contract between Horizon and its mods: the WIT package `horizon:mods@0.1.0`, every interface, type, function and hook in it, and the capabilities a manifest may list. The guide is [mods.md](../mods.md); stability and versions: [stability.md](stability.md). Manifest `api` lines this game serves: `0.1`, `0.2`, `0.3`, `0.4`.

## Capabilities

| In `capabilities` | Grants | The player reads "May …" |
| --- | --- | --- |
| `game-read` | [`game`](#game) | see your position, health and the time |
| `chat` | [`chat`](#chat) | show you chat messages |
| `hud` | [`hud`](#hud) | draw text on your screen |
| `storage` | [`storage`](#storage) | save its own settings to disk |
| `command` | [`command`](#command) | run commands as you, with your permissions |
| `registries` | data files (`assets/`, `data/`), no interface | add blocks and items to your worlds |
| `ui` | [`ui`](#ui) | show its own screens and a settings page |
| `render` | [`render`](#render) | draw on your screen, shake and zoom the camera |
| `input` | [`input`](#input) | see the keys you hold and your clicks, take over the use button on its own items and the mouse wheel, and add keys |
| `players` | [`players`](#players) | see who is on the server and message them |
| `world` | [`world`](#world) | read and change the world's blocks |
| `recipes` | [`recipes`](#recipes) | read the game's recipes and items and which recipes you know, place recipes in your crafting grid and, in creative mode, give you items |
| `plugin` | [`plugin-server`](#plugin-server) | run as a server plugin: see and change players, worlds, entities, items and permissions, and hear and cancel what happens on the server |

## World `horizon-mod`

A client mod: runs in the player's game.

- Imports (what the game offers): `log`, `game`, `chat`, `hud`, `storage`, `command`
- Exports (what the mod provides): `events`

## World `horizon-server-mod`

A server mod (manifest `side = "server"`, `api = "0.3"`, experimental): runs where the world is simulated, on the dedicated server, a LAN host and single player's own server.

- Imports (what the game offers): `log`, `storage`, `players`, `world`
- Exports (what the mod provides): `server-events`

## World `horizon-ui-mod`

A mod with widgets (capability `ui`): the SDK builds every mod against this world. Mods built for `horizon-mod` alone (no `ui-events` export) still load and simply hear nothing of their widgets.

- Imports (what the game offers): `ui`, `render`, `input`, `recipes`, `browser`, `log`, `game`, `chat`, `hud`, `storage`, `command`
- Exports (what the mod provides): `ui-events`, `events`

## World `ui-host`

The game's side of `horizon-ui-mod` beyond `horizon-mod`.

- Imports (what the game offers): `ui`
- Exports (what the mod provides): `ui-events`

## World `render-host`

The game's side of capability `render` (imports only).

- Imports (what the game offers): `render`, `input`
- Exports (what the mod provides): 

## World `recipes-host`

The game's side of capability `recipes` (imports only).

- Imports (what the game offers): `recipes`, `browser`
- Exports (what the mod provides): 

## World `horizon-plugin`

A plugin (manifest `side = "server"`, `api = "0.4"`, capability `plugin`).

- Imports (what the game offers): `log`, `storage`, `plugin-server`, `plugin-types`, `plugin-players`, `plugin-worlds`, `plugin-entities`, `plugin-items`, `plugin-inventories`, `plugin-scheduler`, `plugin-permissions`, `plugin-scoreboards`, `plugin-bossbars`, `plugin-messaging`, `plugin-files`, `plugin-event-types`, `plugin-listeners`, `plugin-commands`
- Exports (what the mod provides): `plugin`

## `log`

Access: always granted.

The mod's log, in the game's log under the mod's id. Always granted.

### enum `level`

| Case | |
| --- | --- |
| `debug` |  |
| `info` |  |
| `warn` |  |
| `error` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `write` | `func(level: level, message: string)` | One line in the game's log as `[mod <id>] message`, at that level. |

## `game`

Access: capability `game-read`.

Capability `game-read`: the local player, the world clock and the block the player looks at.

### record `player`

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` | Degrees, as F3 shows them. |
| `pitch` | `f32` | Degrees: -90 looks straight up, 90 straight down. |
| `health` | `f32` | Hit points as vanilla counts them: 20 is full health without effects. |
| `food` | `u32` | 0 to 20, as the hunger bar. |
| `dimension` | `string` | `minecraft:overworld`, `minecraft:the_nether`, ... |

### record `clock`

| Field | Type | |
| --- | --- | --- |
| `game-time` | `u64` | Ticks since the world was created. |
| `day-time` | `u64` | 24000 ticks per day; 0 is 6:00 on the first day. |

### record `hit`

A block along the player's view.

| Field | Type | |
| --- | --- | --- |
| `block-x` | `s32` | The block hit. |
| `block-y` | `s32` |  |
| `block-z` | `s32` |  |
| `x` | `f64` | The point on its face where the view meets it. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `block` | `string` | `minecraft:stone`, ... |

| Function | Signature | What it does |
| --- | --- | --- |
| `get-player` | `func() -> option<player>` | None on the title screen. |
| `get-clock` | `func() -> option<clock>` | None on the title screen. |
| `raycast` | `func(distance: f32) -> option<hit>` | The first block along the player's view (fluids pass), up to `distance` blocks (at most 1024; past the loaded chunks, across the distant terrain when LOD Distance is on), as of this tick. None when nothing is hit, and on the title screen. |

## `chat`

Access: capability `chat`.

Capability `chat`: messages only the local player sees.

| Function | Signature | What it does |
| --- | --- | --- |
| `show` | `func(text: string)` | Shown in chat after the mod's name, like a system message. |

## `hud`

Access: capability `hud`.

Capability `hud`: one panel of text lines per mod, drawn by the game every frame until the mod changes or clears it (no call per frame).

### enum `corner`

| Case | |
| --- | --- |
| `top-left` |  |
| `top-right` |  |
| `bottom-left` |  |
| `bottom-right` |  |

### record `line`

| Field | Type | |
| --- | --- | --- |
| `text` | `string` | Plain text, up to 256 characters. |
| `color` | `u32` | 0xRRGGBB. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-panel` | `func(corner: corner, lines: list<line>)` | Replaces the mod's panel: up to 16 lines in that corner of the screen. |
| `clear` | `func()` | Removes the mod's panel. |

## `storage`

Access: capability `storage`.

Capability `storage`: a per-mod folder of small key/value files, quota'd (docs/mods.md). Keys are `[a-z0-9_.-]{1,64}`, one path segment.

| Function | Signature | What it does |
| --- | --- | --- |
| `get` | `func(key: string) -> option<list<u8>>` | None if `key` has never been `set`, or the key is not well-formed. |
| `set` | `func(key: string, value: list<u8>) -> result<_, string>` | Errs on a malformed key or over quota (bytes for the whole mod). |
| `delete` | `func(key: string)` | No error if `key` was already absent. |

## `command`

Access: capability `command`.

Capability `command`: vanilla commands run as the player, with the player's permissions, after the hook call returns (8 per call). A mod's own commands are not run this way.

| Function | Signature | What it does |
| --- | --- | --- |
| `run` | `func(line: string)` | `line` without its slash; its feedback goes to the chat as if typed. |
| `run-quiet` | `func(lines: list<string>)` | `lines` (each without its slash, at most 256 a call) as `run` does, but with nothing in the chat: no feedback and no failure. In single player and on Horizon's servers, each runs exactly as the player typing it would (the same rights), in order, 64 a tick, and the server logs one line a batch; one that carries a chat message (`say`, `msg`) is refused. On any other server they go as chat commands, and there their feedback shows: a mod running many should say so. |

## `ui`

Access: capability `ui`.

Capability `ui`: widgets the game draws with its own menus' widgets, for a screen the mod owns and a settings page in Options > Mods. Layout and drawing are the game's: a mod says what its widgets hold, never where they sit or how they look, and the game clips everything it is handed.

### record `text-widget`

A line of text.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget; `ui-events` calls echo it. |
| `text` | `string` | Up to 256 characters. |
| `color` | `u32` | 0xRRGGBB; 0 is the menus' white. |

### record `item-widget`

An item's icon and a name beside it, as recipe viewers show them.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget. |
| `item` | `string` | `minecraft:diamond`, `<mod>:<path>`; an unknown id draws a "?". |
| `name` | `string` | The name shown beside the icon (empty: the item's own). |

### record `list-widget`

Rows the player picks one of, as an options page lists choices: a pick is `ui-events::select`, `selected` is the row to mark now.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget; `ui-events::select` echoes it. |
| `rows` | `list<string>` | The rows' texts, at most 512, each up to 256 characters. |
| `selected` | `option<u32>` | The row to mark as the current choice, if any. |

### record `button-widget`

A thing on a mod's screen or settings page. A button as the options pages have them (200 × 20): a click is `ui-events::select(id, 0)`. Mods cycle its label for a choice ("Corner: Top Left").

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget. |
| `label` | `string` | Up to 256 characters. |

### variant `widget`

| Case | Payload | |
| --- | --- | --- |
| `text` | `text-widget` | Text the player reads. |
| `button` | `button-widget` | A button. |
| `item` | `item-widget` | An item's icon and name. |
| `list` | `list-widget` | Rows the player picks from. |

### record `screen`

A screen the mod owns: a title over widgets stacked down the middle, the Done button the game adds, and Esc to leave it.

| Field | Type | |
| --- | --- | --- |
| `title` | `string` | Its title, up to 256 characters. |
| `widgets` | `list<widget>` | Its widgets, in order, at most 64. |

| Function | Signature | What it does |
| --- | --- | --- |
| `open-screen` | `func(screen: screen)` | Show `screen` over the game; a second call while it is open replaces it. Only the last call of one hook counts. |
| `close-screen` | `func()` | Close the mod's screen when it is open (the mod hears no `closed` for this: it was its own doing). |
| `set-settings` | `func(widgets: list<widget>)` | The mod's page in Options > Mods, reached from a settings button on its row: the widgets it shows, an empty list for no page. The player opening the page is `ui-events::settings-opened`: push fresh widgets then; later calls update the page while it shows. |

## `ui-events`

Access: exported by the mod: the game calls it.

What the game calls on a mod that shows widgets (capability `ui`): what the player did with them.

| Function | Signature | What it does |
| --- | --- | --- |
| `select` | `func(id: u32, row: u32)` | A row of one of the mod's `list` widgets was picked: the widget's `id` and the row, on the screen or the settings page that showed it. |
| `closed` | `func()` | The player left the mod's screen (Esc or Done); not for a screen the mod closed itself or one that went away with the mod. |
| `settings-opened` | `func()` | The player opened the mod's settings page in Options > Mods: call `ui::set-settings` now to fill it with fresh widgets. |

## `input`

Access: capability `input`.

Capability `input` (experimental: the manifest needs `experimental = ["input"]`): the use button on items the mod claims, and the mod's own keys. Events wait for `poll`, at most 256.

### record `held-item`

The main hand's stack.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:spyglass`, `<mod>:<path>`. |
| `count` | `u32` |  |
| `slot` | `u32` | The hotbar slot, 0 to 8. |

### record `item-use`

| Field | Type | |
| --- | --- | --- |
| `item` | `held-item` |  |
| `ticks` | `u32` | Game ticks the button was held (0 on `use-start`). |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `use-start` | `item-use` | The use button went down with a claimed item in the main hand. The game does nothing else with the click: no placing, no eating, no swing. |
| `use-stop` | `item-use` | It came up again, or the item left the hand, or a screen opened. |
| `key-down` | `string` | A key `register-key` named went down or up while playing (no screen open). The game's own mapping on that key still runs. A screen opening lets every held key up. |
| `key-up` | `string` |  |
| `wheel` | `f32` | Mouse wheel steps while the mod holds the wheel (`claim-wheel`): positive away from the player, as the hotbar reads them. |

### record `mapping-state`

A mapping as the player has bound it.

| Field | Type | |
| --- | --- | --- |
| `label` | `string` | The bound key's name as Controls shows it: `W`, `Space`, `LMB`. |
| `down` | `bool` | Held down now (as of this frame for a mod with a `frame` hook, else this tick). |

| Function | Signature | What it does |
| --- | --- | --- |
| `claim-items` | `func(ids: list<string>)` | The items whose use the mod handles instead of the game, by id (at most 64). Replaces the previous list. |
| `register-key` | `func(name: string, label: string, key: string) -> result<_, string>` | A key of the mod's: `name` comes back in `key-down` and `key-up`, `label` says what it does, `key` is the key as options.txt writes it (`key.keyboard.r`), the default: the player can bind another one on the mod's page in Options > Mods, and that one wins. At most 16; an unknown key is an error. |
| `held` | `func() -> option<held-item>` | The main hand's stack, as of this tick; none when it is empty. |
| `poll` | `func() -> list<event>` | The events since the last call, oldest first. |
| `claim-wheel` | `func(on: bool)` | Takes the mouse wheel while playing (true) or gives it back: while taken, its steps are the mod's `wheel` events and the hotbar doesn't move. The game gives it back when the mod stops. |
| `mapping` | `func(name: string) -> option<mapping-state>` | A movement or click mapping as the player has bound it: `key.forward`, `key.left`, `key.back`, `key.right`, `key.jump`, `key.sneak`, `key.sprint`, `key.attack` or `key.use`. None for another name. |
| `clicks-per-second` | `func() -> tuple<u32, u32>` | Clicks of the attack and use buttons in the last second. |

## `render`

Access: capability `render`.

Capability `render` (experimental: the manifest needs `experimental = ["render"]`): shapes the game draws for the mod with its own renderer, kept until the mod replaces them (no call per frame). A mod never supplies shader code.

### enum `corner`

A corner of the screen, as `hud`'s panels use them.

| Case | |
| --- | --- |
| `top-left` |  |
| `top-right` |  |
| `bottom-left` |  |
| `bottom-right` |  |

### record `rect`

A rectangle in GUI pixels: the top left corner and the size.

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |
| `w` | `f32` |  |
| `h` | `f32` |  |

### record `point`

A point in GUI pixels from the screen's top left.

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |

### record `fill-shape`

| Field | Type | |
| --- | --- | --- |
| `rect` | `rect` |  |
| `color` | `u32` | 0xAARRGGBB. |

### record `quad-shape`

Any four-cornered shape (a wheel's segment, an arrow): the corners in order round it.

| Field | Type | |
| --- | --- | --- |
| `corners` | `tuple<point, point, point, point>` |  |
| `color` | `u32` | 0xAARRGGBB. |

### record `sprite-shape`

| Field | Type | |
| --- | --- | --- |
| `sprite` | `string` | One of the game's GUI sprites, as `gui/sprites/` names them (`hud/heart/full`, `widget/button`), the resource packs' versions included. An unknown one draws nothing. |
| `rect` | `rect` |  |
| `color` | `u32` | A tint, 0xAARRGGBB: 0xFFFFFFFF draws it as it is. |

### record `item-shape`

| Field | Type | |
| --- | --- | --- |
| `item` | `string` | `minecraft:diamond`, `<mod>:<path>`; an unknown one draws nothing. |
| `at` | `point` | The icon's top left. |
| `scale` | `f32` | 1.0 is the hotbar's 16 × 16. |

### record `text-shape`

| Field | Type | |
| --- | --- | --- |
| `text` | `string` | Up to 256 characters, one line. |
| `at` | `point` |  |
| `color` | `u32` | 0xRRGGBB, as `hud`'s lines. |
| `centered` | `bool` | Centred on `at.x` instead of starting there. |
| `shadow` | `bool` |  |

### variant `shape`

| Case | Payload | |
| --- | --- | --- |
| `fill` | `fill-shape` |  |
| `quad` | `quad-shape` |  |
| `sprite` | `sprite-shape` |  |
| `item` | `item-shape` |  |
| `text` | `text-shape` |  |

### record `frame-rate`

| Field | Type | |
| --- | --- | --- |
| `fps` | `u32` | Frames drawn in the last second. |
| `min` | `u32` | The slowest frame's rate, and the 1% low (the 99th percentile frame time's rate), over the last 5 seconds. |
| `low` | `u32` |  |

### record `zoom`

| Field | Type | |
| --- | --- | --- |
| `factor` | `f32` | The field of view is divided by it: 1 is none, at most 10. |
| `smooth` | `bool` | Ease in and out through the game's field-of-view easing, as the spyglass does, instead of at once. |
| `turn` | `f32` | Mouse turning is multiplied by it while zoomed: 1 keeps the player's sensitivity, at least 0.05. |

### record `minimap`

| Field | Type | |
| --- | --- | --- |
| `corner` | `corner` |  |
| `size` | `f32` | Its side in GUI pixels, 16 to 256. |
| `blocks-per-pixel` | `f32` | World blocks per map pixel, 0.5 to 4; the map is 128 × 128 pixels. |
| `rotate` | `bool` | The map turns with the player, who faces up, and an N on its rim marks north. Otherwise north is up and the player's arrow turns. |
| `round` | `bool` | Round, the corners left clear; otherwise square. |

### record `far-terrain`

| Field | Type | |
| --- | --- | --- |
| `distance` | `u32` | How far the distant terrain reaches, in chunks: 64 to 1024. On integrated graphics other than Apple's the game caps it lower. |
| `reduced` | `bool` | Coarser far tiles, as the Far Detail setting's Reduced: 2 to 4 ms less GPU time per frame on integrated graphics. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-hud` | `func(shapes: list<shape>)` | Replaces the mod's HUD shapes: up to 1024, drawn every frame in order over the HUD (after the hotbar, under chat and screens), hidden with the HUD (F1). An empty list removes them. |
| `gui-size` | `func() -> tuple<f32, f32>` | The screen's size in GUI pixels (width, height), as of this tick. |
| `shake` | `func(strength: f32, seconds: f32)` | Shakes the camera: a jolt of `strength` (0 to 1, 1 the hardest) that fades out over `seconds` (at most 10). Jolts add up to 1. The view turns, the player doesn't, and the player's Distortion Effects option scales it (0 turns it off). |
| `set-corner` | `func(corner: corner, width: f32, height: f32, shapes: list<shape>)` | Replaces the mod's corner panel: shapes in GUI pixels from the top left of a `width` × `height` box that the game places in `corner`, stacked with the game's overlays and the other mods' panels (under the status effects at the top right), as `hud`'s panels are. |
| `clear-corner` | `func()` | Removes the mod's corner panel. |
| `text-width` | `func(text: string) -> f32` | The width of one line of text in GUI pixels, in the HUD's font: exact for Latin letters (U+0000 to U+024F), 6 per other character. |
| `frame-stats` | `func() -> frame-rate` | The game's frame counter, recomputed twice a second. |
| `set-zoom` | `func(zoom: zoom)` | Zooms the camera until the next call (`factor` 1 ends it), only while playing: a screen opening or the spyglass's own view (which wins) leave the camera vanilla's. With several mods, the strongest zoom counts. |
| `set-minimap` | `func(minimap: option<minimap>)` | Shows a map of the world around the player in `corner`, stacked with the game's overlays and the mods' panels, until the next call (none hides it). It looks like a map item: vanilla's map colours with their height shading and water depth, and the player's arrow. Underground, under a roof and in the Nether it maps the floor around the player's height. The game keeps the map's tiles and refreshes them within its own frame budget; hidden, they are dropped. One map shows at a time: the first loaded mod's. |
| `set-far-terrain` | `func(far-terrain: option<far-terrain>)` | Draws the world beyond the render distance as distant terrain, out to `distance` chunks, until the next call (none ends it). The game builds it on its own workers, from the chunks already loaded or saved and, where nobody has been, from the world's generator; the simulation distance doesn't change. Only worlds whose generator the game has (single player and a LAN world you host) show it. The player's own LOD Distance setting, when on, wins. One far terrain shows at a time: the first loaded mod's. |

## `events`

Access: exported by the mod: the game calls it.

What the game calls. Only the hooks `init` asks for are ever called.

### record `registration`

| Field | Type | |
| --- | --- | --- |
| `commands` | `list<string>` | Chat commands, without the slash. A name vanilla already uses is refused (vanilla's command runs). |
| `tick` | `bool` | Call `tick` every game tick (20 per second while playing). |
| `frame` | `bool` | Call `frame` every rendered frame. Budget: 0.1 ms. |
| `chat` | `bool` | Call `chat` for each chat line the player receives. |

| Function | Signature | What it does |
| --- | --- | --- |
| `init` | `func() -> result<registration, string>` | Once, after loading. An error keeps the mod off and is shown. |
| `tick` | `func(tick: u64)` | Each game tick while in a world, with the game's tick counter. |
| `frame` | `func(seconds: f32)` | Each rendered frame, with the seconds since the last one. |
| `command` | `func(name: string, args: string) -> result<option<string>, string>` | `/name args`: ok(some(text)) is shown as feedback, err(text) in red. |
| `chat` | `func(text: string)` | Each chat line the player receives, as plain text. |

## `recipes`

Access: capability `recipes`.

Capability `recipes` (experimental): the recipes and items the game knows, for recipe viewers. In single player and on Horizon's servers they are the world's; on any other server they are vanilla 26.3's (a client is sent no others), while `known` follows that server's book.

### record `stack`

An item of `items`, by its index there, with a count and the components it has beyond the item's own, in vanilla's item syntax (`[potion_contents={potion:"minecraft:swiftness"}]`), or "".

| Field | Type | |
| --- | --- | --- |
| `item` | `u32` |  |
| `count` | `u32` |  |
| `components` | `string` |  |

### enum `kind`

The station a recipe is made at.

| Case | |
| --- | --- |
| `crafting-shaped` |  |
| `crafting-shapeless` |  |
| `smelting` |  |
| `blasting` |  |
| `smoking` |  |
| `campfire` |  |
| `stonecutting` |  |
| `smithing` |  |
| `brewing` |  |

### record `recipe`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:oak_planks`. |
| `kind` | `kind` |  |
| `category` | `string` | The recipe book's tab (`crafting_building_blocks`, `furnace_food`), or "". |
| `group` | `string` | Recipes the book shows on one button share a group, or "". |
| `width` | `u32` | A shaped recipe's grid: `slots` holds its cells row by row. 0 for the other kinds. |
| `height` | `u32` |  |
| `slots` | `list<list<stack>>` | Each slot's stacks, any one of which fits; an empty list is an empty cell. Cooking and stonecutting: the input. Smithing: the template, the base, the addition. Brewing: the bottle, the reagent. |
| `results` | `list<stack>` | What it makes; several when that varies (a trim on any armour). |
| `cooking-time` | `u32` | Cooking: game ticks and experience; 0 for the other kinds. |
| `experience` | `f32` |  |
| `book-id` | `s32` | The recipe book's id for it (one per way the book shows it), as `known` lists them; -1 for the kinds the book doesn't show (campfire, stonecutting, smithing, brewing). |

### record `item`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:diamond`, `<mod>:<path>`. |
| `name` | `string` | In the player's language. |
| `tags` | `list<string>` | Its item tags, without the `#`. |

| Function | Signature | What it does |
| --- | --- | --- |
| `items` | `func() -> list<item>` | Every item, in the registry's order: `stack.item` indexes this list. |
| `all` | `func() -> list<recipe>` | Every recipe: the book's in its order, then campfire, stonecutting, smithing and brewing. They don't change while a world is open. |
| `known` | `func() -> list<s32>` | The `book-id`s of the recipes the player's recipe book holds now. |
| `place` | `func(book-id: s32, all: bool) -> result<_, string>` | Places a recipe in the open screen's grid as a click in the recipe book does (`ServerboundPlaceRecipePacket`): the inventory's grid or a crafting table. `all` places as many as the items allow, as with Shift. Refused while no such screen is open, for a recipe the player's book doesn't hold and for one that can't be placed; one missing items shows as a ghost in the grid, as in the book. |
| `give` | `func(stack: stack) -> result<_, string>` | Creative mode only: gives the player the stack, as taking it from the creative inventory does (`ServerboundSetCreativeModeSlot`): the selected slot first, then the hotbar, then the inventory. The count is cut to one stack of the item (64, 16 or 1). |

## `browser`

Access: not granted by any capability.

Capability `ui` (experimental): a recipe viewer's panel beside the game's inventory and container screens (not the creative inventory), on the right of the screen. The game draws it with its own widgets and slots, pages it and keeps its search box; the mod says what it holds. The first loaded mod with a panel shows.

### record `stack`

An item to show: its id, a count (1 draws none) and its components beyond the item's own, in vanilla's item syntax, or "".

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |
| `components` | `string` |  |

### record `click`

A click on an item of the panel, by its index in `set-items`'s list.

| Field | Type | |
| --- | --- | --- |
| `index` | `u32` |  |
| `right` | `bool` |  |

### record `item-key`

A key the mod registered (`input.register-key`) went down while the pointer was on an item: a slot of the screen's, or the panel's.

| Field | Type | |
| --- | --- | --- |
| `key` | `string` |  |
| `stack` | `stack` |  |

### record `slot-click`

A click on a slot of a recipe the panel shows (`set-recipes`).

| Field | Type | |
| --- | --- | --- |
| `recipe` | `u32` | The recipe's index in `set-recipes`'s list and the slot's in it; the results come after the slots. |
| `slot` | `u32` |  |
| `stack` | `stack` | The stack the slot showed at that moment. |
| `right` | `bool` |  |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `clicked` | `click` |  |
| `search` | `string` | The search box's text, each time the player changes it. |
| `key` | `item-key` |  |
| `slot-clicked` | `slot-click` |  |
| `action` | `u32` | The action button of the recipe at this index in `set-recipes`'s list. |
| `page` | `u32` | The player turned the recipes' page (`set-recipes`): the index of the first recipe now shown. |
| `back` |  | The back button of `set-recipes`. |
| `tab` | `u32` | The player picked the tab at this index of `set-recipes`' tabs. |

### enum `layout`

How the game lays out a recipe of `set-recipes`.

| Case | |
| --- | --- |
| `grid` | A crafting grid of `width` x `height` slots, an arrow and the result. |
| `cooking` | One input over a flame, an arrow and the result (furnaces, smokers, campfires). |
| `single` | One input, an arrow and the result (the stonecutter). |
| `smithing` | Template, base and addition, an arrow and the result. |
| `brewing` | A potion and an ingredient, an arrow and the result. |

### record `recipe-view`

A recipe to show. Each slot, and the result, cycles through its stacks, one a second; an empty slot shows nothing.

| Field | Type | |
| --- | --- | --- |
| `layout` | `layout` |  |
| `width` | `u32` | The grid's size (`grid` only), at most 3 x 3. |
| `height` | `u32` |  |
| `slots` | `list<list<stack>>` |  |
| `results` | `list<stack>` |  |
| `note` | `string` | A short line under the recipe (a cooking time), or "". |
| `action` | `option<bool>` | The action button beside it: none, enabled, or shown greyed. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-items` | `func(items: list<stack>)` | The panel's items, at most 8192 (an empty list hides the panel). |
| `set-recipes` | `func(title: string, tabs: list<stack>, tab: u32, recipes: list<recipe-view>)` | Shows these recipes in the panel instead of its items, as many as fit a page, under `title` and a back button, with a row of `tabs` (each an icon; `tab` is the selected one) between them; an empty list of recipes shows the items again. At most 256 recipes and 16 tabs. |
| `poll` | `func() -> list<event>` | What the player did on the panel since the last poll, at most 256 kept. |

## `players`

Access: capability `players`.

Capability `players`, server mods only (manifest `api = "0.3"`, experimental): the players on the server, and messages to them.

### enum `game-mode`

| Case | |
| --- | --- |
| `survival` |  |
| `creative` |  |
| `adventure` |  |
| `spectator` |  |

### record `player`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | The player's UUID, `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`. |
| `name` | `string` |  |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `dimension` | `string` | `minecraft:overworld`, `minecraft:the_nether`, ... |
| `game-mode` | `game-mode` |  |
| `health` | `f32` | Hit points as vanilla counts them: 20 is full health without effects. |

| Function | Signature | What it does |
| --- | --- | --- |
| `online` | `func() -> list<player>` | The players online, in the order they joined. |
| `tell` | `func(player: string, text: string) -> bool` | A system message to one player, after the mod's name; false when no player with that id is online. Up to 8 messages per call. |
| `broadcast` | `func(text: string)` | A system message to every player and the server's log, after the mod's name. Counts toward the same 8 per call. |

## `world`

Access: capability `world`.

Capability `world`, server mods only (manifest `api = "0.3"`, experimental): the blocks of the server's loaded chunks, and changes to them through the server's own block updates.

### record `block-change`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |
| `state` | `string` | `minecraft:oak_log[axis=y]`; properties left out take the block's defaults, `minecraft:stone` alone is the default state. |

| Function | Signature | What it does |
| --- | --- | --- |
| `get-block` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<string>` | The block state at a position in a dimension (`minecraft:overworld`, ...), as `minecraft:oak_log[axis=y]`, as of this call: none where the server has no chunk loaded there. |
| `get-biome` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<string>` | The biome at a position (`minecraft:plains`, ...), as the server stores it (one per 4 x 4 x 4 cell): none where no chunk is loaded. |
| `set-blocks` | `func(dimension: string, blocks: list<block-change>) -> result<_, string>` | Sets blocks as `/setblock` does: the old block entity removed, neighbours and shapes updated, light, and every player told. Every state is checked now (an error queues nothing); the changes are made in order after this call returns, before that dimension's next tick; one in a chunk not loaded then is skipped. At most 4096 per call. |

## `server-events`

Access: exported by the mod: the game calls it.

What the server calls in a server mod (manifest `api = "0.3"`, experimental). Only the hooks `init` asks for are ever called. The player in each event is as the server saw them when it happened.

### type `player`

`type player = player`

### record `block-pos`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |

### variant `argument-kind`

What an argument of a command takes. The server sends each command and its arguments to players' games, whose chat box completes and colours them as vanilla's; the server parses them before the mod's `command` hook gets their values.

| Case | Payload | |
| --- | --- | --- |
| `word` |  | One word: letters, digits and `_ - . +`. |
| `text` |  | The rest of the line, spaces included: only last. |
| `integer` | `tuple<option<s32>, option<s32>>` | A whole number, within the bounds given. |
| `number` |  | A number with or without decimals. |
| `boolean` |  | `true` or `false`. |
| `player` |  | One online player: a name, or `@s` or `@p` (whoever typed it, as vanilla picks for a player). |
| `block-pos` |  | A block position: three whole numbers, each one also `~` or `~n`, relative to whoever typed it. |

### record `argument`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | Shown in the chat box's hint (`<height>`) and the key of its value. |
| `kind` | `argument-kind` |  |
| `optional` | `bool` | Optional arguments come after every required one. |

### record `server-command`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | Without the slash. A name vanilla already uses is refused (vanilla's command runs). |
| `arguments` | `list<argument>` | In order; none: the command takes nothing after its name. Free text is one optional `text` argument. |

### variant `argument-value`

An argument's value, parsed as its kind says.

| Case | Payload | |
| --- | --- | --- |
| `word` | `string` |  |
| `text` | `string` |  |
| `integer` | `s32` |  |
| `number` | `f64` |  |
| `boolean` | `bool` |  |
| `player` | `player` |  |
| `block-pos` | `block-pos` |  |

### record `registration`

| Field | Type | |
| --- | --- | --- |
| `commands` | `list<server-command>` | Commands players may type. |
| `tick` | `bool` | Call `tick` every server tick (20 per second). |
| `joins` | `bool` | Call `player-joined` and `player-left`. |
| `chat` | `bool` | Call `chat` for each chat message a player sends. |
| `blocks` | `bool` | Call `block-broken` and `block-placed`. |

| Function | Signature | What it does |
| --- | --- | --- |
| `init` | `func() -> result<registration, string>` | Once, after loading. An error keeps the mod off and is logged. |
| `tick` | `func(tick: u64)` | Each server tick, with the server's tick counter. |
| `player-joined` | `func(player: player)` | A player joined the server; they are in `players.online` already. |
| `player-left` | `func(player: player)` | A player left the server; they are no longer in `players.online`. |
| `chat` | `func(player: player, text: string)` | A chat message a player sent, as plain text. |
| `command` | `func(player: player, name: string, args: string, values: list<tuple<string, argument-value>>) -> result<option<string>, string>` | `/name args` typed by `player`: ok(some(text)) is their feedback, err(text) their error in red. `values` holds each argument given, by name, in order (an optional one left out isn't there); a line that doesn't parse never reaches the mod: the player gets vanilla's error. |
| `block-broken` | `func(player: player, pos: block-pos, state: string)` | A player broke the block at `pos` in their dimension: `state` is what was there, as `minecraft:oak_log[axis=y]`. |
| `block-placed` | `func(player: player, pos: block-pos, state: string)` | A player placed `state` at `pos` in their dimension. |

## `plugin-types`

Access: not granted by any capability.

--------------------------------------------------------------------------- Plugins (manifest `side = "server"`, `api = "0.4"`, capability `plugin`): Bukkit-style server plugins (docs/plugins.md). A plugin runs where the world is simulated, as a server mod does, and reaches the whole server: events it may cancel and change at the moment they happen, commands with aliases, permissions and completion, a scheduler, permissions, players, worlds, entities, items, inventories it owns, scoreboards, boss bars, plugin messaging and a data folder. Text: every argument documented as text takes either a plain string with legacy colour codes (`§a`, or `&a` when the string holds no `§`), or a JSON text component when it starts with `{`, `[` or `"`. --------------------------------------------------------------------------- The records plugins and the server share.

### record `location`

A point in a world, looking somewhere.

| Field | Type | |
| --- | --- | --- |
| `world` | `string` | `minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`. |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` | Degrees, as F3 shows them. |
| `pitch` | `f32` |  |

### record `vec3`

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |

### record `block-pos`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |

### record `block`

A block where it is: its world, position and state (`minecraft:oak_log[axis=y]`).

| Field | Type | |
| --- | --- | --- |
| `world` | `string` |  |
| `pos` | `block-pos` |  |
| `state` | `string` |  |

### enum `game-mode`

| Case | |
| --- | --- |
| `survival` |  |
| `creative` |  |
| `adventure` |  |
| `spectator` |  |

### enum `hand`

| Case | |
| --- | --- |
| `main` |  |
| `off` |  |

### record `item-stack`

An item stack: the item (`minecraft:diamond_sword`), a count and the components it has beyond the item's own, as an SNBT compound (`{"minecraft:enchantments":{"minecraft:sharpness":5}}`), or "".

| Field | Type | |
| --- | --- | --- |
| `id` | `string` |  |
| `count` | `u32` |  |
| `components` | `string` |  |

### record `player`

A player as the server sees them when the call is made.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | The UUID, `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`. |
| `name` | `string` |  |
| `display-name` | `string` | The name chat shows (`plugin-players.set-display-name`), as text. |
| `entity-id` | `s32` | The id of the player's entity, as `plugin-entities` names it. |
| `location` | `location` |  |
| `game-mode` | `game-mode` |  |
| `health` | `f32` |  |
| `max-health` | `f32` |  |
| `food` | `u32` |  |
| `saturation` | `f32` |  |
| `level` | `u32` | Experience level, progress to the next (0 to 1) and total points. |
| `exp` | `f32` |  |
| `total-exp` | `u32` |  |
| `op` | `bool` | An operator (permission level above 0), and the level (0 to 4). |
| `permission-level` | `u32` |  |
| `address` | `string` | `127.0.0.1:52344`. |
| `ping` | `u32` | Milliseconds, as the player list shows them. |
| `locale` | `string` | `en_us`. |
| `client-brand` | `string` |  |
| `view-distance` | `u32` |  |
| `flying` | `bool` |  |
| `allow-flight` | `bool` |  |
| `sneaking` | `bool` |  |
| `sprinting` | `bool` |  |
| `gliding` | `bool` |  |
| `sleeping` | `bool` |  |
| `on-ground` | `bool` |  |
| `dead` | `bool` |  |
| `selected-slot` | `u32` | The hotbar slot in hand, 0 to 8. |
| `held` | `option<item-stack>` | The stack in the main hand. |

### record `entity-ref`

An entity events name: its id, its type (`minecraft:zombie`) and, for a player, the player's UUID.

| Field | Type | |
| --- | --- | --- |
| `id` | `s32` |  |
| `kind` | `string` |  |
| `player` | `option<string>` |  |

### variant `sender`

Who ran a command: the server's console, or a player.

| Case | Payload | |
| --- | --- | --- |
| `console` |  |  |
| `player` | `player` |  |


## `plugin-event-types`

Access: not granted by any capability.

Events: what plugins hear, and how they register for it.

### type `location`

`type location = location`

### type `block`

`type block = block`

### type `block-pos`

`type block-pos = block-pos`

### type `item-stack`

`type item-stack = item-stack`

### type `player`

`type player = player`

### type `entity-ref`

`type entity-ref = entity-ref`

### type `game-mode`

`type game-mode = game-mode`

### type `hand`

`type hand = hand`

### enum `priority`

When a listener runs: lowest first, monitor last. A monitor listener sees the outcome and may change nothing.

| Case | |
| --- | --- |
| `lowest` |  |
| `low` |  |
| `normal` |  |
| `high` |  |
| `highest` |  |
| `monitor` |  |

### record `player-join-event`

A player joined. `message` is the join message every player reads (none: no message).

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `message` | `option<string>` |  |
| `first-join` | `bool` | The player has never been on this server before. |

### record `player-quit-event`

A player left. `message` is the quit message (none: no message).

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `message` | `option<string>` |  |
| `reason` | `string` |  |

### record `player-login-event`

A player is logging in: clear `allowed` to turn them away with `kick-message`.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `allowed` | `bool` |  |
| `kick-message` | `string` |  |

### record `player-chat-event`

A chat message. `format` places the sender's display name at `{name}` and the message at `{message}` (text); only the players in `recipients` (UUIDs) receive it. Unchanged, the message goes as vanilla sends it, signed when it was.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `message` | `string` |  |
| `format` | `string` |  |
| `recipients` | `list<string>` |  |
| `cancelled` | `bool` |  |

### record `player-command-event`

A command a player typed (`line` without its slash), before anything runs it. Change `line` to run another.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `line` | `string` |  |
| `cancelled` | `bool` |  |

### record `player-move-event`

A player moved during the last tick. Change `to` to put them elsewhere; cancelling puts them back at `from`.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `from` | `location` |  |
| `to` | `location` |  |
| `cancelled` | `bool` |  |

### enum `interact-action`

| Case | |
| --- | --- |
| `left-click-block` |  |
| `right-click-block` |  |
| `left-click-air` |  |
| `right-click-air` |  |
| `physical` | Treading on a pressure plate or farmland. |

### record `player-interact-event`

A player clicked a block or the air. Cancelling stops what the click would do: no use, no placing, no breaking start.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `action` | `interact-action` |  |
| `hand` | `hand` |  |
| `item` | `option<item-stack>` |  |
| `block` | `option<block>` |  |
| `face` | `string` | `up`, `down`, `north`, `south`, `west`, `east`; "" for the air. |
| `cancelled` | `bool` |  |

### record `player-interact-entity-event`

A player used an entity (right click).

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `entity` | `entity-ref` |  |
| `hand` | `hand` |  |
| `cancelled` | `bool` |  |

### record `player-toggle-sneak-event`

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `sneaking` | `bool` |  |
| `cancelled` | `bool` |  |

### record `player-toggle-sprint-event`

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `sprinting` | `bool` |  |
| `cancelled` | `bool` |  |

### record `player-toggle-flight-event`

A player started or stopped flying. Cancelling keeps them as they were.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `flying` | `bool` |  |
| `cancelled` | `bool` |  |

### record `player-item-held-event`

A player picked another hotbar slot.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `previous` | `u32` |  |
| `current` | `u32` |  |
| `cancelled` | `bool` |  |

### record `player-drop-item-event`

A player threw an item out (Q, or out of a screen).

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `item` | `item-stack` |  |
| `cancelled` | `bool` |  |

### record `player-swap-hands-event`

A player swapped the stacks of their hands (F).

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `main` | `option<item-stack>` |  |
| `off` | `option<item-stack>` |  |
| `cancelled` | `bool` |  |

### record `player-respawn-event`

A player respawns; change `location` to respawn elsewhere.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `location` | `location` |  |

### record `player-death-event`

A player died. `message` is the death message (text; none: no message); `keep-inventory` and `keep-level` keep what they say.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `message` | `option<string>` |  |
| `keep-inventory` | `bool` |  |
| `keep-level` | `bool` |  |
| `drops` | `list<item-stack>` |  |

### record `player-game-mode-change-event`

A player's game mode is about to change.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `mode` | `game-mode` |  |
| `cancelled` | `bool` |  |

### record `player-changed-world-event`

A player went to another world; `player` is already there.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `from` | `string` |  |

### record `sign-change-event`

A player finished editing a sign: change `lines` (text) to write others.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `block` | `block` |  |
| `lines` | `list<string>` |  |
| `front` | `bool` |  |
| `cancelled` | `bool` |  |

### record `plugin-message-event`

A custom payload the player's client sent on a channel the plugin registered (`plugin-messaging.register-incoming`).

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `channel` | `string` |  |
| `data` | `list<u8>` |  |

### record `player-resource-pack-event`

The player's client answered a resource pack request: `accepted`, `declined`, `downloaded`, `loaded`, `failed` ...

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `status` | `string` |  |

### record `block-break-event`

A player broke a block. `drop-items` and `exp` say what it drops.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `block` | `block` |  |
| `drop-items` | `bool` |  |
| `exp` | `u32` |  |
| `cancelled` | `bool` |  |

### record `block-place-event`

A player placed a block: `block` is the new one, `replaced` the state that was there.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `block` | `block` |  |
| `replaced` | `string` |  |
| `item` | `option<item-stack>` |  |
| `hand` | `hand` |  |
| `cancelled` | `bool` |  |

### record `block-piston-event`

A piston is about to push (`block-piston-extend`) or pull (`block-piston-retract`) blocks: `block` is the piston, `blocks` those it moves (not those it breaks; none when a piston retracts alone). Cancelling leaves the piston as it is.

| Field | Type | |
| --- | --- | --- |
| `block` | `block` |  |
| `direction` | `string` | Where it faces: `north`, `up` ... |
| `sticky` | `bool` |  |
| `blocks` | `list<block-pos>` |  |
| `cancelled` | `bool` |  |

### record `block-from-to-event`

A fluid is about to flow from `block` into `to`, towards `face` (`down`, `north` ...). Cancelling the flow down stops it flowing to the sides this tick too.

| Field | Type | |
| --- | --- | --- |
| `block` | `block` |  |
| `to` | `block` |  |
| `face` | `string` |  |
| `cancelled` | `bool` |  |

### record `block-grow-event`

A block changed by itself. `block-grow`: a plant grew in a random tick (crops, stems and their fruit, cactus, sugar cane, berry bushes, cocoa, nether wart). `block-form`: snow or ice formed in the weather; lava meeting water is about to turn into stone, cobblestone, obsidian or basalt. `block-fade`: ice melted, coral died, fire went out, grass or farmland turned to dirt; a snow layer is about to melt. `block` is the block as it was, `new-state` what it is now (the world already shows it, but for lava and snow, which wait for the plugins). Cancelling puts back every block the change set (lava and snow stay).

| Field | Type | |
| --- | --- | --- |
| `block` | `block` |  |
| `new-state` | `string` |  |
| `cancelled` | `bool` |  |

### record `block-spread-event`

A block spread from `source` into `block` (grass, mycelium, vines, kelp and the other growing plants, mushrooms, fire): `block` as it was, `new-state` what it is now. Cancelling puts it back.

| Field | Type | |
| --- | --- | --- |
| `block` | `block` |  |
| `source` | `block` |  |
| `new-state` | `string` |  |
| `cancelled` | `bool` |  |

### record `block-fertilize-event`

Bone meal is about to be used on `block` (or on the water beside it, for water plants), by `player` when a player uses it. Cancelling grows nothing and keeps the bone meal.

| Field | Type | |
| --- | --- | --- |
| `player` | `option<player>` |  |
| `block` | `block` |  |
| `cancelled` | `bool` |  |

### record `leaves-decay-event`

Leaves too far from a log are about to decay (drop and go). Cancelling keeps them.

| Field | Type | |
| --- | --- | --- |
| `block` | `block` |  |
| `cancelled` | `bool` |  |

### record `block-burn-event`

The fire at `source` is about to burn `block` away. Cancelling keeps the block (and doesn't prime TNT).

| Field | Type | |
| --- | --- | --- |
| `block` | `block` |  |
| `source` | `block` |  |
| `cancelled` | `bool` |  |

### record `entity-damage-event`

An entity is about to be hurt by `cause` (a damage type, `minecraft:fall`); `damage` is the amount before armour.

| Field | Type | |
| --- | --- | --- |
| `entity` | `entity-ref` |  |
| `cause` | `string` |  |
| `damage` | `f32` |  |
| `damager` | `option<entity-ref>` |  |
| `cancelled` | `bool` |  |

### record `entity-death-event`

An entity died (players too, after their own death event).

| Field | Type | |
| --- | --- | --- |
| `entity` | `entity-ref` |  |
| `location` | `location` |  |
| `drops` | `list<item-stack>` |  |
| `exp` | `u32` |  |

### record `entity-spawn-event`

An entity is about to come into a world: `reason` is `natural`, `spawner`, `command`, `breeding`, `egg`, `plugin`, `default` ...

| Field | Type | |
| --- | --- | --- |
| `entity` | `entity-ref` |  |
| `location` | `location` |  |
| `reason` | `string` |  |
| `cancelled` | `bool` |  |

### record `entity-explode-event`

Something explodes: `blocks` are those it would destroy (remove some to spare them).

| Field | Type | |
| --- | --- | --- |
| `entity` | `option<entity-ref>` |  |
| `location` | `location` |  |
| `blocks` | `list<block-pos>` |  |
| `cancelled` | `bool` |  |

### record `inventory-view`

The menu a player has open, as inventory events name it.

| Field | Type | |
| --- | --- | --- |
| `kind` | `string` | The menu type, `minecraft:generic_9x3`; `minecraft:inventory` for the player's own. |
| `plugin-inventory` | `option<u32>` | The plugin inventory (`plugin-inventories`) it shows, if any. |
| `size` | `u32` | Slots of the container itself, before the player's 36. |

### enum `click-mode`

| Case | |
| --- | --- |
| `pickup` |  |
| `quick-move` |  |
| `swap` |  |
| `clone` |  |
| `throw` |  |
| `quick-craft` |  |
| `pickup-all` |  |

### record `inventory-click-event`

A click in a menu, before the server applies it. Cancelling leaves every slot as it was (the player's client is corrected).

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `view` | `inventory-view` |  |
| `slot` | `s32` | The slot clicked in the menu's numbering (container first, then the player's inventory); -999 outside the window. |
| `button` | `u32` |  |
| `mode` | `click-mode` |  |
| `current` | `option<item-stack>` |  |
| `cursor` | `option<item-stack>` |  |
| `cancelled` | `bool` |  |

### record `inventory-close-event`

A player closed a menu.

| Field | Type | |
| --- | --- | --- |
| `player` | `player` |  |
| `view` | `inventory-view` |  |

### record `server-command-event`

A command typed in the server's console (`line` without a slash).

| Field | Type | |
| --- | --- | --- |
| `line` | `string` |  |
| `cancelled` | `bool` |  |

### record `plugin-state-event`

A plugin was enabled or disabled (its id).

| Field | Type | |
| --- | --- | --- |
| `plugin` | `string` |  |
| `enabled` | `bool` |  |

### record `plugin-call-event`

`plugin-server.send-to-plugin` from another plugin.

| Field | Type | |
| --- | --- | --- |
| `from` | `string` |  |
| `channel` | `string` |  |
| `data` | `list<u8>` |  |

### variant `event`

Every event. A listener receives each kind it registered for.

| Case | Payload | |
| --- | --- | --- |
| `player-join` | `player-join-event` |  |
| `player-quit` | `player-quit-event` |  |
| `player-login` | `player-login-event` |  |
| `player-chat` | `player-chat-event` |  |
| `player-command` | `player-command-event` |  |
| `player-move` | `player-move-event` |  |
| `player-interact` | `player-interact-event` |  |
| `player-interact-entity` | `player-interact-entity-event` |  |
| `player-toggle-sneak` | `player-toggle-sneak-event` |  |
| `player-toggle-sprint` | `player-toggle-sprint-event` |  |
| `player-toggle-flight` | `player-toggle-flight-event` |  |
| `player-item-held` | `player-item-held-event` |  |
| `player-drop-item` | `player-drop-item-event` |  |
| `player-swap-hands` | `player-swap-hands-event` |  |
| `player-respawn` | `player-respawn-event` |  |
| `player-death` | `player-death-event` |  |
| `player-game-mode-change` | `player-game-mode-change-event` |  |
| `player-changed-world` | `player-changed-world-event` |  |
| `sign-change` | `sign-change-event` |  |
| `plugin-message` | `plugin-message-event` |  |
| `player-resource-pack` | `player-resource-pack-event` |  |
| `block-break` | `block-break-event` |  |
| `block-place` | `block-place-event` |  |
| `block-piston-extend` | `block-piston-event` |  |
| `block-piston-retract` | `block-piston-event` |  |
| `block-from-to` | `block-from-to-event` |  |
| `block-grow` | `block-grow-event` |  |
| `block-spread` | `block-spread-event` |  |
| `block-form` | `block-grow-event` |  |
| `block-fade` | `block-grow-event` |  |
| `block-fertilize` | `block-fertilize-event` |  |
| `leaves-decay` | `leaves-decay-event` |  |
| `block-burn` | `block-burn-event` |  |
| `entity-damage` | `entity-damage-event` |  |
| `entity-death` | `entity-death-event` |  |
| `entity-spawn` | `entity-spawn-event` |  |
| `entity-explode` | `entity-explode-event` |  |
| `inventory-click` | `inventory-click-event` |  |
| `inventory-close` | `inventory-close-event` |  |
| `server-command` | `server-command-event` |  |
| `plugin-state` | `plugin-state-event` |  |
| `plugin-call` | `plugin-call-event` |  |

### enum `event-kind`

The kinds of `event`, for `plugin-listeners.listen`.

| Case | |
| --- | --- |
| `player-join` |  |
| `player-quit` |  |
| `player-login` |  |
| `player-chat` |  |
| `player-command` |  |
| `player-move` |  |
| `player-interact` |  |
| `player-interact-entity` |  |
| `player-toggle-sneak` |  |
| `player-toggle-sprint` |  |
| `player-toggle-flight` |  |
| `player-item-held` |  |
| `player-drop-item` |  |
| `player-swap-hands` |  |
| `player-respawn` |  |
| `player-death` |  |
| `player-game-mode-change` |  |
| `player-changed-world` |  |
| `sign-change` |  |
| `plugin-message` |  |
| `player-resource-pack` |  |
| `block-break` |  |
| `block-place` |  |
| `block-piston-extend` |  |
| `block-piston-retract` |  |
| `block-from-to` |  |
| `block-grow` |  |
| `block-spread` |  |
| `block-form` |  |
| `block-fade` |  |
| `block-fertilize` |  |
| `leaves-decay` |  |
| `block-burn` |  |
| `entity-damage` |  |
| `entity-death` |  |
| `entity-spawn` |  |
| `entity-explode` |  |
| `inventory-click` |  |
| `inventory-close` |  |
| `server-command` |  |
| `plugin-state` |  |
| `plugin-call` |  |


## `plugin-listeners`

Access: not granted by any capability.

Registering for events.

### type `event-kind`

`type event-kind = event-kind`

### type `priority`

`type priority = priority`

| Function | Signature | What it does |
| --- | --- | --- |
| `listen` | `func(kind: event-kind, priority: priority, ignore-cancelled: bool)` | Hear every event of `kind` at `priority` (once per kind and priority); with `ignore-cancelled`, not those already cancelled. |
| `unlisten` | `func(kind: event-kind)` | Stop hearing `kind` at every priority. |

## `plugin-commands`

Access: not granted by any capability.

Commands the plugin owns beyond those its manifest lists.

### record `command-spec`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` |  |
| `description` | `string` |  |
| `usage` | `string` | Shown when the command returns false; `<command>` is the label used. |
| `aliases` | `list<string>` |  |
| `permission` | `option<string>` | Needed to run it; none: anyone may. |
| `permission-message` | `option<string>` | Shown to a sender without the permission (text). |

| Function | Signature | What it does |
| --- | --- | --- |
| `register` | `func(spec: command-spec) -> result<_, string>` | Adds a command (vanilla's names and other plugins' are refused). |
| `unregister` | `func(name: string)` | Removes a command `register` added. |

## `plugin-server`

Access: capability `plugin`.

The server as a whole.

### record `server-info`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` |  |
| `version` | `string` | The game version, `26.3`, and the protocol number. |
| `protocol` | `u32` |  |
| `motd` | `string` |  |
| `max-players` | `u32` |  |
| `online-mode` | `bool` |  |
| `port` | `u32` |  |
| `view-distance` | `u32` |  |
| `simulation-distance` | `u32` |  |
| `hardcore` | `bool` |  |
| `dedicated` | `bool` | A dedicated server (not a world opened to LAN). |
| `tick` | `u64` | Server ticks since it started. |

### record `plugin-info`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` |  |
| `name` | `string` |  |
| `version` | `string` |  |
| `enabled` | `bool` |  |
| `authors` | `list<string>` |  |
| `description` | `string` |  |
| `depend` | `list<string>` |  |

### record `offline-player`

A player who has played here: online now, or in the user cache.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` |  |
| `name` | `string` |  |
| `online` | `bool` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `info` | `func() -> server-info` | The server's settings and version. |
| `tps` | `func() -> tuple<f64, f64, f64>` | Ticks per second over the last 1, 5 and 15 minutes. |
| `mspt` | `func() -> f64` | The mean time of a tick in milliseconds, over the last 100 ticks. |
| `now-millis` | `func() -> s64` | Milliseconds since the Unix epoch (the sandbox has no clock). |
| `random` | `func() -> u64` | 64 random bits from the server (the sandbox has no entropy). |
| `broadcast` | `func(message: string, permission: option<string>) -> u32` | A message (text) to every player, or with `permission` to those who have it, and to the console: how many players got it. |
| `console` | `func(message: string)` | A line in the console's log as it is, at info level. |
| `dispatch-command` | `func(line: string)` | Runs a command as the console (no slash) once this call returns. |
| `shutdown` | `func()` | Stops the server after this tick. |
| `set-motd` | `func(motd: string)` | The status list's message of the day (text) and player limit. |
| `set-max-players` | `func(max: u32)` | The player limit, from now on. |
| `plugins` | `func() -> list<plugin-info>` | The installed plugins, in load order. |
| `plugin-enabled` | `func(id: string) -> bool` | Whether plugin `id` runs. |
| `send-to-plugin` | `func(target: string, channel: string, data: list<u8>) -> bool` | Sends `data` to plugin `target`, which hears a `plugin-call` event once this call returns. False when no such plugin runs. |
| `find-player` | `func(name-or-id: string) -> option<offline-player>` | A player by name (any case) or UUID: online, or known to the user cache. |
| `whitelist-enabled` | `func() -> bool` | Whether only whitelisted players may join. |
| `set-whitelist-enabled` | `func(on: bool)` | Turns the whitelist on or off, as `/whitelist on\|off`. |
| `whitelist` | `func() -> list<string>` | The whitelisted players' names. |
| `set-whitelisted` | `func(name: string, on: bool)` | Adds a player to the whitelist or removes them. |
| `operators` | `func() -> list<string>` | The operators' names. |
| `banned-players` | `func() -> list<string>` | The banned players' names. |
| `ban` | `func(name: string, reason: option<string>)` | Bans a player, as `/ban` (an online one is kicked). |
| `pardon` | `func(name: string)` | Lifts a player's ban, as `/pardon`. |
| `ban-ip` | `func(address: string, reason: option<string>)` | Bans an address, as `/ban-ip`. |
| `pardon-ip` | `func(address: string)` | Lifts an address's ban, as `/pardon-ip`. |

## `plugin-players`

Access: not granted by any capability.

Players online.

### type `location`

`type location = location`

### type `vec3`

`type vec3 = vec3`

### type `game-mode`

`type game-mode = game-mode`

### type `item-stack`

`type item-stack = item-stack`

### type `block-pos`

`type block-pos = block-pos`

### type `player`

`type player = player`

### record `effect`

| Field | Type | |
| --- | --- | --- |
| `kind` | `string` | `minecraft:speed`. |
| `duration` | `s32` | Ticks; -1 forever. |
| `amplifier` | `u32` |  |
| `ambient` | `bool` |  |
| `particles` | `bool` |  |
| `icon` | `bool` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `online` | `func() -> list<player>` | Every player online, in join order. |
| `get` | `func(id: string) -> option<player>` | The player with this UUID, when online. |
| `by-name` | `func(name: string) -> option<player>` | Any case. |
| `send-message` | `func(id: string, message: string)` | A system message (text). |
| `send-action-bar` | `func(id: string, message: string)` | A message (text) over the hotbar. |
| `send-title` | `func(id: string, title: string, subtitle: string, fade-in: u32, stay: u32, fade-out: u32)` | Ticks for the fades and the stay. |
| `clear-title` | `func(id: string)` | Removes the title the player sees. |
| `set-tab-list` | `func(id: string, header: string, footer: string)` | The player list's header and footer (text) for this player. |
| `set-list-name` | `func(id: string, name: option<string>)` | The player's name in every player's list (text; none: the name). |
| `set-display-name` | `func(id: string, name: option<string>)` | The name chat uses for the player (text; none: the name). |
| `play-sound` | `func(id: string, sound: string, category: string, volume: f32, pitch: f32, at: option<location>)` | `minecraft:entity.experience_orb.pickup` in `category` (`master`, `players`, ...), at `at` or at the player. |
| `stop-sound` | `func(id: string, sound: option<string>)` | Stops `sound` (none: every sound) for the player. |
| `spawn-particle` | `func(id: string, particle: string, at: location, count: u32, offset: vec3, speed: f32)` | Particles only this player sees (`minecraft:flame`). |
| `send-block-change` | `func(id: string, pos: block-pos, state: string)` | Shows this player a block that isn't there (until the chunk changes). |
| `kick` | `func(id: string, reason: string) -> bool` | Disconnects the player with `reason` (text). |
| `transfer` | `func(id: string, host: string, port: u32)` | Sends the player to another server (`ClientboundTransferPacket`). |
| `send-resource-pack` | `func(id: string, url: string, hash: string, required: bool, prompt: option<string>)` | Asks the player's client to load a resource pack (`hash`: SHA-1 in hex, or ""). |
| `teleport` | `func(id: string, to: location) -> bool` | Moves the player, to another world too; false when they aren't online. |
| `set-game-mode` | `func(id: string, mode: game-mode)` | Changes the player's game mode. |
| `set-health` | `func(id: string, health: f32)` | Sets the player's health (0 kills them). |
| `set-max-health` | `func(id: string, health: f64)` | Sets the player's max health attribute. |
| `set-food` | `func(id: string, food: u32)` | Sets the hunger bar, 0 to 20. |
| `set-saturation` | `func(id: string, saturation: f32)` | Sets the food saturation. |
| `set-level` | `func(id: string, level: u32)` | Sets the experience level. |
| `set-exp` | `func(id: string, progress: f32)` | Sets the progress to the next level, 0 to 1. |
| `give-exp` | `func(id: string, points: s32)` | Adds (or with a negative value takes) experience points. |
| `set-allow-flight` | `func(id: string, allow: bool)` | Lets the player fly, as creative mode does, or stops it. |
| `set-flying` | `func(id: string, flying: bool)` | Starts or stops the player's flight (it needs `allow-flight`). |
| `set-walk-speed` | `func(id: string, speed: f32)` | 0.1 is vanilla's walking speed (the movement speed attribute). |
| `set-fly-speed` | `func(id: string, speed: f32)` | 0.05 is vanilla's. |
| `set-op` | `func(id: string, op: bool)` | Makes the player an operator (level 4) or takes it away. |
| `set-velocity` | `func(id: string, velocity: vec3)` | Pushes the player: blocks per tick. |
| `damage` | `func(id: string, amount: f32, cause: option<string>)` | Hurts the player by `amount`, of damage type `cause` (none: generic). |
| `kill` | `func(id: string)` | Kills the player, as `/kill`. |
| `set-fire-ticks` | `func(id: string, ticks: u32)` | Sets the player on fire for that many ticks (0 puts it out). |
| `effects` | `func(id: string) -> list<effect>` | The player's status effects. |
| `add-effect` | `func(id: string, effect: effect)` | Gives the player a status effect, as `/effect give`. |
| `remove-effect` | `func(id: string, kind: string)` | Removes one status effect. |
| `clear-effects` | `func(id: string)` | Removes every status effect. |
| `hide-player` | `func(viewer: string, target: string)` | Hides `target` from `viewer` (the list and the world) until shown. |
| `show-player` | `func(viewer: string, target: string)` | Shows `target` to `viewer` again. |
| `can-see` | `func(viewer: string, target: string) -> bool` | Whether `viewer` sees `target` (no plugin hid them). |
| `perform-command` | `func(id: string, line: string)` | Runs a command as the player, with their permissions (no slash). |
| `chat` | `func(id: string, message: string)` | Sends a chat message as the player. |
| `set-respawn-location` | `func(id: string, at: option<location>)` | Where the player respawns; none: the world spawn. |
| `inventory` | `func(id: string) -> list<option<item-stack>>` | Slots 0 to 35 (0 to 8 the hotbar), 36 to 39 the armour (feet, legs, chest, head), 40 the off hand. |
| `get-slot` | `func(id: string, slot: u32) -> option<item-stack>` | One slot of the player's inventory, numbered as `inventory` does. |
| `set-slot` | `func(id: string, slot: u32, item: option<item-stack>)` | Replaces one slot of the player's inventory. |
| `give` | `func(id: string, items: list<item-stack>) -> list<item-stack>` | Adds stacks as picking them up does; what doesn't fit comes back. |
| `remove-item` | `func(id: string, item: string, count: u32) -> u32` | Removes up to `count` of item `item` (any components); how many went. |
| `clear-inventory` | `func(id: string)` | Empties the inventory, the armour and the off hand. |
| `ender-chest` | `func(id: string) -> list<option<item-stack>>` | The player's ender chest, 27 slots. |
| `set-ender-slot` | `func(id: string, slot: u32, item: option<item-stack>)` | Replaces one slot of the player's ender chest. |
| `set-held-slot` | `func(id: string, slot: u32)` | Selects a hotbar slot, 0 to 8. |
| `cursor` | `func(id: string) -> option<item-stack>` | The stack on the pointer in an open menu. |
| `set-cursor` | `func(id: string, item: option<item-stack>)` | Replaces the stack on the pointer. |
| `close-inventory` | `func(id: string)` | Closes the open menu. |
| `update-inventory` | `func(id: string)` | Sends the whole inventory to the player's client again. |

## `plugin-worlds`

Access: not granted by any capability.

Worlds (dimensions) and their blocks.

### type `location`

`type location = location`

### type `block-pos`

`type block-pos = block-pos`

### type `vec3`

`type vec3 = vec3`

### type `item-stack`

`type item-stack = item-stack`

### enum `environment`

| Case | |
| --- | --- |
| `normal` |  |
| `nether` |  |
| `the-end` |  |

### record `world-info`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | `minecraft:overworld`. |
| `environment` | `environment` |  |
| `seed` | `s64` |  |
| `time` | `u64` | Time of day, 0 to 23999, and ticks since the world began. |
| `full-time` | `u64` |  |
| `storm` | `bool` |  |
| `thundering` | `bool` |  |
| `spawn` | `location` |  |
| `difficulty` | `string` | `peaceful`, `easy`, `normal`, `hard`. |
| `min-y` | `s32` |  |
| `max-y` | `s32` |  |
| `players` | `u32` |  |
| `loaded-chunks` | `u32` |  |

### record `block-change`

| Field | Type | |
| --- | --- | --- |
| `pos` | `block-pos` |  |
| `state` | `string` |  |

### record `world-border`

| Field | Type | |
| --- | --- | --- |
| `center-x` | `f64` |  |
| `center-z` | `f64` |  |
| `size` | `f64` |  |
| `damage-per-block` | `f64` |  |
| `warning-blocks` | `u32` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `worlds` | `func() -> list<world-info>` | Every world the server has. |
| `get` | `func(world: string) -> option<world-info>` | One world, by name. |
| `get-block` | `func(world: string, pos: block-pos) -> option<string>` | The state at a position (`minecraft:oak_log[axis=y]`); none where no chunk is loaded. |
| `get-biome` | `func(world: string, pos: block-pos) -> option<string>` | The biome at a position (`minecraft:plains`); none where no chunk is loaded. |
| `highest-block-y` | `func(world: string, x: s32, z: s32) -> option<s32>` | The y of the highest block that isn't air at x z, in a loaded chunk. |
| `get-block-data` | `func(world: string, pos: block-pos) -> option<string>` | The block entity there (a chest's items, a sign's text), as SNBT. |
| `merge-block-data` | `func(world: string, pos: block-pos, snbt: string)` | Merges SNBT into the block entity there, as `/data merge block`. |
| `set-blocks` | `func(world: string, changes: list<block-change>) -> result<_, string>` | Sets blocks as `/setblock` does (neighbours, light, every client), once this call returns. Every state is checked now. At most 32768. |
| `is-chunk-loaded` | `func(world: string, x: s32, z: s32) -> bool` | Whether chunk x z (block coordinates / 16) is loaded. |
| `loaded-chunks` | `func(world: string) -> list<tuple<s32, s32>>` | The chunks loaded in a world. |
| `set-chunk-forced` | `func(world: string, x: s32, z: s32, forced: bool)` | Keeps a chunk loaded (and ticking) until released, as `/forceload`. |
| `set-time` | `func(world: string, time: u64)` | Sets the time of day (0 to 23999 is the first day). |
| `set-storm` | `func(world: string, storm: bool, duration: u32)` | `duration` ticks; 0 picks one as vanilla does. |
| `set-thundering` | `func(world: string, thundering: bool)` | Starts or stops a thunderstorm. |
| `set-spawn` | `func(at: location)` | Moves the world spawn. |
| `set-difficulty` | `func(difficulty: string)` | `peaceful`, `easy`, `normal` or `hard`. |
| `game-rule` | `func(rule: string) -> option<string>` | A game rule's value (`keep_inventory` -> `false`). |
| `set-game-rule` | `func(rule: string, value: string)` | Sets a game rule, as `/gamerule`. |
| `strike-lightning` | `func(at: location, effect-only: bool)` | A lightning bolt; with `effect-only` it sets nothing on fire and hurts no one. |
| `explode` | `func(at: location, power: f32, fire: bool, break-blocks: bool)` | An explosion of `power` (TNT is 4). |
| `drop-item` | `func(at: location, item: item-stack, naturally: bool)` | Drops an item entity; `naturally` scatters it as a broken block does. |
| `play-sound` | `func(at: location, sound: string, category: string, volume: f32, pitch: f32)` | A sound every player near `at` hears. |
| `spawn-particle` | `func(at: location, particle: string, count: u32, offset: vec3, speed: f32, force: bool)` | Particles players near `at` see (`force`: up to 512 blocks away). |
| `border` | `func(world: string) -> world-border` | The world border. |
| `set-border` | `func(world: string, center-x: f64, center-z: f64, size: f64, seconds: u32)` | Moves the border to `size` over `seconds` (0: at once). |
| `save` | `func()` | Saves every world. |

## `plugin-entities`

Access: not granted by any capability.

Entities.

### type `location`

`type location = location`

### type `vec3`

`type vec3 = vec3`

### type `item-stack`

`type item-stack = item-stack`

### record `entity`

| Field | Type | |
| --- | --- | --- |
| `id` | `s32` |  |
| `uuid` | `string` |  |
| `kind` | `string` | `minecraft:zombie`. |
| `location` | `location` |  |
| `velocity` | `vec3` |  |
| `custom-name` | `option<string>` | Text, as the name tag shows it. |
| `health` | `option<f32>` | Living entities only. |
| `max-health` | `option<f32>` |  |
| `dead` | `bool` |  |
| `on-ground` | `bool` |  |
| `tags` | `list<string>` | Scoreboard tags (`/tag`). |
| `player` | `option<string>` | A player's UUID. |

| Function | Signature | What it does |
| --- | --- | --- |
| `get` | `func(id: s32) -> option<entity>` | An entity by id, when it is in a loaded chunk. |
| `by-uuid` | `func(uuid: string) -> option<entity>` | An entity by UUID. |
| `in-world` | `func(world: string) -> list<entity>` | Every entity in a world. |
| `nearby` | `func(at: location, radius: f64) -> list<entity>` | Entities within `radius` blocks of `at`. |
| `by-tag` | `func(tag: string) -> list<entity>` | Entities with a scoreboard tag. |
| `spawn` | `func(at: location, kind: string, nbt: string) -> string` | Summons `kind` with SNBT `nbt` ("" for none) once this call returns; the entity carries the tag returned, for `by-tag`. |
| `teleport` | `func(id: s32, to: location) -> bool` | Moves an entity; false when there is none with that id. |
| `remove` | `func(id: s32)` | Removes an entity from the world (players are kicked out of it, not removed). |
| `set-velocity` | `func(id: s32, velocity: vec3)` | Pushes an entity: blocks per tick. |
| `set-custom-name` | `func(id: s32, name: option<string>, visible: bool)` | Names an entity (text; none: no name), shown over it when `visible`. |
| `set-glowing` | `func(id: s32, glowing: bool)` | The glowing outline. |
| `set-invulnerable` | `func(id: s32, invulnerable: bool)` | Whether nothing but the void and `/kill` hurts it. |
| `set-silent` | `func(id: s32, silent: bool)` | Whether it makes sounds. |
| `set-gravity` | `func(id: s32, gravity: bool)` | Whether it falls. |
| `set-ai` | `func(id: s32, ai: bool)` | Mobs: whether their AI runs (`NoAI`). |
| `damage` | `func(id: s32, amount: f32, cause: option<string>)` | Hurts an entity by `amount`, of damage type `cause` (none: generic). |
| `set-health` | `func(id: s32, health: f32)` | A living entity's health. |
| `add-tag` | `func(id: s32, tag: string)` | Adds a scoreboard tag. |
| `remove-tag` | `func(id: s32, tag: string)` | Removes a scoreboard tag. |
| `add-passenger` | `func(vehicle: s32, passenger: s32)` | Makes `passenger` ride `vehicle`. |
| `eject` | `func(id: s32)` | Gets every passenger off. |
| `get-nbt` | `func(id: s32) -> option<string>` | The entity's NBT as `/data get entity` writes it. |
| `merge-nbt` | `func(id: s32, snbt: string)` | Merges SNBT into the entity, as `/data merge entity`. |

## `plugin-items`

Access: not granted by any capability.

Items.

### type `item-stack`

`type item-stack = item-stack`

### record `enchantment`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:sharpness`. |
| `level` | `u32` |  |

### record `item-meta`

An item stack's common components, read or written in one piece.

| Field | Type | |
| --- | --- | --- |
| `display-name` | `option<string>` | `minecraft:custom_name` (text, italic unless it says otherwise). |
| `item-name` | `option<string>` | `minecraft:item_name` (text, not italic). |
| `lore` | `list<string>` | `minecraft:lore`, a line each (text). |
| `enchantments` | `list<enchantment>` |  |
| `damage` | `u32` |  |
| `max-damage` | `option<u32>` |  |
| `unbreakable` | `bool` |  |
| `custom-model-data` | `option<f32>` | `minecraft:custom_model_data`'s first float. |
| `max-stack-size` | `option<u32>` |  |
| `glint` | `option<bool>` | `minecraft:enchantment_glint_override`. |
| `hide-tooltip` | `bool` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `parse` | `func(text: string, count: u32) -> result<item-stack, string>` | An item as `/give` reads it: `diamond_sword[enchantments={sharpness:5}]`. |
| `meta` | `func(stack: item-stack) -> item-meta` | The stack's common components. |
| `with-meta` | `func(stack: item-stack, meta: item-meta) -> item-stack` | The stack with these components (the others kept). |
| `get-tag` | `func(stack: item-stack, key: string) -> option<string>` | A plugin's own value on a stack (`minecraft:custom_data`, under the plugin's id): Bukkit's persistent data. |
| `set-tag` | `func(stack: item-stack, key: string, value: option<string>) -> item-stack` | The stack with the plugin's value under `key` set (none: removed). |
| `similar` | `func(a: item-stack, b: item-stack) -> bool` | The same item with the same components (counts aside). |
| `max-stack` | `func(id: string) -> u32` | How many of an item a stack holds (64, 16 or 1). |
| `display-name` | `func(stack: item-stack) -> string` | The name the stack shows (plain text, English). |
| `registry` | `func(name: string) -> list<string>` | The ids of a registry: `item`, `block`, `entity_type`, `mob_effect`, `enchantment`, `biome`, `sound_event`, `particle_type`. |

## `plugin-inventories`

Access: not granted by any capability.

Inventories a plugin owns: chest-like menus it fills and opens.

### type `item-stack`

`type item-stack = item-stack`

### enum `inventory-kind`

| Case | |
| --- | --- |
| `chest` | 9 × rows (1 to 6): `minecraft:generic_9xN`. |
| `hopper` | 5 slots. |
| `dropper` | 3 × 3. |

| Function | Signature | What it does |
| --- | --- | --- |
| `create` | `func(kind: inventory-kind, rows: u32, title: string) -> u32` | A new inventory: `rows` counts for a chest only. Its id is the server's (every plugin's inventories share the numbering). |
| `size` | `func(inventory: u32) -> u32` | How many slots it has. |
| `get` | `func(inventory: u32, slot: u32) -> option<item-stack>` | One slot. |
| `set` | `func(inventory: u32, slot: u32, item: option<item-stack>)` | Replaces one slot; viewers see it at once. |
| `contents` | `func(inventory: u32) -> list<option<item-stack>>` | Every slot, in order. |
| `set-contents` | `func(inventory: u32, items: list<option<item-stack>>)` | Replaces every slot (missing ones become empty). |
| `add` | `func(inventory: u32, items: list<item-stack>) -> list<item-stack>` | Adds stacks as a chest takes them; what doesn't fit comes back. |
| `clear` | `func(inventory: u32)` | Empties every slot. |
| `set-title` | `func(inventory: u32, title: string)` | The title for the next time it opens (text). |
| `open` | `func(player: string, inventory: u32) -> bool` | Opens it for a player (closing what they had open). |
| `viewers` | `func(inventory: u32) -> list<string>` | The players looking at it. |
| `open-inventory` | `func(player: string) -> option<u32>` | The plugin inventory a player has open. |
| `destroy` | `func(inventory: u32)` | Closes it for its viewers and forgets it. |

## `plugin-scheduler`

Access: not granted by any capability.

The task scheduler: `plugin.on-task` runs at the ticks asked for.

| Function | Signature | What it does |
| --- | --- | --- |
| `run-later` | `func(delay: u64, period: option<u64>) -> u32` | Runs task `on-task(id)` after `delay` ticks (0: at the end of this tick), then every `period` ticks when given (at least 1). |
| `cancel` | `func(task: u32) -> bool` | Cancels a task: false when it was not queued. |
| `cancel-all` | `func()` | Cancels every task of the plugin. |
| `is-queued` | `func(task: u32) -> bool` | Whether a task is still to run. |
| `current-tick` | `func() -> u64` | The server's tick counter. |

## `plugin-permissions`

Access: not granted by any capability.

Permissions: nodes with defaults, plus what plugins set per player.

### enum `permission-default`

| Case | |
| --- | --- |
| `always` | Everyone has it. |
| `never` |  |
| `op` | Operators only. |
| `not-op` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `has` | `func(player: string, node: string) -> bool` | What a player has: what any plugin set (`*` and `a.b.*` match the nodes under them), else the node's default (operators for a node no one declared). |
| `is-set` | `func(player: string, node: string) -> bool` | Some plugin set it for the player. |
| `set` | `func(player: string, node: string, value: bool)` | Sets a node for a player, as this plugin's attachment. |
| `unset` | `func(player: string, node: string)` | Drops a node this plugin set for the player. |
| `clear` | `func(player: string)` | Drops every node this plugin set for the player. |
| `effective` | `func(player: string) -> list<tuple<string, bool>>` | The nodes set for the player, by every plugin. |
| `declare` | `func(node: string, default: permission-default, description: string, children: list<tuple<string, bool>>)` | Declares a node (the manifest's `[permissions]` do this at load). |

## `plugin-scoreboards`

Access: not granted by any capability.

Scoreboards: sidebars and name tags per player, and the server's own.

### record `objective`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` |  |
| `criteria` | `string` |  |
| `display-name` | `string` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-sidebar` | `func(player: string, title: string, lines: list<string>)` | A sidebar only this player sees: a title and up to 15 lines (text), top first. Replaces the previous one. |
| `clear-sidebar` | `func(player: string)` | Removes the sidebar `set-sidebar` showed. |
| `set-name-tag` | `func(target: string, prefix: string, suffix: string, color: option<string>)` | What every client shows around `target`'s name (text) and its colour (`red`, `gold` ...): a team of their own. |
| `clear-name-tag` | `func(target: string)` | Removes what `set-name-tag` put around the name. |
| `objectives` | `func() -> list<objective>` | The server's scoreboard (`/scoreboard`). |
| `add-objective` | `func(name: string, criteria: string, display-name: string)` | `/scoreboard objectives add`. |
| `remove-objective` | `func(name: string)` | `/scoreboard objectives remove`. |
| `set-display-slot` | `func(slot: string, objective: option<string>)` | `sidebar`, `list`, `below_name`, `sidebar.team.red` ... |
| `get-score` | `func(objective: string, holder: string) -> option<s32>` | A score holder's score; none when it has none. |
| `set-score` | `func(objective: string, holder: string, value: s32)` | `/scoreboard players set`. |
| `reset-score` | `func(objective: string, holder: string)` | `/scoreboard players reset`. |
| `teams` | `func() -> list<string>` | The server's teams (`/team`). |
| `add-team` | `func(name: string, display-name: string)` | `/team add`. |
| `remove-team` | `func(name: string)` | `/team remove`. |
| `join-team` | `func(team: string, member: string)` | `/team join`: a player name or another score holder. |
| `leave-team` | `func(member: string)` | `/team leave`. |
| `set-team-option` | `func(team: string, key: string, value: string)` | `/team modify <team> <key> <value>`. |

## `plugin-bossbars`

Access: not granted by any capability.

Boss bars a plugin shows.

### enum `bar-color`

| Case | |
| --- | --- |
| `pink` |  |
| `blue` |  |
| `red` |  |
| `green` |  |
| `yellow` |  |
| `purple` |  |
| `white` |  |

### enum `bar-style`

| Case | |
| --- | --- |
| `solid` |  |
| `segmented-six` |  |
| `segmented-ten` |  |
| `segmented-twelve` |  |
| `segmented-twenty` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `create` | `func(title: string, color: bar-color, style: bar-style, progress: f32) -> u32` | A new bar no one sees yet; its id. |
| `set-title` | `func(bar: u32, title: string)` | The bar's title (text). |
| `set-progress` | `func(bar: u32, progress: f32)` | 0 to 1. |
| `set-color` | `func(bar: u32, color: bar-color)` | The bar's colour. |
| `set-style` | `func(bar: u32, style: bar-style)` | Whether the bar is split in segments. |
| `set-flags` | `func(bar: u32, darken-sky: bool, play-music: bool, create-fog: bool)` | The sky darkens, the boss music plays, fog thickens. |
| `add-player` | `func(bar: u32, player: string)` | Shows the bar to a player. |
| `remove-player` | `func(bar: u32, player: string)` | Hides the bar from a player. |
| `players` | `func(bar: u32) -> list<string>` | The players who see the bar. |
| `remove` | `func(bar: u32)` | Hides the bar from everyone and forgets it. |

## `plugin-messaging`

Access: not granted by any capability.

Plugin channels: custom payloads to and from players' clients (a proxy's `bungeecord:main` too).

| Function | Signature | What it does |
| --- | --- | --- |
| `register-incoming` | `func(channel: string)` | Hear `plugin-message` events for payloads on `channel`. |
| `unregister-incoming` | `func(channel: string)` | Stop hearing `channel`. |
| `send` | `func(player: string, channel: string, data: list<u8>) -> bool` | A payload to the player's client (or the proxy in front of it); false when they aren't online. |

## `plugin-files`

Access: not granted by any capability.

The plugin's data folder, `plugins/<id>/` in the server's folder. Paths are relative, `/`-separated, of `[A-Za-z0-9_.-]` segments.

| Function | Signature | What it does |
| --- | --- | --- |
| `read` | `func(path: string) -> option<list<u8>>` | A file's bytes; none when it doesn't exist. |
| `write` | `func(path: string, data: list<u8>) -> result<_, string>` | Writes a file, making its folders. |
| `append` | `func(path: string, data: list<u8>) -> result<_, string>` | Adds to the end of a file, making it when missing. |
| `delete` | `func(path: string) -> bool` | Removes a file or an empty folder: false when there was none. |
| `exists` | `func(path: string) -> bool` | Whether a file or folder is there. |
| `list` | `func(dir: string) -> list<string>` | The names in a folder ("" for the data folder itself). |
| `bundled` | `func(path: string) -> option<list<u8>>` | A file the plugin's package carries under `defaults/`. |
| `save-default` | `func(path: string) -> bool` | Writes `bundled(path)` to `path` unless that file exists: true when it wrote it. |

## `plugin`

Access: exported by the mod: the game calls it.

What the server calls in a plugin.

### type `sender`

`type sender = sender`

### type `event`

`type event = event`

### type `priority`

`type priority = priority`

| Function | Signature | What it does |
| --- | --- | --- |
| `enable` | `func() -> result<_, string>` | Once, when the server enables the plugin (after the plugins it depends on). An error keeps it off and is logged. |
| `disable` | `func()` | When the server disables it (stopping, or reloading it). |
| `on-event` | `func(priority: priority, event: event) -> event` | An event of a kind the plugin listens to, at `priority`: the event back, changed as the plugin wants it (a monitor's changes are ignored). |
| `on-command` | `func(sender: sender, command: string, label: string, args: list<string>) -> bool` | `/label args` for a command the plugin owns (`command` its name, `label` the alias typed): false shows the command's usage. |
| `on-tab-complete` | `func(sender: sender, command: string, args: list<string>) -> list<string>` | What to offer for the last argument while `args` are typed. |
| `on-task` | `func(task: u32)` | A task of `plugin-scheduler` is due. |
