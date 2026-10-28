//! Methods of client files the server copy still calls (to prune).
#![allow(dead_code, unused_imports, unused_variables)]
use super::*;

impl crate::game::Sim<'_> {
    pub(crate) fn options_path() -> std::path::PathBuf {
        unreachable!("client only: options_path")
    }
}

impl crate::game::Sim<'_> {
    /// Find a game slot in the menu's complete, contiguous slot mapping.
    pub(crate) fn menu_slot_index(menu: &str, internal: usize) -> Option<usize> {
        unreachable!("client only: menu_slot_index")
    }
}

impl crate::game::Sim<'_> {
    /// `BlockEntity.triggerEvent(a, b)` on the client copy of the block entity at `pos`.
    pub(crate) fn block_entity_event(&mut self, pos: [i32; 3], a: i32, b: i32) {
        unreachable!("client only: block_entity_event")
    }
}

impl crate::game::Sim<'_> {
    /// `ServerboundChangeGameModePacket` handled by the integrated server:
    /// `GameModeCommand.setGameMode` with its feedback.
    pub(crate) fn change_game_mode(&mut self, mode: GameMode) {
        unreachable!("client only: change_game_mode")
    }
}

impl crate::game::Sim<'_> {
    /// `BellBlockEntity.updateEntities`: the living entities whose bounding
    /// box meets the bell's block box inflated by 48.
    pub(crate) fn bell_gather(&self, pos: [i32; 3]) -> Vec<u64> {
        unreachable!("client only: bell_gather")
    }
}

impl crate::game::Sim<'_> {
    /// `BellBlockEntity.updateEntities`: every gathered entity alive within 32
    /// blocks of the bell's centre gets `HEARD_BELL_TIME` (only villagers
    /// have a brain that reads it).
    pub(crate) fn bell_heard(&mut self, pos: [i32; 3], now: i64) {
        unreachable!("client only: bell_heard")
    }
}

impl crate::game::Sim<'_> {
    /// A line in the connection's chat, with its styles, click and hover
    /// events (ChatComponent.addMessage). The lines the Chat option hides
    /// are dropped there (its `visibleMessageFilter`).
    pub(crate) fn chat_line(
        &mut self,
        content: horizon_client::text::Text,
        source: horizon_client::chat::ChatSource,
    ) {
        unreachable!("client only: chat_line")
    }
}

impl crate::game::Sim<'_> {
    /// `TrialSpawner.tickClient`: the state's particles, the spin of the
    /// mob shown inside and the ambient sound.
    pub(crate) fn client_tick_trial_spawner(&mut self, key: Pos) {
        unreachable!("client only: client_tick_trial_spawner")
    }
}

impl crate::game::Sim<'_> {
    /// `VaultBlockEntity.Client.tick`.
    pub(crate) fn client_tick_vault(&mut self, key: Pos) {
        unreachable!("client only: client_tick_vault")
    }
}

impl crate::game::Sim<'_> {
    /// A player opened (`open`) or closed the container at `key`: the opener
    /// count block event (`ContainerOpenersCounter` / `ShulkerBoxBlockEntity`
    /// `startOpen`/`stopOpen`) for chests, ender chests and shulker boxes,
    /// on both halves of a double chest (`CompoundContainer`). True when the
    /// count went from 0 to 1 or back to 0: the only moments the container
    /// plays its open or close sound.
    pub(crate) fn container_opener_event(&mut self, key: (i32, i32, i32), open: bool) -> bool {
        unreachable!("client only: container_opener_event")
    }
}

impl crate::game::Sim<'_> {
    /// `container_opener_event` for a player who is not a spectator (a guest).
    pub(crate) fn container_openers_changed(&mut self, key: (i32, i32, i32), open: bool) -> bool {
        unreachable!("client only: container_openers_changed")
    }
}

impl crate::game::Sim<'_> {
    /// The hurts and melee attacks each player logged since the last call
    /// (`PlayerMotion::hurt_log`, `attack_log`), taken out.
    pub(crate) fn control_hurt_log(&mut self) -> Value {
        unreachable!("client only: control_hurt_log")
    }
}

impl crate::game::Sim<'_> {
    /// `horizon-ctl save` and `save-all`.
    pub(crate) fn control_save(&mut self, all: bool) -> Result<()> {
        unreachable!("client only: control_save")
    }
}

