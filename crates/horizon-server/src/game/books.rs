//! Books (`BookEditScreen`, `BookSignScreen`, `BookViewScreen`,
//! `LecternScreen`): writing pages of a book and quill (14 lines of 114
//! pixels, 1024 characters, 100 pages), signing it into a written book,
//! reading written books, and lecterns (placing a book, turning its pages
//! with a redstone pulse, the comparator output and taking the book).
use super::server_net::Network;
use crate::hc::{
    containers::{BlockEntity, Lectern},
    first_person::Hand,
    item_data::WrittenBook,
    items::ItemStack,
    menu::{Kind, Menu},
};
use horizon_core::position::BlockPos;
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_world::level::flags as level_flags;

/// A book a player's turn opened: on a server only a lectern's
/// (`LecternBlock.openScreen`), which the server then opens on the player's
/// client (game/net_actions.rs, `net_open_lectern`).
pub(super) enum BookScreen {
    View { lectern: Option<(i32, i32, i32)> },
}
impl crate::game::Sim<'_> {
    /// The book and quill at `slot` takes the pages, or with a title becomes
    /// the written book its writer signed.
    fn write_book(&mut self, slot: usize, pages: Vec<String>, title: Option<String>) {
        let author = self
            .net_acting_name
            .clone()
            .unwrap_or_else(|| super::chat::PLAYER_NAME.to_owned());
        let Some(stack) = self
            .stack_in_mut(slot)
            .as_mut()
            .filter(|s| &*s.item == "minecraft:writable_book")
        else {
            return;
        };
        if let Some(title) = title {
            let mut written = ItemStack::new("minecraft:written_book", 1);
            written.data_mut().written = Some(WrittenBook {
                title,
                author,
                generation: 0,
                pages,
                page_tags: Vec::new(),
            });
            *stack = written;
        } else {
            stack.data_mut().writable_pages = if pages.iter().all(String::is_empty) {
                Vec::new()
            } else {
                pages
            };
            stack.tidy();
        }
    }
    /// `ServerGamePacketListenerImpl.handleEditBook`: a player's book in a
    /// hotbar slot or its off hand (`updateBookContents`, `signBook`).
    pub(super) fn net_edit_book(&mut self, net: &mut Network, conn: u32, p: play::EditBook) {
        if !((0..9).contains(&p.slot) || p.slot == super::shield::OFFHAND_SLOT as i32) {
            return;
        }
        self.as_player(net, conn, |g| {
            g.write_book(p.slot as usize, p.pages, p.title)
        });
    }
    fn set_lectern_powered(&mut self, key: (i32, i32, i32), powered: bool) {
        self.set_lectern_property(key, "powered", powered);
        let p = BlockPos::new(key.0, key.1, key.2);
        let state = self.world.block(key.0, key.1, key.2);
        self.with_level(|b, level, ticks| {
            let block = b.block(state);
            b.update_neighbors_at(level, ticks, BlockPos::new(p.x, p.y - 1, p.z), block);
        });
    }
    fn set_lectern_property(&mut self, key: (i32, i32, i32), name: &str, value: bool) {
        let state = self.world.block(key.0, key.1, key.2);
        let registry = self.pack.assets.registry;
        let Some(mut s) = registry.state(state).cloned() else {
            return;
        };
        if !s.properties.contains_key(name) {
            return;
        }
        s.properties.insert(name.into(), value.to_string());
        if let Some(new) = registry.id(&s) {
            let p = BlockPos::new(key.0, key.1, key.2);
            self.with_level(|b, level, ticks| {
                b.set_block(level, ticks, p, new, level_flags::UPDATE_ALL)
            });
        }
    }
    /// The 2-tick pulses of turned lecterns.
    pub(super) fn tick_lecterns(&mut self) {
        let mut done = Vec::new();
        for (key, left) in &mut self.lectern_pulses {
            *left -= 1;
            if *left <= 0 {
                done.push(*key);
            }
        }
        self.lectern_pulses.retain(|(_, left)| *left > 0);
        for key in done {
            self.set_lectern_powered(key, false);
        }
    }
    /// `LecternBlock.useItemOn`: a book goes onto an empty lectern.
    pub(super) fn use_lectern(&mut self, pos: [i32; 3], held: Option<&ItemStack>) -> bool {
        let key = (pos[0], pos[1], pos[2]);
        if &*self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name
            != "minecraft:lectern"
        {
            return false;
        }
        let has_book = matches!(self.world.block_entities.get(&key), Some(BlockEntity::Lectern(l)) if l.book.is_some());
        let Some(book) = held.filter(|h| {
            matches!(
                &*h.item,
                "minecraft:writable_book" | "minecraft:written_book"
            )
        }) else {
            return false;
        };
        if has_book {
            return false;
        }
        let mut one = book.clone();
        one.count = 1;
        self.world.block_entities.insert(
            key,
            BlockEntity::Lectern(Lectern {
                book: Some(one),
                page: 0,
            }),
        );
        self.set_lectern_property(key, "has_book", true);
        if !self.creative {
            self.consume_held();
        }
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        self.sounds
            .play("minecraft:item.book.put", Some(at), 1.0, 1.0);
        self.container_changed(key);
        true
    }
    /// `LecternMenu` over the lectern at `key` (the menu engine, for the
    /// local player and players alike): its book and page.
    pub(super) fn lectern_menu(&self, key: (i32, i32, i32)) -> Option<Menu> {
        let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get(&key) else {
            return None;
        };
        let mut menu = Menu::new(Kind::Lectern);
        menu.set_slot(&self.menu_env(), 0, l.book.clone());
        menu.data[0] = l.page;
        Some(menu)
    }
    /// The lectern menu's book and page back in the block entity: a new page
    /// is `LecternBlock.signalPageChange` (the 2-tick pulse, level event
    /// 1043), a book gone `onBookItemRemove` (`resetBookState`).
    pub(super) fn lectern_menu_done(&mut self, key: (i32, i32, i32), menu: &Menu) {
        let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get_mut(&key) else {
            return;
        };
        let (had, before) = (l.book.is_some(), l.page);
        l.book = menu.slot(0).cloned();
        l.page = menu.data[0];
        if had && l.book.is_none() {
            self.set_lectern_powered(key, false);
            self.set_lectern_property(key, "has_book", false);
            self.container_changed(key);
        } else if menu.data[0] != before {
            self.set_lectern_powered(key, true);
            self.lectern_pulses.push((key, 2));
            self.level_event_at(1043, [key.0, key.1, key.2], 0);
            self.container_changed(key);
        }
    }
}
/// `LecternBlockEntity.getRedstoneSignal`.
pub(super) fn lectern_signal(world: &crate::hc::world::World, pos: BlockPos) -> Option<i32> {
    match world.block_entities.get(&(pos.x, pos.y, pos.z)) {
        Some(BlockEntity::Lectern(l)) => Some(l.book.as_ref().map_or(0, |b| {
            let data = b.data();
            let count = data
                .and_then(|d| d.written.as_ref().map(|w| w.pages.len()))
                .or_else(|| data.map(|d| d.writable_pages.len()))
                .unwrap_or(0)
                .max(1);
            let f = if count > 1 {
                l.page as f32 / (count - 1) as f32
            } else {
                1.0
            };
            (f * 14.0).floor() as i32 + 1
        })),
        _ => None,
    }
}

impl crate::game::Sim<'_> {
    /// `WrittenBookItem.use`: a written book opens on its reader's client
    /// (`ServerPlayer.openItemGui`); a book and quill opens its editor there by itself.
    pub(super) fn use_book(&mut self, stack: &ItemStack) -> bool {
        let written = match &*stack.item {
            "minecraft:writable_book" => false,
            "minecraft:written_book" => true,
            _ => return false,
        };
        if self.net_acting.is_some() && written {
            let hand = i32::from(self.hand == Hand::Off);
            self.player_packets
                .push(C::OpenBook(play::OpenBook { hand }));
        }
        self.swing_arm();
        true
    }
    /// `LecternBlock.openScreen`: the reader's turn takes the lectern's book.
    pub(super) fn open_lectern(&mut self, pos: [i32; 3]) -> bool {
        let key = (pos[0], pos[1], pos[2]);
        let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get(&key) else {
            return false;
        };
        if l.book.is_none() {
            return false;
        }
        self.stats.custom("interact_with_lectern", 1);
        self.book = Some(BookScreen::View { lectern: Some(key) });
        true
    }
}
