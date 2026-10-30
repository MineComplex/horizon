use std::fmt::{self, Display};

use crate::azalea::{
    chat::{
        FormattedText,
        base_component::BaseComponent,
        style::{ChatFormatting, Style, TextColor},
    },
    language::{self, Language},
};

/// A component that contains text that's the same in all locales.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextComponent {
    pub base: BaseComponent,
    pub text: String,
}
const LEGACY_FORMATTING_CODE_SYMBOL: char = '§';

/// Convert a legacy color code string (`§a...`) into styled siblings. Vanilla keeps `§` in literal
/// text and leaves it to the renderer; this is for console/MOTD-style strings only.
pub fn legacy_color_code_to_text_component(legacy_color_code: &str) -> TextComponent {
    if legacy_color_code.is_empty() {
        return TextComponent::new("");
    }

    let mut components: Vec<TextComponent> = Vec::with_capacity(1);
    // iterate over legacy_color_code, if it starts with LEGACY_COLOR_CODE_SYMBOL
    // then read the next character and get the style from that otherwise, add
    // the character to the text

    let mut cur_component = TextComponent::new("");

    // we don't use a normal for loop since we need to be able to skip after reading
    // the formatter code symbol
    let mut i = 0;
    while i < legacy_color_code.chars().count() {
        if legacy_color_code.chars().nth(i).unwrap() == LEGACY_FORMATTING_CODE_SYMBOL {
            let formatting_code = legacy_color_code.chars().nth(i + 1);
            let Some(formatting_code) = formatting_code else {
                i += 1;
                continue;
            };
            if formatting_code == '#' {
                let color = legacy_color_code
                    .chars()
                    .skip(i + 1)
                    // 7 to include the #
                    .take(7)
                    .collect::<String>();

                if !cur_component.text.is_empty() {
                    // we need to split this into a new component
                    components.push(cur_component.clone());
                    cur_component.text = "".to_owned();
                };
                cur_component.base.style.color = TextColor::parse(&color);

                i += 6;
            } else if let Some(formatter) = ChatFormatting::from_code(formatting_code) {
                if !cur_component.text.is_empty() || formatter == ChatFormatting::Reset {
                    // we need to split this into a new component
                    components.push(cur_component.clone());
                    cur_component.text = "".to_owned();
                };
                cur_component.base.style.apply_formatting(&formatter);
            }
            i += 1;
        } else {
            cur_component
                .text
                .push(legacy_color_code.chars().nth(i).unwrap());
        };
        i += 1;
    }

    components.push(cur_component);

    // create the final component by adding all of the components as siblings
    let mut final_component = TextComponent::new("");
    for component in components {
        final_component.base.siblings.push(component.get());
    }

    final_component
}

impl TextComponent {
    /// A literal (`Component.literal`): the text is kept as is, `§` included.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            base: BaseComponent::new(),
            text: text.into(),
        }
    }

    fn get(self) -> FormattedText {
        FormattedText::Text(self)
    }
    pub fn with_style(mut self, style: Style) -> Self {
        *self.base.style = style;
        self
    }
}

impl TextComponent {
    /// Appends the text without styling (this and its siblings), translated with `language`.
    pub(crate) fn write_plain(&self, out: &mut String, language: &Language) {
        out.push_str(&self.text);
        for sibling in &self.base.siblings {
            sibling.write_plain(out, language);
        }
    }
}

impl Display for TextComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.write_plain(&mut out, language::installed_or_empty());
        f.write_str(&out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::azalea::chat::style::Ansi;

    #[test]
    fn test_hypixel_motd_ansi() {
        let component = legacy_color_code_to_text_component(
            "§aHypixel Network  §c[1.8-1.18]\n§b§lHAPPY HOLIDAYS",
        )
        .get();
        assert_eq!(
            component.to_ansi(),
            format!(
                "{GREEN}Hypixel Network  {RED}[1.8-1.18]\n{BOLD}{AQUA}HAPPY HOLIDAYS{RESET}",
                GREEN = Ansi::rgb(ChatFormatting::Green.color().unwrap()),
                RED = Ansi::rgb(ChatFormatting::Red.color().unwrap()),
                AQUA = Ansi::rgb(ChatFormatting::Aqua.color().unwrap()),
                BOLD = Ansi::BOLD,
                RESET = Ansi::RESET
            )
        );
    }

    #[test]
    fn test_hypixel_motd_html() {
        let component = legacy_color_code_to_text_component(
            "§aHypixel Network  §c[1.8-1.18]\n§b§lHAPPY HOLIDAYS",
        )
        .get();

        assert_eq!(
            component.to_html(),
            format!(
                "{GREEN}Hypixel Network  {END_SPAN}{RED}[1.8-1.18]<br>{END_SPAN}{BOLD_AQUA}HAPPY HOLIDAYS{END_SPAN}",
                END_SPAN = "</span>",
                GREEN = "<span style=\"color:#55FF55;\">",
                RED = "<span style=\"color:#FF5555;\">",
                BOLD_AQUA = "<span style=\"color:#55FFFF;font-weight:bold;\">",
            )
        );
    }

    #[test]
    fn test_xss_html() {
        let component = legacy_color_code_to_text_component("§a<b>&\n§b</b>").get();

        assert_eq!(
            component.to_html(),
            format!(
                "{GREEN}&lt;b&gt;&amp;<br>{END_SPAN}{AQUA}&lt;/b&gt;{END_SPAN}",
                END_SPAN = "</span>",
                GREEN = "<span style=\"color:#55FF55;\">",
                AQUA = "<span style=\"color:#55FFFF;\">",
            )
        );
    }

    #[test]
    fn test_legacy_color_code_to_component() {
        let component = legacy_color_code_to_text_component("§lHello §r§1w§2o§3r§4l§5d").get();
        assert_eq!(
            component.to_ansi(),
            format!(
                "{BOLD}{WHITE}Hello {RESET}{DARK_BLUE}w{DARK_GREEN}o{DARK_AQUA}r{DARK_RED}l{DARK_PURPLE}d{RESET}",
                BOLD = Ansi::BOLD,
                WHITE = Ansi::rgb(ChatFormatting::White.color().unwrap()),
                RESET = Ansi::RESET,
                DARK_BLUE = Ansi::rgb(ChatFormatting::DarkBlue.color().unwrap()),
                DARK_GREEN = Ansi::rgb(ChatFormatting::DarkGreen.color().unwrap()),
                DARK_AQUA = Ansi::rgb(ChatFormatting::DarkAqua.color().unwrap()),
                DARK_RED = Ansi::rgb(ChatFormatting::DarkRed.color().unwrap()),
                DARK_PURPLE = Ansi::rgb(ChatFormatting::DarkPurple.color().unwrap())
            )
        );
    }

    #[test]
    fn test_legacy_color_code_with_rgb() {
        let component = legacy_color_code_to_text_component("§#Ff0000This is a test message").get();
        assert_eq!(
            component.to_ansi(),
            format!(
                "{RED}This is a test message{RESET}",
                RED = Ansi::rgb(0xff0000),
                RESET = Ansi::RESET
            )
        );
    }

    #[test]
    fn test_serialize_to_json() {
        let component = legacy_color_code_to_text_component("Hello §aworld").get();
        assert_eq!(
            serde_json::to_string(&component).unwrap(),
            "{\"text\":\"\",\"extra\":[\"Hello \",{\"text\":\"world\",\"color\":\"#55FF55\"}]}"
        );
    }
}
