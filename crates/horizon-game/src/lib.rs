//! The game without a window: the rules and state a server runs, shared by the client
//! (`horizon-client`) and the dedicated server. No wgpu, winit, audio or controllers here
//! (docs/crates.md). `horizon_client` re-exports every module under its old path.
#![forbid(unsafe_code)]
pub mod advancements;
pub mod animate_tick;
pub mod arrows;
pub mod assets;
pub mod attack_strength;
pub mod attribute_sync;
pub mod attribute_updates;
pub mod auth;
pub mod auto_jump;
pub mod banner;
pub mod beehive;
pub mod biome;
pub mod block_entities;
pub mod block_entity_anim;
pub mod block_entity_item;
pub mod block_transform;
pub mod brewing;
pub mod camera;
pub mod chunk;
pub mod chunk_map;
pub mod chunk_status;
pub mod clock;
pub mod collisions;
pub mod command_block;
pub mod containers;
pub mod control;
pub mod crafting;
pub mod crash;
pub mod creative_menu;
pub mod data_cmd;
pub mod dialog;
pub mod effects;
pub mod end_crystal;
pub mod entities;
pub mod entity_nbt;
pub mod entity_tracking;
pub mod environment;
pub mod face_visibility;
pub mod falling_block;
pub mod fast_hash;
pub mod first_person;
pub mod furnace;
pub mod gallery;
pub mod gossip;
pub mod health;
pub mod hotbar;
pub mod hunger;
pub mod image;
pub mod input;
pub mod inv_sort;
pub mod item_sync;
pub mod java_text;
pub mod landing;
pub mod light;
pub mod lod_column;
pub mod logging;
pub mod menu;
pub mod merchant;
pub mod minecomplex;
pub mod mobs;
pub mod motion;
pub mod offline_name;
pub mod overlay;
pub mod par;
pub mod particles;
pub mod poi_sections;
pub mod portal_sections;
pub mod pose;
pub mod projectiles;
pub mod providers;
pub mod random_tick_sections;
pub mod recipe_book;
pub mod recipe_book_screen;
pub mod save;
pub mod screen_layout;
pub mod sculk;
pub mod signs;
pub mod skins;
pub mod sound_out;
pub mod spans;
pub mod spear;
pub mod sprint;
pub mod stations;
pub mod streaming;
pub mod structure_blocks;
pub mod swing;
pub mod testgen;
pub mod text_display;
pub mod ticket_levels;
pub mod vault;
pub mod vertex;
pub mod wire;
pub mod world;
pub mod world_data;
pub mod world_options;
pub mod worldborder;
pub mod zip;

pub use horizon_client_core::Result;
pub use horizon_client_core::{
    block_entity_nbt, bossbar, component, fastutil_order, gamerules, lang, nbt_path, scoreboard,
    snbt, text,
};
pub use horizon_command as command;
pub use horizon_items::{
    banner_layer, blocks, bundle, content_namespaces, cooldowns, enchant, firework, item_data,
    item_nbt, items, random_sequences, use_animation,
};