impl crate::game::Sim<'_> {
    /// Whether a dialog screen takes the input.
    pub(crate) fn dialog_open(&self) -> bool {
        unreachable!("client only: dialog_open")
    }
}

impl crate::game::Sim<'_> {
    /// A server's `flush_to_window`: it has no window, so what the game queued for one goes.
    pub(crate) fn drop_window_output(&mut self) {
        unreachable!("client only: drop_window_output")
    }
}

impl crate::game::Sim<'_> {
    /// `LivingEntity.spawnItemParticles(stack, count)` for the local player,
    /// drawing from the world random as its eating sounds do.
    pub(crate) fn eat_particles(&mut self, stack: &ItemStack, count: u32) {
        unreachable!("client only: eat_particles")
    }
}

impl crate::game::Sim<'_> {
    /// The slime ball, snowball and cobweb models behind `item_slime`,
    /// `item_snowball` and `item_cobweb`, resolved on first use.
    pub(crate) fn ensure_item_particle_sprites(&mut self) {
        unreachable!("client only: ensure_item_particle_sprites")
    }
}

impl crate::game::Sim<'_> {
    /// `AbstractVillager.getOffers`: generated on first use.
    pub(crate) fn ensure_offers(&mut self, index: usize) {
        unreachable!("client only: ensure_offers")
    }
}

impl crate::game::Sim<'_> {
    /// `isEyeInFluid(WATER)`: the eye inside the water of its block column.
    pub(crate) fn eye_in_water(&self, x: f64, eye_y: f64, z: f64) -> bool {
        unreachable!("client only: eye_in_water")
    }
}

impl crate::game::Sim<'_> {
    /// `EndGatewayBlock.entityInside` on the server: whether the gateway at
    /// `pos` let the entity in (`triggerCooldown`, with its block event).
    pub(crate) fn gateway_enter(&mut self, pos: [i32; 3]) -> bool {
        unreachable!("client only: gateway_enter")
    }
}

impl crate::game::Sim<'_> {
    /// The End gateway at `pos` as its chunk was loaded: `Age` from the saved
    /// block entity (`getUpdateTag`), or a new gateway.
    pub(crate) fn gateway_loaded(&self, pos: [i32; 3]) -> EndGateway {
        unreachable!("client only: gateway_loaded")
    }
}

impl crate::game::Sim<'_> {
    /// `Entity.isInRain`: rain at the block position or at the top of the box.
    pub(crate) fn in_rain(&mut self, at: Vec3, top: f64) -> bool {
        unreachable!("client only: in_rain")
    }
}

impl crate::game::Sim<'_> {
    /// `Villager.mobInteract` / `WanderingTrader.mobInteract`: shake the
    /// head without offers, else open the trading screen.
    pub(crate) fn interact_trader(
        &mut self,
        index: usize,
        item: &str,
    ) -> horizon_client::Result<bool> {
        unreachable!("client only: interact_trader")
    }
}

impl crate::game::Sim<'_> {
    /// `ItemParticleOption(ITEM, stack)`: the layer particle sprites of the
    /// stack's model resolved for the ground, with seed 0.
    pub(crate) fn item_particle(&self, stack: &ItemStack) -> ParticleOptions {
        unreachable!("client only: item_particle")
    }
}

impl crate::game::Sim<'_> {
    /// Map a menu slot to Horizon's inventory screen slot ids.
    pub(crate) fn menu_slot(menu: &str, slot: i32) -> Option<usize> {
        unreachable!("client only: menu_slot")
    }
}

impl crate::game::Sim<'_> {
    /// What a menu click's session did, applied to the world and its mob;
    /// with the result of each trade made, for `merchant_traded`.
    pub(crate) fn merchant_apply(
        &mut self,
        session: Box<merchant::Session>,
        mob: Option<u32>,
    ) -> (Box<merchant::Session>, Vec<ItemStack>) {
        unreachable!("client only: merchant_apply")
    }
}

impl crate::game::Sim<'_> {
    /// `Villager.setTradingPlayer(null)`: the session goes back to the mob
    /// (special prices stay as they were), the borrowed sequences to the world.
    pub(crate) fn merchant_closed(&mut self, state: &mut MerchantState) {
        unreachable!("client only: merchant_closed")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn merchant_scroll(&mut self, d: f32) {
        unreachable!("client only: merchant_scroll")
    }
}

