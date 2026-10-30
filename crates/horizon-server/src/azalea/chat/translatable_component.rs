use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};
#[cfg(feature = "simdnbt")]
use simdnbt::{
    ToNbtTag,
    owned::{NbtList, NbtTag},
};

use crate::{
    azalea::{
        chat::{FormattedText, base_component::BaseComponent, text_component::TextComponent},
        language::{self, Language},
    },
    util::number_text::{double_to_string, float_to_string},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PrimitiveOrComponent {
    Boolean(bool),
    Short(i16),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    FormattedText(FormattedText),
}

#[cfg(feature = "simdnbt")]
impl simdnbt::ToNbtTag for PrimitiveOrComponent {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        match self {
            PrimitiveOrComponent::Boolean(value) => value.to_nbt_tag(),
            PrimitiveOrComponent::Short(value) => value.to_nbt_tag(),
            PrimitiveOrComponent::Integer(value) => value.to_nbt_tag(),
            PrimitiveOrComponent::Long(value) => value.to_nbt_tag(),
            PrimitiveOrComponent::Float(value) => value.to_nbt_tag(),
            PrimitiveOrComponent::Double(value) => value.to_nbt_tag(),
            PrimitiveOrComponent::String(value) => value.to_nbt_tag(),
            PrimitiveOrComponent::FormattedText(value) => value.to_nbt_tag(),
        }
    }
}

/// A message whose content depends on the client's language.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TranslatableComponent {
    #[serde(flatten)]
    pub base: BaseComponent,
    #[serde(rename = "translate")]
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    #[serde(rename = "with")]
    pub args: Vec<PrimitiveOrComponent>,
}

#[cfg(feature = "simdnbt")]
fn serialize_args_as_nbt(args: Vec<PrimitiveOrComponent>) -> NbtList {
    let tags = args
        .into_iter()
        .map(|arg| arg.to_nbt_tag())
        .collect::<Vec<NbtTag>>();
    NbtList::from(tags)
}

#[cfg(feature = "simdnbt")]
impl simdnbt::Serialize for TranslatableComponent {
    fn to_compound(self) -> simdnbt::owned::NbtCompound {
        let mut compound = simdnbt::owned::NbtCompound::new();
        compound.insert("translate", self.key);
        compound.extend(self.base.style.to_compound());

        compound.insert("with", serialize_args_as_nbt(self.args));
        compound
    }
}

impl TranslatableComponent {
    pub fn new(key: String, args: Vec<PrimitiveOrComponent>) -> Self {
        Self {
            base: BaseComponent::new(),
            key,
            fallback: None,
            args,
        }
    }

    pub fn with_fallback(
        key: String,
        fallback: Option<String>,
        args: Vec<PrimitiveOrComponent>,
    ) -> Self {
        Self {
            base: BaseComponent::new(),
            key,
            fallback,
            args,
        }
    }

    /// Convert the key and args to a FormattedText, with the installed language (an empty one when
    /// none is installed).
    pub fn read(&self) -> Result<TextComponent, fmt::Error> {
        self.read_in(language::installed_or_empty())
    }

