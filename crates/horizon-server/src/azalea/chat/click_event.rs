use crate::nbt::Tag;

/// What clicking a component does (`ClickEvent`, 26.3 shapes). `open_file` is left out: vanilla
/// refuses it in both directions (`Action.filterForSerialization`).
#[derive(Clone, Debug, PartialEq)]
pub enum ClickEvent {
    /// An `http`/`https` URL.
    OpenUrl {
        url: String,
    },
    RunCommand {
        command: String,
    },
    SuggestCommand {
        command: String,
    },
    /// `Holder<Dialog>`: a dialog id or an inline dialog, kept as its NBT form until dialogs are
    /// ported.
    ShowDialog {
        dialog: Tag,
    },
    /// Positive.
    ChangePage {
        page: i32,
    },
    CopyToClipboard {
        value: String,
    },
    Custom {
        id: String,
        payload: Option<Tag>,
    },
}

impl ClickEvent {
    pub fn action(&self) -> &'static str {
        match self {
            Self::OpenUrl { .. } => "open_url",
            Self::RunCommand { .. } => "run_command",
            Self::SuggestCommand { .. } => "suggest_command",
            Self::ShowDialog { .. } => "show_dialog",
            Self::ChangePage { .. } => "change_page",
            Self::CopyToClipboard { .. } => "copy_to_clipboard",
            Self::Custom { .. } => "custom",
        }
    }
}