impl crate::game::Sim<'_> {
    /// The trading mob's random before a menu click (it ticks meanwhile).
    pub(crate) fn merchant_sync_in(&self, session: &mut merchant::Session, mob: Option<u32>) {
        unreachable!("client only: merchant_sync_in")
    }
}

impl crate::game::Sim<'_> {
    /// After a menu click: the session back to the station, its random to
    /// the mob, and what it did (orbs, level-ups, the yes/no voice).
    pub(crate) fn merchant_sync_out(&mut self, session: Box<merchant::Session>) {
        unreachable!("client only: merchant_sync_out")
    }
}

impl crate::game::Sim<'_> {
    /// The offer buttons a controller can focus (GUI origin relative).
    pub(crate) fn merchant_targets(&self) -> Vec<[f32; 4]> {
        unreachable!("client only: merchant_targets")
    }
}

impl crate::game::Sim<'_> {
    /// `Villager.customServerAiStep`: the TRADE reputation event of the last
    /// trade, a tick later; the prices follow the new reputation.
    pub(crate) fn merchant_tick(&mut self) {
        unreachable!("client only: merchant_tick")
    }
}

impl crate::game::Sim<'_> {
    /// `MerchantResultSlot.checkTakeAchievements` and `AbstractVillager.notifyTrade`
    /// for each trade made: the statistic and the `villager_trade` trigger.
    pub(crate) fn merchant_traded(&mut self, mob: Option<u32>, results: &[ItemStack]) {
        unreachable!("client only: merchant_traded")
    }
}

impl crate::game::Sim<'_> {
    /// Whether the mount menu is the open screen.
    pub(crate) fn mount_active(&self) -> bool {
        unreachable!("client only: mount_active")
    }
}

impl crate::game::Sim<'_> {
    /// Closing the screen keeps what is in the slots on the mount.
    pub(crate) fn mount_closed(&mut self) {
        unreachable!("client only: mount_closed")
    }
}

impl crate::game::Sim<'_> {
    /// The slots of mount `index` as its menu shows them: the saddle, the
    /// body item (None when the slot isn't active), the chest.
    pub(crate) fn mount_contents(
        &self,
        index: usize,
    ) -> (
        Option<ItemStack>,
        Option<Option<ItemStack>>,
        Vec<Option<ItemStack>>,
    ) {
        unreachable!("client only: mount_contents")
    }
}

impl crate::game::Sim<'_> {
    /// The chest columns of the open mount menu (the server's window's).
    pub(crate) fn mount_menu_columns(&self) -> Option<usize> {
        unreachable!("client only: mount_menu_columns")
    }
}

impl crate::game::Sim<'_> {
    /// The entity type of the open mount menu's mount (`minecraft:horse`).
    pub(crate) fn mount_menu_entity(&self) -> Option<String> {
        unreachable!("client only: mount_menu_entity")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn mount_slot_mut(&mut self, i: usize) -> &mut Option<ItemStack> {
        unreachable!("client only: mount_slot_mut")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn mount_slot_position(&self, i: usize) -> Option<(f32, f32)> {
        unreachable!("client only: mount_slot_position")
    }
}

impl crate::game::Sim<'_> {
    /// `ArmorSlot.mayPlace` and the stack limits of the mount's slots.
    pub(crate) fn mount_slot_rules(&self, slot: usize, stack: &ItemStack) -> (bool, u32) {
        unreachable!("client only: mount_slot_rules")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn mount_slots(&self) -> Option<Vec<usize>> {
        unreachable!("client only: mount_slots")
    }
}

impl crate::game::Sim<'_> {
    /// The menu's slots back on mount `index` (`body` None: not its slot),
    /// with the equip sound of what was put on (`Equippable.equipSound`).
    pub(crate) fn mount_store(
        &mut self,
        index: usize,
        saddle: Option<ItemStack>,
        body: Option<Option<ItemStack>>,
        items: Vec<Option<ItemStack>>,
    ) {
        unreachable!("client only: mount_store")
    }
}

impl crate::game::Sim<'_> {
    /// `AbstractHorse.openCustomInventoryScreen` for a tamed adult mount.
    pub(crate) fn open_mount_menu(&mut self, index: usize) {
        unreachable!("client only: open_mount_menu")
    }
}

impl crate::game::Sim<'_> {
    /// E while riding a tamed mount opens its inventory instead of the player's.
    pub(crate) fn open_ridden_mount_menu(&mut self) -> bool {
        unreachable!("client only: open_ridden_mount_menu")
    }
}