    /// Convert the key and args to a FormattedText, looking up this key and those of translatable
    /// args in `language`.
    pub fn read_in(&self, language: &Language) -> Result<TextComponent, fmt::Error> {
        let template = language
            .get(&self.key)
            .unwrap_or_else(|| self.fallback.as_deref().unwrap_or(&self.key));
        // decode the % things

        let mut matched = 0;

        // every time we get a char we add it to built_text, and we push it to
        // `arguments` and clear it when we add a new argument component
        let mut built_text = String::new();
        let mut components = Vec::new();

        let mut chars = template.chars();
        while let Some(char) = chars.next() {
            if char != '%' {
                built_text.push(char);
                continue;
            }

            let mut chars_preview = chars.clone();
            let Some(char_after) = chars_preview.next() else {
                built_text.push('%');
                break;
            };
            match char_after {
                '%' => {
                    chars.next();

                    built_text.push('%');
                }
                's' => {
                    chars.next();

                    let arg_component = self
                        .args
                        .get(matched)
                        .cloned()
                        .unwrap_or_else(|| PrimitiveOrComponent::String("".to_owned()));

                    components.push(TextComponent::new(built_text.clone()));
                    built_text.clear();
                    components.push(TextComponent::new(arg_component.text_in(language)));
                    matched += 1;
                }
                '0'..='9' if let Some(d) = char_after.to_digit(10) => {
                    chars.next();
                    // make sure the next two chars are $s
                    let Some('$') = chars.next() else {
                        return Err(fmt::Error);
                    };
                    let Some('s') = chars.next() else {
                        return Err(fmt::Error);
                    };
                    if let Some(arg) = self.args.get((d as usize).wrapping_sub(1)) {
                        built_text.push_str(&arg.text_in(language));
                    }
                }
                _ => {
                    built_text.push('%');
                }
            }
        }

        if components.is_empty() {
            return Ok(TextComponent::new(built_text));
        }

        components.push(TextComponent::new(built_text));

        Ok(TextComponent {
            base: BaseComponent {
                siblings: components.into_iter().map(FormattedText::Text).collect(),
                style: Default::default(),
            },
            text: "".to_owned(),
        })
    }
}

impl TranslatableComponent {
    /// Appends the text without styling (this and its siblings), translated with `language`.
    pub(crate) fn write_plain(&self, out: &mut String, language: &Language) {
        match self.read_in(language) {
            Ok(text) => text.write_plain(out, language),
            Err(_) => out.push_str(&self.key),
        }
        for sibling in &self.base.siblings {
            sibling.write_plain(out, language);
        }
    }
}

impl Display for TranslatableComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.write_plain(&mut out, language::installed_or_empty());
        f.write_str(&out)
    }
}

impl PrimitiveOrComponent {
    /// The argument's text as `String.valueOf` gives it (Java's number text), translated with
    /// `language` when it is a component.
    pub fn text_in(&self, language: &Language) -> String {
        match self {
            PrimitiveOrComponent::Boolean(value) => value.to_string(),
            PrimitiveOrComponent::Short(value) => value.to_string(),
            PrimitiveOrComponent::Integer(value) => value.to_string(),
            PrimitiveOrComponent::Long(value) => value.to_string(),
            PrimitiveOrComponent::Float(value) => float_to_string(*value),
            PrimitiveOrComponent::Double(value) => double_to_string(*value),
            PrimitiveOrComponent::String(value) => value.clone(),
            PrimitiveOrComponent::FormattedText(value) => value.to_string_in(language),
        }
    }
}

impl Display for PrimitiveOrComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.write_str(&self.text_in(language::installed_or_empty()))
    }
}

