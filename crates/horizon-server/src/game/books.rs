//! Books (`BookEditScreen`, `BookSignScreen`, `BookViewScreen`,
//! `LecternScreen`): writing pages of a book and quill (14 lines of 114
//! pixels, 1024 characters, 100 pages), signing it into a written book,
//! reading written books, and lecterns (placing a book, turning its pages
//! with a redstone pulse, the comparator output and taking the book).
use super::{lan::LanHost};
use crate::hc::{
    containers::{BlockEntity, Lectern},
    first_person::Hand,
    hud::{HudBatch, rgba},
    item_data::{StackData, WrittenBook},
    items::ItemStack,
    menu::{Kind, Menu},
    text::Text,
    ui,
};
use horizon_core::position::BlockPos;
use horizon_protocol::packets::play::{self, Clientbound as C, Serverbound as S};
use horizon_world::level::flags as level_flags;
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

const BLACK: u32 = 0xFF00_0000;
const PAGE_CHARS: usize = 1024;
const MAX_PAGES: usize = 100;
const LINES: usize = 14;

fn book_title_box() -> ui::EditBox {
    let mut edit = ui::EditBox::new(0.0, 0.0, 114.0, 9.0);
    edit.bordered = false;
    edit.max_length = 15;
    edit.text_color = BLACK;
    edit.focused = true;
    edit
}
fn book_page_box() -> ui::EditBox {
    let mut edit = ui::EditBox::new(0.0, 0.0, 114.0, 9.0);
    edit.bordered = false;
    edit.multiline = true;
    edit.max_length = PAGE_CHARS;
    edit.text_color = BLACK;
    edit.focused = true;
    edit
}