impl crate::game::Sim<'_> {
    /// The blocks a use changed, set on this client's level and kept as
    /// predicted under the use's sequence until the server acknowledges it
    /// (`BlockStatePredictionHandler.retainKnownServerState`): a use the
    /// server refused goes back.
    pub(crate) fn predict_blocks(&mut self, changes: &DryChanges, seq: i32) {
        unreachable!("client only: predict_blocks")
    }
}

impl crate::game::Sim<'_> {
    /// Their kind and place, for `status`.
    pub(crate) fn projectiles_status(&self) -> Vec<serde_json::Value> {
        unreachable!("client only: projectiles_status")
    }
}

impl crate::game::Sim<'_> {
    /// How far the open book pushes the menu to the right
    /// (`RecipeBookComponent.updateScreenPosition`).
    pub(crate) fn recipe_book_shift(&self, width: f32) -> f32 {
        unreachable!("client only: recipe_book_shift")
    }
}

impl crate::game::Sim<'_> {
    /// Whether typed keys go to the search box.
    pub(crate) fn recipe_search_focused(&self) -> bool {
        unreachable!("client only: recipe_search_focused")
    }
}

impl crate::game::Sim<'_> {
    /// Text typed (or one backspace) into the search box.
    pub(crate) fn recipe_text(&mut self, text: Option<&str>, backspace: bool) {
        unreachable!("client only: recipe_text")
    }
}

impl crate::game::Sim<'_> {
    /// `RecipeToast.addOrUpdate` for each display of the recipes just
    /// unlocked: its crafting station and result join the toast still
    /// queued or shown, or a new one.
    pub(crate) fn recipe_toast(&mut self, recipes: &[String]) {
        unreachable!("client only: recipe_toast")
    }
}

impl crate::game::Sim<'_> {
    /// `AnvilScreen.onNameChanged` on a server: the rename box's text (blank
    /// when it is the input's own name) goes to the server when the menu's
    /// `setItemName` takes it.
    pub(crate) fn remote_anvil_rename(&mut self) {
        unreachable!("client only: remote_anvil_rename")
    }
}

impl crate::game::Sim<'_> {
    /// A server's chest-like menu (`MenuType.GENERIC_9xN`): its rows and title.
    pub(crate) fn remote_chest_menu(&self) -> Option<(usize, horizon_client::text::Text)> {
        unreachable!("client only: remote_chest_menu")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn remote_close_container(&mut self) {
        unreachable!("client only: remote_close_container")
    }
}

impl crate::game::Sim<'_> {
    /// `CreativeModeInventoryScreen.slotClicked` outside the window: the server knows nothing of
    /// a stack picked from the creative screen, so the carried stack (an item tab's right click:
    /// one of it) is dropped through `handleCreativeModeItemDrop`, a SetCreativeModeSlot to slot
    /// -1, and the hand swings as `Player.drop` predicts.
    pub(crate) fn remote_creative_drop(&mut self, right: bool) {
        unreachable!("client only: remote_creative_drop")
    }
}

impl crate::game::Sim<'_> {
    /// The server's network id of a mob effect (`brewing::EFFECTS` id).
    pub(crate) fn remote_effect_network_id(&self, effect: &str) -> Option<i32> {
        unreachable!("client only: remote_effect_network_id")
    }
}

impl crate::game::Sim<'_> {
    /// Shared flag 6 of an entity on a server's client (`None` without a server).
    pub(crate) fn remote_glowing(&self, id: u32) -> Option<bool> {
        unreachable!("client only: remote_glowing")
    }
}

impl crate::game::Sim<'_> {
    /// The server's open menu: its window id and menu type (0, "inventory"
    /// when none is open).
    pub(crate) fn remote_menu(&self) -> (i32, String) {
        unreachable!("client only: remote_menu")
    }
}

impl crate::game::Sim<'_> {
    /// `MultiPlayerGameMode.handleContainerInput` for each click of `ops` on
    /// the server's open menu (a quick craft drag is several): the verified
    /// menu engine predicts them on this client's copy, then each goes out as
    /// a `ContainerClick` with the slots it changed and the carried stack
    /// after it; the server corrects anything it sees differently. A menu the
    /// engine doesn't model sends the clicks unpredicted. False when a click's
    /// slot isn't one of the menu's.
    pub(crate) fn remote_menu_ops(&mut self, ops: &[(Option<usize>, i32, Input)]) -> bool {
        unreachable!("client only: remote_menu_ops")
    }
}

