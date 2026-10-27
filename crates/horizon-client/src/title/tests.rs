use super::*;
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture {
    root: PathBuf,
    title: Title,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "horizon-title-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("saves")).unwrap();
        Self {
            title: Title::new(root.join("saves"), &root),
            root,
        }
    }
    fn click(&mut self, label: &str) -> Action {
        let button = self
            .title
            .buttons(480.0, 262.0)
            .into_iter()
            .find(|b| b.label == label)
            .unwrap();
        self.title
            .click(button.x + button.w / 2.0, button.y + 10.0, (480.0, 262.0))
            .0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_title_has_no_single_player_realms_or_friends() {
    let f = Fixture::new();
    let labels: Vec<String> = f
        .title
        .buttons(480.0, 262.0)
        .into_iter()
        .map(|b| b.label)
        .collect();
    assert!(labels.contains(&"Multiplayer".to_owned()));
    for gone in ["Singleplayer", "Minecraft Realms", "Friends"] {
        assert!(!labels.contains(&gone.to_owned()), "{gone} in {labels:?}");
    }
}

#[test]
fn direct_connection_remembers_the_address_on_cancel_escape_and_join() {
    let mut f = Fixture::new();
    f.title.last_server = "localhost:25566".into();
    f.click("Multiplayer");
    f.click("Direct Connection");
    assert_eq!(f.title.focused_field().unwrap().1, "localhost:25566");

    *f.title.focused_field().unwrap().1 = "example.org:25567".into();
    f.click("Cancel");
    f.click("Direct Connection");
    assert_eq!(f.title.focused_field().unwrap().1, "example.org:25567");

    *f.title.focused_field().unwrap().1 = "[::1]:25568".into();
    f.title.escape();
    f.title.escape();
    f.click("Multiplayer");
    f.click("Direct Connection");
    assert_eq!(f.title.focused_field().unwrap().1, "[::1]:25568");

    *f.title.focused_field().unwrap().1 = "localhost:25569".into();
    assert_eq!(
        f.click("Join Server"),
        Action::Connect {
            address: "localhost:25569".into(),
        }
    );
    assert_eq!(f.title.last_server, "localhost:25569");

    f.title.focused_field().unwrap().1.clear();
    f.title.escape();
    f.click("Direct Connection");
    assert!(f.title.focused_field().unwrap().1.is_empty());
}

#[test]
fn server_fields_edit_at_their_cursor() {
    use winit::keyboard::KeyCode;
    let mut f = Fixture::new();
    f.title.mode = Mode::Multiplayer(crate::multiplayer::Screen::Edit {
        index: None,
        name: "Minecraft Server".into(),
        address: String::new(),
        focus: 1,
        packs: None,
    });
    f.title.type_text("localhost");
    for _ in 0..4 {
        assert!(f.title.field_key(KeyCode::ArrowLeft, false, false));
    }
    f.title.type_text("X");
    let Mode::Multiplayer(crate::multiplayer::Screen::Edit { address, .. }) = &f.title.mode else {
        panic!("add server");
    };
    assert_eq!(address, "localXhost");
    // Shift+Home selects back to the start; typing replaces the selection.
    f.title.field_key(KeyCode::Home, false, true);
    f.title.type_text("Y");
    let Mode::Multiplayer(crate::multiplayer::Screen::Edit { address, .. }) = &f.title.mode else {
        panic!("add server");
    };
    assert_eq!(address, "Yhost");
    // Tab moves to the name, which edits at its own cursor.
    f.title.tab();
    f.title.field_key(KeyCode::ArrowLeft, false, false);
    f.title.type_text("X");
    let Mode::Multiplayer(crate::multiplayer::Screen::Edit { name, .. }) = &f.title.mode else {
        panic!("add server");
    };
    assert_eq!(name, "Minecraft ServeXr");
    f.title.mode = Mode::Multiplayer(crate::multiplayer::Screen::Direct {
        address: String::new(),
    });
    f.title.type_text("one two");
    // Ctrl+Backspace deletes the word before the caret.
    f.title.field_key(KeyCode::Backspace, true, false);
    let Mode::Multiplayer(crate::multiplayer::Screen::Direct { address }) = &f.title.mode else {
        panic!("direct");
    };
    assert_eq!(address, "one ");
}
