//! Port of `net.kyori.adventure.text.event.ClickCallback` (with `Options`, `Options.Builder`, `Provider`).
use crate::prelude::*;
use crate::net::kyori::adventure::audience::Audience;
use crate::net::kyori::adventure::text::event::ClickEvent;
use std::time::Duration;

/// `ClickCallback.DEFAULT_LIFETIME`: 12 hours.
pub const DEFAULT_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
/// `ClickCallback.UNLIMITED_USES`.
pub const UNLIMITED_USES: i32 = -1;

// ---- interface net.kyori.adventure.text.event.ClickCallback<T extends Audience> (functional)
pub struct ClickCallback<T = JRef<dyn Audience>>(pub Arc<dyn Fn(T) + Send + Sync>);
impl<T> Clone for ClickCallback<T> {
    fn clone(&self) -> Self {
        ClickCallback(self.0.clone())
    }
}
impl<T: 'static> ClickCallback<T> {
    pub fn new(f: impl Fn(T) + Send + Sync + 'static) -> Self {
        ClickCallback(Arc::new(f))
    }
    pub fn accept(&self, audience: T) {
        (self.0)(audience)
    }
    /// `filter(Predicate<T>)`.
    pub fn filter(&self, filter: impl Fn(&T) -> bool + Send + Sync + 'static) -> ClickCallback<T> {
        self.filter_2(filter, None::<fn(&T)>)
    }
    /// `filter(Predicate<T>, Consumer<? super Audience>)`.
    pub fn filter_2(
        &self,
        filter: impl Fn(&T) -> bool + Send + Sync + 'static,
        otherwise: Option<impl Fn(&T) + Send + Sync + 'static>,
    ) -> ClickCallback<T> {
        let this = self.clone();
        ClickCallback::new(move |audience: T| {
            if filter(&audience) {
                this.accept(audience);
            } else if let Some(o) = &otherwise {
                o(&audience);
            }
        })
    }
}
impl ClickCallback<JRef<dyn Audience>> {
    /// `requiringPermission(String)`: runs only for audiences with the permission
    /// (`Audience.pointers().get(PermissionChecker.POINTER)`, Paper's permissible check).
    pub fn requiring_permission(&self, permission: &str) -> ClickCallback<JRef<dyn Audience>> {
        self.requiring_permission_2(permission, None::<fn(&JRef<dyn Audience>)>)
    }
    pub fn requiring_permission_2(
        &self,
        permission: &str,
        otherwise: Option<impl Fn(&JRef<dyn Audience>) + Send + Sync + 'static>,
    ) -> ClickCallback<JRef<dyn Audience>> {
        let permission = permission.to_string();
        self.filter_2(move |audience| audience.has_permission_pointer(&permission), otherwise)
    }
}

// ---- interface net.kyori.adventure.text.event.ClickCallback.Options (ClickCallbackOptionsImpl)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClickCallbackOptions {
    uses: i32,
    lifetime: Duration,
}
impl ClickCallbackOptions {
    pub fn builder() -> ClickCallbackOptionsBuilder {
        ClickCallbackOptionsBuilder(Arc::new(Mut::new(ClickCallbackOptions { uses: 1, lifetime: DEFAULT_LIFETIME })))
    }
    /// `Options.builder(Options existing)`.
    pub fn builder_1(existing: ClickCallbackOptions) -> ClickCallbackOptionsBuilder {
        ClickCallbackOptionsBuilder(Arc::new(Mut::new(existing)))
    }
    pub fn uses(&self) -> i32 {
        self.uses
    }
    pub fn lifetime(&self) -> Duration {
        self.lifetime
    }
}

// ---- interface net.kyori.adventure.text.event.ClickCallback.Options.Builder
#[derive(Clone)]
pub struct ClickCallbackOptionsBuilder(Arc<Mut<ClickCallbackOptions>>);
impl ClickCallbackOptionsBuilder {
    pub fn uses(&self, use_count: i32) -> Self {
        if use_count != UNLIMITED_USES && use_count <= 0 {
            panic!("java.lang.IllegalArgumentException: Click callback uses must be positive or UNLIMITED_USES");
        }
        self.0.borrow_mut().uses = use_count;
        self.clone()
    }
    pub fn lifetime(&self, duration: Duration) -> Self {
        self.0.borrow_mut().lifetime = duration;
        self.clone()
    }
    pub fn build(&self) -> ClickCallbackOptions {
        self.0.get()
    }
}

// ---- interface net.kyori.adventure.text.event.ClickCallback.Provider
pub trait ClickCallbackProvider: Send + Sync {
    fn create(&self, callback: ClickCallback<JRef<dyn Audience>>, options: ClickCallbackOptions) -> ClickEvent;
}

/// `ClickCallbackInternals.PROVIDER`: Paper's `ClickCallbackProviderImpl` (its `META-INF/services` entry).
pub fn provider() -> JRef<dyn ClickCallbackProvider> {
    static PROVIDER: LazyLock<JRef<dyn ClickCallbackProvider>> = LazyLock::new(|| {
        crate::io::papermc::paper::adventure::providers::click_callback_provider_impl::ClickCallbackProviderImpl::new()
    });
    PROVIDER.clone()
}
impl ClickCallbackProvider for crate::io::papermc::paper::adventure::providers::click_callback_provider_impl::ClickCallbackProviderImpl {
    fn create(&self, callback: ClickCallback<JRef<dyn Audience>>, options: ClickCallbackOptions) -> ClickEvent {
        crate::io::papermc::paper::adventure::providers::click_callback_provider_impl::ClickCallbackProviderImpl::create(self, callback, options)
    }
}
