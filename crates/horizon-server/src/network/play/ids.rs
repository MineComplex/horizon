//! Play packet ids of protocol 777 (26.3): a name's index is its id, in `GameProtocols`'s
//! registration order (the clientbound bundle delimiter first, where `withBundlePacket` adds it).
//! Generated from the 26.3 classes; the ids the tests encode are checked against the Java codec.

#[rustfmt::skip]
pub const CLIENTBOUND: [&str; 144] = [
    "bundle_delimiter", "add_entity", "animate", "award_stats", "block_changed_ack", "block_destruction",
    "block_entity_data", "block_event", "block_update", "boss_event", "change_difficulty",
    "chunk_batch_finished", "chunk_batch_start", "chunks_biomes", "clear_titles", "command_suggestions",
    "commands", "container_close", "container_set_content", "container_set_data", "container_set_slot",
    "cookie_request", "cooldown", "custom_chat_completions", "custom_payload", "damage_event",
    "debug/block_value", "debug/chunk_value", "debug/entity_value", "debug/event", "debug_sample",
    "delete_chat", "disconnect", "disguised_chat", "entity_event", "entity_position_sync", "explode",
    "add_transient_block", "forget_level_chunk", "game_event", "game_rule_values", "game_test_highlight_pos",
    "mount_screen_open", "hurt_animation", "initialize_border", "keep_alive", "level_chunk_with_light",
    "level_event", "level_particles", "light_update", "login", "low_disk_space_warning", "map_item_data",
    "merchant_offers", "move_entity_pos", "move_entity_pos_rot", "move_minecart_along_track",
    "move_entity_rot", "move_vehicle", "open_book", "open_screen", "open_sign_editor", "ping",
    "pong_response", "place_ghost_recipe", "player_abilities", "player_chat", "player_combat_end",
    "player_combat_enter", "player_combat_kill", "player_info_remove", "player_info_update", "player_look_at",
    "player_position", "player_rotation", "recipe_book_add", "recipe_book_remove", "recipe_book_settings",
    "remove_entities", "remove_mob_effect", "reset_score", "resource_pack_pop", "resource_pack_push",
    "post_effects", "respawn", "rotate_head", "section_blocks_update", "select_advancements_tab",
    "server_data", "set_action_bar_text", "set_border_center", "set_border_lerp_size", "set_border_size",
    "set_border_warning_delay", "set_border_warning_distance", "set_camera", "set_chunk_cache_center",
    "set_chunk_cache_radius", "set_cursor_item", "set_default_spawn_position", "set_display_objective",
    "set_entity_data", "set_entity_link", "set_entity_motion", "set_equipment", "set_experience",
    "set_health", "set_held_slot", "set_objective", "set_passengers", "set_player_inventory",
    "set_player_team", "set_score", "set_simulation_distance", "set_subtitle_text", "set_time",
    "set_title_text", "set_titles_animation", "sound_entity", "sound", "start_configuration", "stop_sound",
    "store_cookie", "swing_animation", "system_chat", "tab_list", "tag_query", "take_item_entity",
    "teleport_entity", "test_instance_block_status", "ticking_state", "ticking_step", "transfer",
    "update_advancements", "update_attributes", "update_mob_effect", "update_recipes", "update_tags",
    "projectile_power", "custom_report_details", "server_links", "waypoint", "clear_dialog", "show_dialog",
];

