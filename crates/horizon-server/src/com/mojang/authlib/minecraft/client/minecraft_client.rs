//! Port of `com.mojang.authlib.minecraft.client.MinecraftClient`. This server never talks to Mojang's
//! services (see the package doc): every request fails as when they are unreachable, with
//! `MinecraftClientException(SERVICE_UNAVAILABLE)`, which authlib's callers already handle.
use crate::prelude::*;
use crate::com::mojang::authlib::exceptions::{MinecraftClientException, MinecraftClientExceptionErrorType};
use crate::java::net::Proxy;

#[derive(Clone)]
pub struct MinecraftClient {
    pub access_token: Option<String>,
    pub proxy: Proxy,
}
impl MinecraftClient {
    pub fn new(access_token: Option<String>, proxy: Proxy) -> Self {
        MinecraftClient { access_token, proxy }
    }

    pub fn unauthenticated(proxy: Proxy) -> Self {
        Self::new(None, proxy)
    }

    /// `get(URL, Class<T>)`.
    pub fn get<T>(&self, url: &str) -> Result<Option<T>, MinecraftClientException> {
        Err(Self::unavailable(url))
    }

    /// `post(URL, Object body, Class<T>)`.
    pub fn post<B, T>(&self, url: &str, body: B) -> Result<Option<T>, MinecraftClientException> {
        let _ = body;
        Err(Self::unavailable(url))
    }

    fn unavailable(url: &str) -> MinecraftClientException {
        MinecraftClientException::new(
            MinecraftClientExceptionErrorType::SERVICE_UNAVAILABLE,
            &format!("Failed to read from {url} due to services being unavailable"),
        )
    }
}
