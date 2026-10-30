//! Port of `com.mojang.authlib.HttpDiscoveryService` (the base of `MinecraftServicesDiscoveryService`).
//! URLs stay strings: no request ever leaves this server (see the package doc).
use crate::prelude::*;
use crate::java::net::Proxy;

pub struct HttpDiscoveryService {
    pub proxy: Proxy,
}
impl HttpDiscoveryService {
    pub fn new(proxy: Proxy) -> Self {
        HttpDiscoveryService { proxy }
    }

    pub fn get_proxy(&self) -> Proxy {
        self.proxy.clone()
    }

    /// `constantURL`: Java builds a `URL`; the port keeps the string.
    pub fn constant_url(url: &str) -> String {
        url.to_string()
    }

    pub fn build_query(query: Option<&IndexMap<String, Option<String>>>) -> String {
        let Some(query) = query else { return String::new() };
        let mut builder = String::new();
        for (k, v) in query {
            if !builder.is_empty() {
                builder.push('&');
            }
            builder.push_str(&url_encode(k));
            if let Some(v) = v {
                builder.push('=');
                builder.push_str(&url_encode(v));
            }
        }
        builder
    }

    pub fn concatenate_url(url: &str, query: &str) -> String {
        if url.contains('?') { format!("{url}&{query}") } else { format!("{url}?{query}") }
    }
}

/// `URLEncoder.encode(s, UTF_8)`.
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