impl crate::game::Sim<'_> {
    /// The type of the menu a server opened (`MenuType`'s id).
    pub(crate) fn remote_menu_type(&self) -> Option<&str> {
        unreachable!("client only: remote_menu_type")
    }
}

impl crate::game::Sim<'_> {
    /// The nearest boat, minecart, item frame, armor stand or painting the
    /// view hits within reach; also End crystals when attacking.
    pub(crate) fn remote_object_hit(&self, attacking: bool) -> Option<ObjectHit> {
        unreachable!("client only: remote_object_hit")
    }
}

impl crate::game::Sim<'_> {
    /// The names in the server's player list, sorted.
    pub(crate) fn remote_online_names(&self) -> Option<Vec<String>> {
        unreachable!("client only: remote_online_names")
    }
}

impl crate::game::Sim<'_> {
    /// Status for `horizon-ctl status`.
    /// The other players' entities in the level and their names (from the
    /// player list).
    pub(crate) fn remote_player_names(&self) -> Vec<(u32, String)> {
        unreachable!("client only: remote_player_names")
    }
}

impl crate::game::Sim<'_> {
    /// A `Holder<SoundEvent>` off the wire: an inline event's location, or
    /// the name of a registered one.
    pub(crate) fn remote_sound_name(
        &self,
        sound: &(i32, Option<(String, Option<f32>)>),
    ) -> Option<String> {
        unreachable!("client only: remote_sound_name")
    }
}

impl crate::game::Sim<'_> {
    /// `MultiPlayerGameMode.handleInventoryButtonClick`: the screen already
    /// ran `clickMenuButton` on its own menu.
    pub(crate) fn remote_station_button(&self, button: i32) {
        unreachable!("client only: remote_station_button")
    }
}

impl crate::game::Sim<'_> {
    /// ServerboundPlayerCommandPacket STOP_SLEEPING.
    pub(crate) fn remote_stop_sleeping(&mut self) {
        unreachable!("client only: remote_stop_sleeping")
    }
}

impl crate::game::Sim<'_> {
    /// The server's open menu is the inventory or a chest-like container
    /// (chests, barrels, ender chests: `generic_9xN`; shulker boxes).
    pub(crate) fn remote_storage_open(&self) -> bool {
        unreachable!("client only: remote_storage_open")
    }
}

impl crate::game::Sim<'_> {
    /// The player list's view of the connection: the listed players (with
    /// their downloaded skins once ready), online mode, and which players
    /// are in the level with an upside-down name.
    pub(crate) fn remote_tab_players(
        &mut self,
    ) -> Option<(Vec<horizon_client::tab_list::TabPlayer>, bool, Vec<u128>)> {
        unreachable!("client only: remote_tab_players")
    }
}

impl crate::game::Sim<'_> {
    /// A click outside a server's window (slot -999): predicted like
    /// `MultiPlayerGameMode.handleContainerInput` (the carried stack after
    /// `doClick`; the server drops the stack and swings the hand).
    pub(crate) fn remote_throw(&mut self, right: bool, input: horizon_client::menu::Input) {
        unreachable!("client only: remote_throw")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn request_remote_mining_stop(&mut self) {
        unreachable!("client only: request_remote_mining_stop")
    }
}

impl crate::game::Sim<'_> {
    /// `RandomSequences` as `random_sequences.dat` (not on a server's guest).
    pub(crate) fn save_random_sequences(&mut self) {
        unreachable!("client only: save_random_sequences")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn send(&self, packet: S) {
        unreachable!("client only: send")
    }
}

impl crate::game::Sim<'_> {
    /// ClientPacketListener.sendChat / sendCommand (unsigned). The Chat
    /// option's Hidden and System levels keep chat out and Hidden keeps the
    /// commands out (ChatAbilities).
    pub(crate) fn send_chat(&mut self, message: &str) {
        unreachable!("client only: send_chat")
    }
}

impl crate::game::Sim<'_> {
    /// `ShulkerBoxBlock.canOpen`: a closed box opens only when its lid has
    /// room to rise (`level.noCollision` of the half-open lid box).
    pub(crate) fn shulker_can_open(&self, pos: [i32; 3]) -> bool {
        unreachable!("client only: shulker_can_open")
    }
}

