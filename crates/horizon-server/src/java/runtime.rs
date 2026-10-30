//! The Java object model the port runs on (crates/horizon-server/PORTING.md).
//!
//! Java objects are shared, mutable through any reference, and collected by a GC. The port mirrors
//! that: an object is `JRef<T>` (an `Arc`), every method takes `&self`, and fields that Java ever
//! assigns after construction are `Mut<T>`, which hands out short-lived `&mut` through `&self`
//! exactly as Java does (no locking, Java's memory model: `synchronized` code takes a monitor).

use std::any::{Any, TypeId};
use std::cell::UnsafeCell;
use std::sync::Arc;

/// A Java reference: shared ownership, identity by pointer (`Arc::ptr_eq`).
pub type JRef<T> = Arc<T>;

/// `java.lang.Object`: any ported object.
pub type JObjectRef = Arc<dyn JObject>;

/// A Java field the program assigns after construction (or a collection it mutates).
///
/// `get`/`set` copy values in and out; `borrow_mut` lends the value for one statement's worth of
/// work. As in Java, nothing stops two threads (or a re-entrant call) from touching the same field:
/// keep each borrow short and never hold one across a call that can come back to this object.
#[repr(transparent)]
pub struct Mut<T: ?Sized>(UnsafeCell<T>);

// Java semantics: fields are shared between threads; the code synchronizes where Java does.
unsafe impl<T: ?Sized + Send> Sync for Mut<T> {}
unsafe impl<T: ?Sized + Send> Send for Mut<T> {}

impl<T> Mut<T> {
    pub const fn new(v: T) -> Self {
        Mut(UnsafeCell::new(v))
    }
    /// Reads the field (Java: `this.x`).
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        unsafe { (*self.0.get()).clone() }
    }
    /// Assigns the field (Java: `this.x = v`).
    pub fn set(&self, v: T) {
        unsafe { *self.0.get() = v }
    }
    pub fn replace(&self, v: T) -> T {
        unsafe { std::mem::replace(&mut *self.0.get(), v) }
    }
    pub fn take(&self) -> T
    where
        T: Default,
    {
        self.replace(T::default())
    }
    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

impl<T: ?Sized> Mut<T> {
    /// Lends the value for reading.
    #[allow(clippy::mut_from_ref)]
    pub fn borrow(&self) -> &T {
        unsafe { &*self.0.get() }
    }
    /// Lends the value for mutation (Java: `this.list.add(x)`). Keep it short.
    #[allow(clippy::mut_from_ref)]
    pub fn borrow_mut(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
    /// Runs `f` on the value (Java: a compound update such as `this.count += n`).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(self.borrow_mut())
    }
}

impl<T: Default> Default for Mut<T> {
    fn default() -> Self {
        Mut::new(T::default())
    }
}

impl<T: Clone> Clone for Mut<T> {
    fn clone(&self) -> Self {
        Mut::new(self.get())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Mut<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.borrow().fmt(f)
    }
}

/// Bound for Java type parameters: they stand for references or values the port clones freely.
pub trait JGeneric: Clone + Send + Sync + 'static {}
impl<T: Clone + Send + Sync + 'static> JGeneric for T {}

/// `java.lang.Object`'s runtime type information: `instanceof` and casts (`cast`).
pub trait JObject: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    /// `self` as `Arc<Target>` when this object's class is or implements `Target`
    /// (`target` = `TypeId::of::<Arc<Target>>()`): boxed `Arc<Target>`. Implemented by `jobject!`.
    fn cast_arc(self: Arc<Self>, target: TypeId) -> Option<Box<dyn Any>>;
    /// The Java class name (`getClass().getSimpleName()`).
    fn class_name(&self) -> &'static str {
        let n = std::any::type_name::<Self>();
        n.rsplit("::").next().unwrap_or(n)
    }
}

/// Java's checked cast `(T) o` / `o instanceof T t`: `T` is a concrete struct or `dyn Trait`.
pub fn cast<T: ?Sized + 'static, S: ?Sized + JObject>(o: &Arc<S>) -> Option<Arc<T>> {
    o.clone().cast_arc(TypeId::of::<Arc<T>>()).and_then(|b| b.downcast::<Arc<T>>().ok()).map(|b| *b)
}

/// `o instanceof T`.
pub fn instance_of<T: ?Sized + 'static, S: ?Sized + JObject>(o: &Arc<S>) -> bool {
    cast::<T, S>(o).is_some()
}

/// Upcasts any ported object to `java.lang.Object`.
pub fn as_object<S: JObject>(o: Arc<S>) -> JObjectRef {
    o
}

/// Java's `==` on references.
pub fn same<A: ?Sized, B: ?Sized>(a: &Arc<A>, b: &Arc<B>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

/// A key compared by identity (Java's `IdentityHashMap`, fastutil `Reference*` collections).
#[derive(Clone)]
pub struct ById<T: ?Sized>(pub Arc<T>);
impl<T: ?Sized> PartialEq for ById<T> {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.0, &o.0)
    }
}
impl<T: ?Sized> Eq for ById<T> {}
impl<T: ?Sized> std::hash::Hash for ById<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (Arc::as_ptr(&self.0) as *const () as usize).hash(h)
    }
}
impl<T: ?Sized> std::ops::Deref for ById<T> {
    type Target = Arc<T>;
    fn deref(&self) -> &Arc<T> {
        &self.0
    }
}

/// Java's object monitor (`synchronized (this)`): re-entrant, as Java's is.
#[derive(Default)]
pub struct Monitor(parking_lot::ReentrantMutex<()>);
impl Monitor {
    pub const fn new() -> Self {
        Monitor(parking_lot::ReentrantMutex::new(()))
    }
    pub fn lock(&self) -> parking_lot::ReentrantMutexGuard<'_, ()> {
        self.0.lock()
    }
}

/// Logging: slf4j's `LOGGER` fields are dropped; use `log::info!` and friends.
pub struct Logger;

/// Implements `JObject` for a ported type, listing the class traits and interfaces it can be cast to:
/// `jobject!(Zombie: dyn EntityTrait, dyn MobTrait, dyn Enemy);`
#[macro_export]
macro_rules! jobject {
    ($t:ty $(: $($target:ty),* $(,)?)?) => {
        impl $crate::java::JObject for $t {
            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }
            #[allow(unused_variables)]
            fn cast_arc(
                self: ::std::sync::Arc<Self>,
                target: ::std::any::TypeId,
            ) -> ::std::option::Option<::std::boxed::Box<dyn ::std::any::Any>> {
                if target == ::std::any::TypeId::of::<::std::sync::Arc<Self>>() {
                    return Some(::std::boxed::Box::new(self));
                }
                $($(
                    if target == ::std::any::TypeId::of::<::std::sync::Arc<$target>>() {
                        return Some(::std::boxed::Box::new(self as ::std::sync::Arc<$target>));
                    }
                )*)?
                None
            }
        }
    };
}
