//! The game without a window: the rules and state a server runs, shared by the client
//! (`horizon-client`) and the dedicated server. No wgpu, winit, audio or controllers here
//! (docs/crates.md). `horizon_client` re-exports every module under its old path.
#![forbid(unsafe_code)]
pub mod advancements;
pub mod arrows;
pub mod assets;
pub mod attack_strength;
pub mod attribute_updates;
pub mod auth;
pub mod auto_jump;
pub mod banner;
pub mod beehive;
pub mod biome;
pub mod block_entities;
pub mod block_entity_item;
pub mod block_transform;
pub mod brewing;
pub mod cauldron;
pub mod chunk;
pub mod chunk_map;
pub mod chunk_status;
pub mod clock;
pub mod collisions;
pub mod command_block;
pub mod command_storage;
pub mod containers;
pub mod crafting;
pub mod creative_menu;
pub mod cushion;
pub mod data_cmd;
pub mod datapack;
pub mod dialog;
pub mod effects;
pub mod end_crystal;
pub mod entities;
pub mod entity_nbt;
pub mod environment;
pub mod face_visibility;
pub mod falling_block;
pub mod fast_hash;
pub mod flatgen;
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
pub mod lan;
pub mod landing;
pub mod light;
pub mod lod_column;
pub mod logging;
pub mod menu;
pub mod merchant;
pub mod minecomplex;
pub mod mobs;
pub mod mod_content;
pub mod motion;
pub mod nether_portal;
pub mod offline_name;
pub mod overlay;
pub mod par;
pub mod particles;
pub mod patrol;
pub mod pick_item;
pub mod poi_sections;
pub mod portal_sections;
pub mod pose;
pub mod pregen;
pub mod projectiles;
pub mod raid;
pub mod random_tick_sections;
pub mod recipe_book;
pub mod save;
pub mod scoreboard_cmd;
pub mod sculk;
pub mod signs;
pub mod skins;
pub mod sound_out;
pub mod spans;
pub mod spear;
pub mod sprint;
pub mod stations;
pub mod stopwatches;
pub mod streaming;
pub mod structure_blocks;
pub mod swing;
pub mod testgen;
pub mod text_display;
pub mod tick_chunk_membership;
pub mod ticket_levels;
pub mod vanillagen;
pub mod vault;
pub mod vertex;
pub mod world;
pub mod world_data;
pub mod world_spawn;
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
pub use horizon_server_files as server;