impl crate::game::Sim<'_> {
    /// `AbstractHurtingProjectile.createParticleTrail`: one smoke puff half a
    /// block above each position.
    pub(crate) fn smoke_trails(&mut self, at: impl IntoIterator<Item = Vec3>) {
        unreachable!("client only: smoke_trails")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn state_string(&self, state: BlockStateId) -> String {
        unreachable!("client only: state_string")
    }
}

impl crate::game::Sim<'_> {
    /// `SpyglassItem.releaseUsing` / `finishUsingItem` (after 1200 ticks).
    pub(crate) fn stop_spyglass(&mut self) {
        unreachable!("client only: stop_spyglass")
    }
}

impl crate::game::Sim<'_> {
    /// `ConduitBlockEntity.serverTick` of every conduit in the chunks that
    /// tick blocks, with or without a renderer. The conduits are looked for
    /// when the shape is checked, every 40 ticks (they have no block entity
    /// of their own here).
    pub(crate) fn tick_conduit_servers(&mut self) {
        unreachable!("client only: tick_conduit_servers")
    }
}

impl crate::game::Sim<'_> {
    /// `TheEndGatewayBlockEntity.portalTick` (the server ticker) of the
    /// gateways a server keeps a copy of, in block-ticking chunks
    /// (`Level.tickBlockEntities`): the cooldown an entry started runs out,
    /// and every 2400 ticks `triggerCooldown` sends its block event.
    pub(crate) fn tick_gateway_servers(&mut self) {
        unreachable!("client only: tick_gateway_servers")
    }
}

impl crate::game::Sim<'_> {
    /// The menu ends when the mount dies or is more than 4 blocks away
    /// (`AbstractMountInventoryMenu.stillValid`).
    pub(crate) fn tick_mount_menu(&mut self) {
        unreachable!("client only: tick_mount_menu")
    }
}

impl crate::game::Sim<'_> {
    /// `LocalPlayer.setExperienceValues`: a change of progress shows the
    /// experience bar for 100 ticks, except the first one
    /// (`experienceDisplayStartTick` starts at `Integer.MIN_VALUE`).
    pub(crate) fn track_experience_display(&mut self) {
        unreachable!("client only: track_experience_display")
    }
}

impl crate::game::Sim<'_> {
    /// `LevelEventHandler.levelEvent` 3011 to 3021.
    pub(crate) fn trial_level_event(&mut self, id: i32, pos: [i32; 3], data: i32) {
        unreachable!("client only: trial_level_event")
    }
}

impl crate::game::Sim<'_> {
    /// `SpyglassItem.use`: start scoping.
    pub(crate) fn use_spyglass(&mut self) {
        unreachable!("client only: use_spyglass")
    }
}

impl crate::game::Sim<'_> {
    /// The book the open screen has, if any.
    pub(crate) fn active_book(&self) -> Option<BookType> {
        unreachable!("client only: active_book")
    }
}

impl crate::game::Sim<'_> {
    /// `BarrelBlockEntity.openersCounter`: the first opener opens the barrel
    /// (`open=true`), the last one to leave closes it (`updateBlockState`).
    /// True when the count crossed zero.
    pub(crate) fn barrel_openers_changed(&mut self, key: (i32, i32, i32), open: bool) -> bool {
        unreachable!("client only: barrel_openers_changed")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn book_open(&self, book: BookType) -> bool {
        unreachable!("client only: book_open")
    }
}

impl crate::game::Sim<'_> {
    /// `TrialSpawner.addDetectPlayerParticles`.
    pub(crate) fn detect_player_particles(&mut self, pos: [i32; 3], data: i32, ty: T) {
        unreachable!("client only: detect_player_particles")
    }
}

impl crate::game::Sim<'_> {
    /// `TrialSpawner.addEjectItemParticles`.
    pub(crate) fn eject_item_particles(&mut self, pos: [i32; 3]) {
        unreachable!("client only: eject_item_particles")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn ensure_book(&mut self) {
        unreachable!("client only: ensure_book")
    }
}

impl crate::game::Sim<'_> {
    /// Sets `be_anims.conduit_servers` to the conduits in the
    /// chunks that tick blocks, keeping the state of those already known.
    pub(crate) fn find_conduits(&mut self) {
        unreachable!("client only: find_conduits")
    }
}