impl From<PrimitiveOrComponent> for TextComponent {
    fn from(soc: PrimitiveOrComponent) -> Self {
        match soc {
            PrimitiveOrComponent::String(value) => TextComponent::new(value),
            other => TextComponent::new(other.text_in(language::installed_or_empty())),
        }
    }
}
impl From<&str> for TranslatableComponent {
    fn from(s: &str) -> Self {
        TranslatableComponent::new(s.to_owned(), vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// azalea's test strings (Minecraft's own `translation.test.*` keys).
    fn test_language() -> Language {
        Language::from_entries([
            ("translation.test.none", "Hello, world!"),
            (
                "translation.test.complex",
                "Prefix, %s%2$s again %s and %1$s lastly %s and also %1$s again!",
            ),
            ("translation.test.escape", "%%s %%%s %%%%s %%%%%s"),
            ("translation.test.invalid", "hi %"),
            ("translation.test.invalid2", "hi %  s"),
            ("translation.test.args", "%s %s"),
            ("translation.test.world", "world"),
        ])
    }

    fn render(c: &TranslatableComponent) -> String {
        c.read_in(&test_language()).unwrap().to_string()
    }

    #[test]
    fn test_none() {
        let c = TranslatableComponent::new("translation.test.none".to_owned(), vec![]);
        assert_eq!(render(&c), "Hello, world!".to_owned());
    }
    #[test]
    fn test_complex() {
        let c = TranslatableComponent::new(
            "translation.test.complex".to_owned(),
            vec![
                PrimitiveOrComponent::String("a".to_owned()),
                PrimitiveOrComponent::String("b".to_owned()),
                PrimitiveOrComponent::String("c".to_owned()),
                PrimitiveOrComponent::String("d".to_owned()),
            ],
        );
        // so true mojang
        assert_eq!(
            render(&c),
            "Prefix, ab again b and a lastly c and also a again!".to_owned()
        );
    }
    #[test]
    fn test_escape() {
        let c = TranslatableComponent::new(
            "translation.test.escape".to_owned(),
            vec![
                PrimitiveOrComponent::String("a".to_owned()),
                PrimitiveOrComponent::String("b".to_owned()),
                PrimitiveOrComponent::String("c".to_owned()),
                PrimitiveOrComponent::String("d".to_owned()),
            ],
        );
        assert_eq!(render(&c), "%s %a %%s %%b".to_owned());
    }
    #[test]
    fn test_invalid() {
        let c = TranslatableComponent::new(
            "translation.test.invalid".to_owned(),
            vec![
                PrimitiveOrComponent::String("a".to_owned()),
                PrimitiveOrComponent::String("b".to_owned()),
                PrimitiveOrComponent::String("c".to_owned()),
                PrimitiveOrComponent::String("d".to_owned()),
            ],
        );
        assert_eq!(render(&c), "hi %".to_owned());
    }
    #[test]
    fn test_invalid2() {
        let c = TranslatableComponent::new(
            "translation.test.invalid2".to_owned(),
            vec![
                PrimitiveOrComponent::String("a".to_owned()),
                PrimitiveOrComponent::String("b".to_owned()),
                PrimitiveOrComponent::String("c".to_owned()),
                PrimitiveOrComponent::String("d".to_owned()),
            ],
        );
        assert_eq!(render(&c), "hi %  s".to_owned());
    }

    #[test]
    fn test_undefined() {
        let c = TranslatableComponent::new(
            "translation.test.undefined".to_owned(),
            vec![PrimitiveOrComponent::String("a".to_owned())],
        );
        assert_eq!(render(&c), "translation.test.undefined".to_owned());
    }

    #[test]
    fn test_undefined_with_fallback() {
        let c = TranslatableComponent::with_fallback(
            "translation.test.undefined".to_owned(),
            Some("translation fallback: %s".to_owned()),
            vec![PrimitiveOrComponent::String("a".to_owned())],
        );
        assert_eq!(render(&c), "translation fallback: a".to_owned());
    }

    #[test]
    fn test_nested_translatable_arg() {
        let world = TranslatableComponent::new("translation.test.world".to_owned(), vec![]);
        let c = TranslatableComponent::new(
            "translation.test.args".to_owned(),
            vec![
                PrimitiveOrComponent::String("hello".to_owned()),
                PrimitiveOrComponent::FormattedText(world.into()),
            ],
        );
        assert_eq!(render(&c), "hello world");
        let outer = FormattedText::Translatable(c);
        assert_eq!(outer.to_string_in(&test_language()), "hello world");
        assert_eq!(
            outer.to_string_in(&Language::default()),
            "translation.test.args"
        );
    }

    #[test]
    fn test_java_number_text() {
        let c = TranslatableComponent::new(
            "translation.test.args".to_owned(),
            vec![
                PrimitiveOrComponent::Double(1.0),
                PrimitiveOrComponent::Float(1.0e7),
            ],
        );
        assert_eq!(render(&c), "1.0 1.0E7");
        assert_eq!(PrimitiveOrComponent::Double(0.5).to_string(), "0.5");
        assert_eq!(PrimitiveOrComponent::Float(-2.0).to_string(), "-2.0");
        assert_eq!(
            TextComponent::from(PrimitiveOrComponent::Double(1e-4)).text,
            "1.0E-4"
        );
        assert_eq!(PrimitiveOrComponent::Integer(3).to_string(), "3");
    }

    #[test]
    fn test_zero_positional_index() {
        let c = TranslatableComponent::with_fallback(
            "translation.test.undefined".to_owned(),
            Some("a%0$sb".to_owned()),
            vec![PrimitiveOrComponent::String("x".to_owned())],
        );
        assert_eq!(render(&c), "ab");
    }
}
