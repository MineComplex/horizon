use crate::azalea::chat::{FormattedText, style::Style};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BaseComponent {
    /// Components in the "extra" field.
    pub siblings: Vec<FormattedText>,
    pub style: Box<Style>,
}

impl BaseComponent {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_style(self, style: Style) -> Self {
        Self {
            style: Box::new(style),
            ..self
        }
    }
}
