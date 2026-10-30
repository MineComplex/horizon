//! Port of `com.mojang.authlib.minecraft.MinecraftProfileTextures` (a record).
use crate::prelude::*;
use crate::com::mojang::authlib::SignatureState;
use crate::com::mojang::authlib::minecraft::MinecraftProfileTexture;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinecraftProfileTextures {
    pub skin: Option<MinecraftProfileTexture>,
    pub cape: Option<MinecraftProfileTexture>,
    pub elytra: Option<MinecraftProfileTexture>,
    pub signature_state: SignatureState,
}
pub static EMPTY: LazyLock<MinecraftProfileTextures> =
    LazyLock::new(|| MinecraftProfileTextures::new(None, None, None, SignatureState::SIGNED));
impl MinecraftProfileTextures {
    pub fn new(
        skin: Option<MinecraftProfileTexture>,
        cape: Option<MinecraftProfileTexture>,
        elytra: Option<MinecraftProfileTexture>,
        signature_state: SignatureState,
    ) -> Self {
        MinecraftProfileTextures { skin, cape, elytra, signature_state }
    }
    pub fn skin(&self) -> Option<MinecraftProfileTexture> {
        self.skin.clone()
    }
    pub fn cape(&self) -> Option<MinecraftProfileTexture> {
        self.cape.clone()
    }
    pub fn elytra(&self) -> Option<MinecraftProfileTexture> {
        self.elytra.clone()
    }
    pub fn signature_state(&self) -> SignatureState {
        self.signature_state
    }
}