impl crate::game::Sim<'_> {
    /// `ContainerOpenersCounter.incrementOpeners` / `decrementOpeners` of the
    /// chest at `key` (the host and the guests are counted), then
    /// `ChestBlockEntity.signalOpenCount`: block event 1 with the count, here
    /// and for the guests. True when the count crossed zero.
    pub(crate) fn lid_openers_changed(&mut self, key: (i32, i32, i32), open: bool) -> bool {
        unreachable!("client only: lid_openers_changed")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn local_sound(&mut self, name: &str, pos: [i32; 3], volume: f32, pitch: f32) {
        unreachable!("client only: local_sound")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn mount_mob_index(&self, id: u32) -> Option<usize> {
        unreachable!("client only: mount_mob_index")
    }
}

impl crate::game::Sim<'_> {
    /// Writes the menu slots to the mount, with the equip sounds. A
    /// server's menu is the server's to write.
    pub(crate) fn mount_sync(&mut self) {
        unreachable!("client only: mount_sync")
    }
}

impl crate::game::Sim<'_> {
    /// `AbstractVillager.addOffersFromTradeSet` with the world's random
    /// sequence of the set, at the merchant's position.
    pub(crate) fn offers_from_set(
        &mut self,
        set: &str,
        variant: &str,
        origin: [f64; 3],
    ) -> Vec<MerchantOffer> {
        unreachable!("client only: offers_from_set")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn open_merchant_state(&mut self, state: MerchantState) {
        unreachable!("client only: open_merchant_state")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn particle(&mut self, ty: T, p: [f64; 3], v: [f64; 3]) {
        unreachable!("client only: particle")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn random_sequences_path(&self) -> std::path::PathBuf {
        unreachable!("client only: random_sequences_path")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn remote_close_local(&mut self) {
        unreachable!("client only: remote_close_local")
    }
}

impl crate::game::Sim<'_> {
    /// The workstation screen of a menu type a server opens.
    pub(crate) fn remote_station_kind(menu: &str) -> Option<super::stations::Kind> {
        unreachable!("client only: remote_station_kind")
    }
}

impl crate::game::Sim<'_> {
    /// Window `container` is the server's workstation menu, open here.
    pub(crate) fn remote_station_open(&self, container: i32) -> bool {
        unreachable!("client only: remote_station_open")
    }
}

impl crate::game::Sim<'_> {
    /// `ItemModelResolver.updateForTopItem`.
    pub(crate) fn resolve_item(
        &self,
        stack: &ItemStack,
        display: DisplayContext,
        owner: Option<&OwnerState>,
        seed: i32,
    ) -> ItemRenderState {
        unreachable!("client only: resolve_item")
    }
}

impl crate::game::Sim<'_> {
    /// The block entity at `pos` Horizon keeps as loaded, if any.
    pub(crate) fn saved_tag(&self, pos: [i32; 3]) -> Option<&Tag> {
        unreachable!("client only: saved_tag")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn send_clicks(&mut self, container: i32, clicks: Vec<Predicted>) {
        unreachable!("client only: send_clicks")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn set_block(&mut self, pos: [i32; 3], state: i32) {
        unreachable!("client only: set_block")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn show_mount_menu(&mut self) {
        unreachable!("client only: show_mount_menu")
    }
}

impl crate::game::Sim<'_> {
    /// `ConduitBlockEntity.serverTick` of the conduit at `pos`, in a chunk
    /// that ticks blocks: the shape check every 40 ticks with its sounds, the
    /// power for players in range, the hurt for an enemy, then the ambient
    /// sounds.
    pub(crate) fn tick_conduit_server(&mut self, pos: [i32; 3]) {
        unreachable!("client only: tick_conduit_server")
    }
}

impl crate::game::Sim<'_> {
    /// A stack to the server (`SetCreativeModeSlot`): the same component
    /// patch a Horizon server sends its clients, with the registries' ids, so
    /// the server's echo doesn't hand back a bare item.
    pub(crate) fn to_network(&self, s: &Option<ItemStack>) -> Option<Stack> {
        unreachable!("client only: to_network")
    }
}

impl crate::game::Sim<'_> {
    /// `TrialSpawner.addSpawnParticles`.
    pub(crate) fn trial_spawn_particles(&mut self, pos: [i32; 3], ty: T) {
        unreachable!("client only: trial_spawn_particles")
    }
}