#[rustfmt::skip]
pub const SERVERBOUND: [&str; 69] = [
    "accept_teleportation", "attack", "block_entity_tag_query", "bundle_item_selected", "change_difficulty",
    "change_game_mode", "chat_ack", "chat_command", "chat_command_signed", "chat", "chat_session_update",
    "chunk_batch_received", "client_command", "client_tick_end", "client_information", "command_suggestion",
    "configuration_acknowledged", "container_button_click", "container_click", "container_close",
    "container_slot_state_changed", "cookie_response", "custom_payload", "debug_subscription_request",
    "edit_book", "entity_tag_query", "interact", "jigsaw_generate", "keep_alive", "lock_difficulty",
    "move_player_pos", "move_player_pos_rot", "move_player_rot", "move_player_status_only", "move_vehicle",
    "paddle_boat", "pick_item_from_block", "pick_item_from_entity", "ping_request", "place_recipe",
    "player_abilities", "player_action", "player_command", "player_input", "player_loaded", "pong", "punch",
    "recipe_book_change_settings", "recipe_book_seen_recipe", "rename_item", "resource_pack",
    "seen_advancements", "select_trade", "set_beacon", "set_carried_item", "set_command_block",
    "set_command_minecart", "set_creative_mode_slot", "set_game_rule", "set_jigsaw_block",
    "set_structure_block", "set_test_block", "sign_update", "spectator_action", "teleport_to_entity",
    "test_instance_block_action", "use_item_on", "use_item", "custom_click_action",
];

/// The id of a clientbound play packet by name (`None` when 26.3 has no such packet).
pub fn clientbound(name: &str) -> Option<u8> {
    CLIENTBOUND.iter().position(|n| *n == name).map(|i| i as u8)
}

/// Clientbound ids this module encodes or the join sequence names.
pub mod cb {
    pub const BLOCK_UPDATE: u8 = 8;
    pub const CHANGE_DIFFICULTY: u8 = 10;
    pub const CHUNK_BATCH_FINISHED: u8 = 11;
    pub const CHUNK_BATCH_START: u8 = 12;
    pub const COMMANDS: u8 = 16;
    pub const DISCONNECT: u8 = 32;
    pub const ENTITY_EVENT: u8 = 34;
    pub const FORGET_LEVEL_CHUNK: u8 = 38;
    pub const GAME_EVENT: u8 = 39;
    pub const INITIALIZE_BORDER: u8 = 44;
    pub const KEEP_ALIVE: u8 = 45;
    pub const LEVEL_EVENT: u8 = 43;
    pub const LEVEL_CHUNK_WITH_LIGHT: u8 = 46;
    pub const LIGHT_UPDATE: u8 = 49;
    pub const LOGIN: u8 = 50;
    pub const PLAYER_ABILITIES: u8 = 65;
    pub const PLAYER_INFO_UPDATE: u8 = 71;
    pub const PLAYER_POSITION: u8 = 73;
    pub const RESPAWN: u8 = 84;
    pub const SECTION_BLOCKS_UPDATE: u8 = 82;
    pub const SET_CHUNK_CACHE_CENTER: u8 = 96;
    pub const SET_CHUNK_CACHE_RADIUS: u8 = 97;
    pub const SET_DEFAULT_SPAWN_POSITION: u8 = 99;
    pub const SET_HELD_SLOT: u8 = 107;
    pub const SET_SIMULATION_DISTANCE: u8 = 113;
    pub const SET_TIME: u8 = 115;
    pub const SYSTEM_CHAT: u8 = 124;
    pub const TICKING_STATE: u8 = 130;
    pub const TICKING_STEP: u8 = 131;
}

/// Serverbound ids this module decodes.
pub mod sb {
    pub const ACCEPT_TELEPORTATION: i32 = 0;
    pub const CHUNK_BATCH_RECEIVED: i32 = 11;
    pub const CLIENT_TICK_END: i32 = 13;
    pub const CLIENT_INFORMATION: i32 = 14;
    pub const KEEP_ALIVE: i32 = 28;
    pub const MOVE_PLAYER_POS: i32 = 30;
    pub const MOVE_PLAYER_POS_ROT: i32 = 31;
    pub const MOVE_PLAYER_ROT: i32 = 32;
    pub const MOVE_PLAYER_STATUS_ONLY: i32 = 33;
    pub const PLAYER_LOADED: i32 = 44;
}