pub(super) enum BookScreen {
    Edit {
        /// Inventory slot of the book (the selected hotbar slot).
        slot: usize,
        pages: Vec<String>,
        page: usize,
        signing: bool,
        title: String,
        /// BookSignScreen's title box (`EditBox` of 15).
        title_edit: ui::EditBox,
        /// BookEditScreen's page (`MultiLineEditBox`).
        page_edit: ui::EditBox,
    },
    View {
        pages: Vec<Text>,
        page: usize,
        lectern: Option<(i32, i32, i32)>,
    },
}
/// The pages a book screen reads: a written book's components (so their
/// click events stay), else a writable book's text.
fn view_pages(data: Option<&StackData>) -> Vec<Text> {
    let Some(data) = data else { return Vec::new() };
    let Some(w) = &data.written else {
        return data.writable_pages.iter().map(Text::literal).collect();
    };
    let tagged = w.page_tags.len() == w.pages.len();
    w.pages
        .iter()
        .enumerate()
        .map(|(i, page)| match tagged.then(|| &w.page_tags[i]) {
            // A component object, or a component list (`ComponentSerialization`
            // takes both; servers behind ViaVersion send the list form).
            Some(
                tag @ (horizon_world::nbt::Tag::Compound(_) | horizon_world::nbt::Tag::List(_)),
            ) => crate::hc::entity_nbt::name_text(tag),
            _ => Text::literal(page),
        })
        .collect()
}
impl crate::game::Sim<'_> {
    /// `WritableBookItem.use` / `WrittenBookItem.use` (`Player.openItemGui`).
    /// A server's player writes in its own client's editor
    /// (`LocalPlayer.openItemGui`), and reads on its own client once told to
    /// (`ServerPlayer.openItemGui`'s ClientboundOpenBookPacket).
    pub(super) fn use_book(&mut self, stack: &ItemStack) -> bool {
        let written = match &*stack.item {
            "minecraft:writable_book" => false,
            "minecraft:written_book" => true,
            _ => return false,
        };
        if self.lan_acting.is_some() {
            if written {
                let hand = i32::from(self.hand == Hand::Off);
                self.guest_packets
                    .push(C::OpenBook(play::OpenBook { hand }));
            }
        } else if let Some(screen) = self.book_screen(stack) {
            self.open_book(screen);
        }
        self.swing_arm();
        true
    }
    /// The screen a book in hand opens: `BookEditScreen` for a book and
    /// quill, `BookViewScreen` for a written book.
    fn book_screen(&self, stack: &ItemStack) -> Option<BookScreen> {
        Some(match &*stack.item {
            "minecraft:writable_book" => {
                let mut pages = stack
                    .data()
                    .map(|d| d.writable_pages.clone())
                    .unwrap_or_default();
                if pages.is_empty() {
                    pages.push(String::new());
                }
                let mut page_edit = book_page_box();
                page_edit.set_value(&pages[0]);
                BookScreen::Edit {
                    slot: self.held_slot(),
                    pages,
                    page: 0,
                    signing: false,
                    title: String::new(),
                    title_edit: book_title_box(),
                    page_edit,
                }
            }
            "minecraft:written_book" => BookScreen::View {
                pages: view_pages(stack.data()),
                page: 0,
                lectern: None,
            },
            _ => return None,
        })
    }
    /// `MultiPlayerGameMode.useItem`'s own use of a book and quill on a
    /// server's client: its editor opens here at once.
    pub(super) fn remote_use_book(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:writable_book" {
            return false;
        }
        if let Some(screen) = self.book_screen(stack) {
            self.open_book(screen);
        }
        true
    }
    /// `ClientPacketListener.handleOpenBook`: the written book in that hand.
    pub(super) fn remote_open_book(&mut self, hand: i32) {
        let hand = if hand == 1 { Hand::Off } else { Hand::Main };
        let screen = self.with_hand(hand, |g| {
            let stack = g.held().filter(|s| &*s.item == "minecraft:written_book")?;
            g.book_screen(&stack.clone())
        });
        if let Some(screen) = screen {
            self.open_book(screen);
        }
    }
    fn open_book(&mut self, screen: BookScreen) {
        self.book = Some(screen);
        self.keys.clear();
        self.right = false;
        self.capture(false);
    }
    pub(super) fn close_book(&mut self) {
        // A server's lectern: LecternScreen's menu closes (closeContainer).
        if matches!(
            self.book,
            Some(BookScreen::View {
                lectern: Some(_),
                ..
            })
        ) && self.remote.is_some()
        {
            self.remote_close_container();
        }
        self.book = None;
        self.capture(true);
    }
    /// `BookEditScreen.saveChanges` and `BookSignScreen`'s Sign and Close:
    /// trailing empty pages dropped, the book in hand updated here and, on a
    /// server's client, sent (ServerboundEditBookPacket).
    fn save_book(&mut self, sign: bool) {
        let Some(BookScreen::Edit {
            slot, pages, title, ..
        }) = self.book.take()
        else {
            return;
        };
        let mut pages = pages;
        while pages.len() > 1 && pages.last().is_some_and(String::is_empty) {
            pages.pop();
        }
        let title = sign.then(|| title.trim().to_owned());
        if self.remote.is_some() {
            self.send(S::EditBook(play::EditBook {
                slot: slot as i32,
                pages: pages.clone(),
                title: title.clone(),
            }));
        }
        self.write_book(slot, pages, title);
        self.capture(true);
    }
    /// The book and quill at `slot` takes the pages, or with a title becomes
    /// the written book its writer signed.
    fn write_book(&mut self, slot: usize, pages: Vec<String>, title: Option<String>) {
        let author = self
            .lan_acting_name
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
    /// `ServerGamePacketListenerImpl.handleEditBook`: a guest's book in a
    /// hotbar slot or its off hand (`updateBookContents`, `signBook`).
    pub(super) fn lan_edit_book(&mut self, host: &mut LanHost, conn: u32, p: play::EditBook) {
        if !((0..9).contains(&p.slot) || p.slot == super::shield::OFFHAND_SLOT as i32) {
            return;
        }
        self.as_guest(host, conn, |g| {
            g.write_book(p.slot as usize, p.pages, p.title)
        });
    }
    /// `BookSignScreen.keyPressed` on the title box.
    fn book_title_key(&mut self, key: KeyCode) -> bool {
        let (ctrl, shift) = (self.has_control_down(), self.has_shift_down());
        let Some(BookScreen::Edit {
            title, title_edit, ..
        }) = self.book.as_mut()
        else {
            return false;
        };
        if !title_edit.key(key, ctrl, shift) {
            return false;
        }
        title.clone_from(&title_edit.value);
        true
    }
    fn book_title_insert(&mut self, text: &str) {
        let Some(BookScreen::Edit {
            title, title_edit, ..
        }) = self.book.as_mut()
        else {
            return;
        };
        title_edit.insert_text(text);
        title.clone_from(&title_edit.value);
    }
    /// The page buttons (`pageForward` adds a page while editing).
    fn turn_page(&mut self, forward: bool) {
        let mut lectern_page = None;
        match self.book.as_mut() {
            Some(BookScreen::Edit {
                pages,
                page,
                page_edit,
                ..
            }) => {
                let before = *page;
                if forward {
                    if *page + 1 >= pages.len() && pages.len() < MAX_PAGES {
                        pages.push(String::new());
                    }
                    if *page + 1 < pages.len() {
                        *page += 1;
                    }
                } else if *page > 0 {
                    *page -= 1;
                }
                if *page != before {
                    let text = pages[*page].clone();
                    page_edit.set_value(&text);
                }
            }
            Some(BookScreen::View {
                lectern: Some(key), ..
            }) => lectern_page = Some(*key),
            Some(BookScreen::View { pages, page, .. }) => {
                if forward && *page + 1 < pages.len() {
                    *page += 1;
                } else if !forward && *page > 0 {
                    *page -= 1;
                }
            }
            None => return,
        }
        // LecternScreen's page buttons click the menu's (1 back, 2 forward);
        // the page turns with the server's data slot and level event 1043.
        if let Some(key) = lectern_page {
            self.lectern_button(key, if forward { 2 } else { 1 });
            return;
        }
        self.sounds
            .play("minecraft:item.book.page_turn", None, 1.0, 1.0);
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
        self.follow_lectern();
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
    /// `LecternBlock.openScreen`: reading the lectern's book.
    pub(super) fn open_lectern(&mut self, pos: [i32; 3]) -> bool {
        let key = (pos[0], pos[1], pos[2]);
        let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get(&key) else {
            return false;
        };
        let Some(book) = &l.book else { return false };
        let pages = view_pages(book.data());
        let page = (l.page.max(0) as usize).min(pages.len().saturating_sub(1));
        self.stats.custom("interact_with_lectern", 1);
        self.open_book(BookScreen::View {
            pages,
            page,
            lectern: Some(key),
        });
        true
    }
    /// A `LecternMenu` button of the open lectern screen: on a server it is
    /// sent (`handleInventoryButtonClick`); here the lectern's menu takes it
    /// with the player's inventory, and the screen follows the block entity.
    fn lectern_button(&mut self, key: (i32, i32, i32), id: i32) {
        if self.remote.is_some() {
            if let Some((container, _)) = self.remote.as_ref().and_then(|r| r.menu.as_ref()) {
                self.send(
                    horizon_protocol::packets::play::Serverbound::ContainerButtonClick(
                        horizon_protocol::packets::play::ContainerButtonClick {
                            container: *container,
                            button: id,
                        },
                    ),
                );
            }
            return;
        }
        let Some(mut menu) = self.lectern_menu(key) else {
            return;
        };
        for i in 0..36 {
            menu.set_player_item(i, self.inventory[i].clone());
        }
        menu.set_player_item(40, self.offhand.clone());
        menu.creative = self.creative;
        menu.selected = self.selected;
        menu.may_build = self.may_build();
        if !menu.button(&self.menu_env(), id) {
            return;
        }
        for i in 0..36 {
            self.inventory[i] = menu.player_item(i).cloned();
        }
        self.offhand = menu.player_item(40).cloned();
        for stack in std::mem::take(&mut menu.dropped) {
            self.player_drop(stack);
        }
        self.lectern_menu_done(key, &menu);
        self.follow_lectern();
    }
    /// The `book` control command: a button of the open book or lectern
    /// screen (the page arrows, Take Book, Done), then its page.
    pub(super) fn book_control(
        &mut self,
        action: Option<&str>,
    ) -> crate::hc::Result<serde_json::Value> {
        let lectern = match &self.book {
            Some(BookScreen::View { lectern, .. }) => *lectern,
            Some(BookScreen::Edit { .. }) => None,
            None => return Err("no book is open".into()),
        };
        match action {
            None => {}
            Some("next") => self.turn_page(true),
            Some("back") => self.turn_page(false),
            Some("take") => match lectern {
                Some(key) => self.lectern_button(key, 3),
                None => return Err("not a lectern".into()),
            },
            // The book and quill's buttons: Sign, then Sign and Close; Done
            // (which saves), then Cancel.
            Some("sign") => match self.book.as_mut() {
                Some(BookScreen::Edit {
                    signing: true,
                    title,
                    ..
                }) if !title.trim().is_empty() => self.save_book(true),
                Some(BookScreen::Edit { signing, .. }) => *signing = true,
                _ => return Err("not a book and quill".into()),
            },
            Some("done") => match self.book.as_mut() {
                Some(BookScreen::Edit { signing: true, .. }) => {
                    if let Some(BookScreen::Edit { signing, .. }) = self.book.as_mut() {
                        *signing = false;
                    }
                }
                Some(BookScreen::Edit { .. }) => self.save_book(false),
                _ => self.close_book(),
            },
            Some(other) => return Err(format!("unknown book button {other}").into()),
        }
        Ok(match &self.book {
            Some(BookScreen::View {
                pages,
                page,
                lectern,
            }) => serde_json::json!({
                "page": page, "pages": pages.len(), "lectern": lectern.map(|k| [k.0, k.1, k.2]),
            }),
            Some(BookScreen::Edit { pages, page, .. }) => {
                serde_json::json!({ "page": page, "pages": pages.len(), "lectern": null })
            }
            None => serde_json::json!({ "closed": true }),
        })
    }
    /// A server's `minecraft:lectern` menu: LecternScreen, its book and
    /// page to come (`remote_lectern_book`, `remote_lectern_page`).
    pub(super) fn remote_open_lectern(&mut self, key: (i32, i32, i32)) {
        self.open_book(BookScreen::View {
            pages: Vec::new(),
            page: 0,
            lectern: Some(key),
        });
    }
    /// `LecternScreen.bookChanged`: the menu's book slot.
    pub(super) fn remote_lectern_book(&mut self, book: Option<ItemStack>) {
        if let Some(BookScreen::View {
            lectern: Some(_),
            pages,
            page,
        }) = self.book.as_mut()
        {
            *pages = view_pages(book.as_ref().and_then(ItemStack::data));
            *page = (*page).min(pages.len().saturating_sub(1));
        }
    }
    /// `LecternScreen.pageChanged`: the menu's page data slot.
    pub(super) fn remote_lectern_page(&mut self, value: i32) {
        if let Some(BookScreen::View {
            lectern: Some(_),
            pages,
            page,
        }) = self.book.as_mut()
        {
            *page = (value.max(0) as usize).min(pages.len().saturating_sub(1));
        }
    }
    /// The server closed its lectern menu; false when none is open.
    pub(super) fn remote_lectern_closed(&mut self) -> bool {
        if !matches!(
            self.book,
            Some(BookScreen::View {
                lectern: Some(_),
                ..
            })
        ) {
            return false;
        }
        if let Some(r) = self.remote.as_mut() {
            r.menu = None;
            r.menu_title = None;
        }
        self.book = None;
        self.capture(true);
        true
    }
    /// `LecternMenu` over the lectern at `key` (the menu engine, for the
    /// local player and LAN guests alike): its book and page.
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
    /// The open lectern screen follows its block entity (`LecternScreen`'s
    /// menu listener): the page, and closing once the book is gone.
    pub(super) fn follow_lectern(&mut self) {
        let Some(BookScreen::View {
            lectern: Some(key),
            page,
            pages,
        }) = self.book.as_mut()
        else {
            return;
        };
        if self.remote.is_some() {
            return;
        }
        match self.world.block_entities.get(key) {
            Some(BlockEntity::Lectern(l)) if l.book.is_some() => {
                *page = (l.page.max(0) as usize).min(pages.len().saturating_sub(1));
            }
            _ => self.close_book(),
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
