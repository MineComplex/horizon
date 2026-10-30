//! Taken from azalea-chat 0.16.0 (https://github.com/azalea-rs/azalea, MIT), brought to 26.3's
//! component format (`ComponentSerialization`) in `codec`.

pub mod base_component;
pub mod click_event;
pub mod codec;
mod component;
pub mod contents;
pub mod hover_event;
#[cfg(feature = "numbers")]
pub mod numbers;
pub mod style;
pub mod text_component;
pub mod translatable_component;

pub use codec::{read_component, write_component};
pub use component::{DEFAULT_STYLE, FormattedText};
