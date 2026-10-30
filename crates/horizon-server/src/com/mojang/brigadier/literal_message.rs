//! Port of `com.mojang.brigadier.LiteralMessage` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;

pub struct LiteralMessage {
    pub string: String,
}

impl LiteralMessage {
    pub fn new(string: &str) -> JRef<LiteralMessage> {
        Arc::new(LiteralMessage { string: string.to_string() })
    }

    pub fn get_string(&self) -> String {
        self.string.clone()
    }

    pub fn to_string(&self) -> String {
        self.string.clone()
    }
}

impl Message for LiteralMessage {
    fn get_string(&self) -> String {
        LiteralMessage::get_string(self)
    }
}

crate::jobject!(LiteralMessage: dyn Message);