impl crate::game::Sim<'_> {
    /// `PlayerInfo.getSkin` for every player entity, including NPCs that are
    /// never in the tab list: resolve each profile's textures and give the
    /// level's player models their skin.
    pub(crate) fn update_remote_skins(&mut self) {
        unreachable!("client only: update_remote_skins")
    }
}

impl crate::game::Sim<'_> {
    /// `VaultBlockEntity.Client.emitActivationParticles`.
    pub(crate) fn vault_activation_particles(&mut self, pos: [i32; 3], ominous: bool) {
        unreachable!("client only: vault_activation_particles")
    }
}

impl crate::game::Sim<'_> {
    /// `emitConnectionParticlesForNearbyPlayers`: from the keyhole towards
    /// each connected player within the particles' range.
    pub(crate) fn vault_connection_particles(&mut self, pos: [i32; 3]) {
        unreachable!("client only: vault_connection_particles")
    }
}

impl crate::game::Sim<'_> {
    /// `show_skin`, or — its slot evicted — the image queued for upload again.
    pub(crate) fn again_skin(&mut self, location: &str) -> bool {
        unreachable!("client only: again_skin")
    }
}

impl crate::game::Sim<'_> {
    /// `ConduitBlockEntity.applyEffects`: conduit power for every player
    /// in water or rain within `frames / 7 * 16` blocks.
    pub(crate) fn conduit_apply_effects(&mut self, pos: [i32; 3], frames: usize) {
        unreachable!("client only: conduit_apply_effects")
    }
}

impl crate::game::Sim<'_> {
    /// `ConduitBlockEntity.updateAndAttackTarget`: while the conduit hunts
    /// (42 frame blocks) its target stays until it dies or leaves the 8
    /// blocks, else a random enemy in water or rain is chosen; the target
    /// takes 4 magic damage.
    pub(crate) fn conduit_attack(&mut self, conduit: &mut ConduitServer, pos: [i32; 3], hunting: bool) {
        unreachable!("client only: conduit_attack")
    }
}

impl crate::game::Sim<'_> {
    /// One opener more (`open`) or less at `key`: the count before and after
    /// (`ContainerOpenersCounter.incrementOpeners` / `decrementOpeners`).
    pub(crate) fn count_opener(&mut self, key: (i32, i32, i32), open: bool) -> (i32, i32) {
        unreachable!("client only: count_opener")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn item_env(&self) -> Env<'_, '_> {
        unreachable!("client only: item_env")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn item_id(&self, name: &str) -> i32 {
        unreachable!("client only: item_id")
    }
}

impl crate::game::Sim<'_> {
    /// The client level as item models read it: worked out once a frame (a
    /// screen of items read the same level, its sky sampled once).
    pub(crate) fn item_level(&self) -> LevelState {
        unreachable!("client only: item_level")
    }
}

impl crate::game::Sim<'_> {
    /// A server closed its window: the screen's copy goes.
    pub(crate) fn mount_menu_reset(&mut self) {
        unreachable!("client only: mount_menu_reset")
    }
}

impl crate::game::Sim<'_> {
    /// The signed-in account's `textures`, once the services answered.
    pub(crate) fn own_textures(&self) -> Option<horizon_client::skins::Textures> {
        unreachable!("client only: own_textures")
    }
}

impl crate::game::Sim<'_> {
    /// Keep the renderable block entities (chests, furnaces) in step with blocks.
    pub(crate) fn sync_block_entity(&mut self, pos: (i32, i32, i32), state: BlockStateId) {
        unreachable!("client only: sync_block_entity")
    }
}

impl crate::game::Sim<'_> {
    /// `ConduitBlockEntity.selectNewTarget`: one of the enemies in water or
    /// rain whose box meets the conduit's block inflated by 8.
    pub(crate) fn conduit_new_target(&mut self, pos: [i32; 3]) -> Option<u32> {
        unreachable!("client only: conduit_new_target")
    }
}

impl crate::game::Sim<'_> {
    pub(crate) fn level_state_now(&self, dimension: &'static str) -> LevelState {
        unreachable!("client only: level_state_now")
    }
}

impl crate::game::Sim<'_> {
    /// Whether a downloaded skin is in the atlas, marking it recently shown.
    pub(crate) fn show_skin(&mut self, location: &str) -> bool {
        unreachable!("client only: show_skin")
    }
}
