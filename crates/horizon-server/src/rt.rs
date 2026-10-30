//! The few runtime pieces no Rust crate provides in the shape Leaf's code needs. Everything else from
//! Java's libraries is a Rust crate type (PORTING.md "Rust libraries instead of Java's").

use parking_lot::{Condvar, Mutex};
use std::any::TypeId;
use std::sync::Arc;

/// A Java class token (`Foo.class`): the type and its simple name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Class {
    pub id: TypeId,
    pub name: &'static str,
}
impl Class {
    pub fn of<T: ?Sized + 'static>() -> Class {
        let n = std::any::type_name::<T>();
        Class { id: TypeId::of::<T>(), name: n.rsplit("::").next().unwrap_or(n) }
    }
    pub fn get_simple_name(&self) -> String {
        self.name.to_string()
    }
}

/// A value completed later, once, by any thread, with callbacks (Java's `CompletableFuture`).
pub struct CompletableFuture<T>(Arc<Completion<T>>);
pub struct Completion<T> {
    state: Mutex<(Option<Result<T, Arc<anyhow::Error>>>, Vec<Box<dyn FnOnce(&Result<T, Arc<anyhow::Error>>) + Send>>)>,
    done: Condvar,
}
impl<T> Clone for CompletableFuture<T> {
    fn clone(&self) -> Self {
        CompletableFuture(self.0.clone())
    }
}
impl<T: Clone + Send + 'static> Default for CompletableFuture<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Clone + Send + 'static> CompletableFuture<T> {
    pub fn new() -> Self {
        CompletableFuture(Arc::new(Completion { state: Mutex::new((None, Vec::new())), done: Condvar::new() }))
    }
    pub fn completed(v: T) -> Self {
        let f = Self::new();
        f.complete(v);
        f
    }
    fn finish(&self, r: Result<T, Arc<anyhow::Error>>) -> bool {
        let callbacks = {
            let mut s = self.0.state.lock();
            if s.0.is_some() {
                return false;
            }
            s.0 = Some(r.clone());
            std::mem::take(&mut s.1)
        };
        self.0.done.notify_all();
        for c in callbacks {
            c(&r);
        }
        true
    }
    pub fn complete(&self, v: T) -> bool {
        self.finish(Ok(v))
    }
    pub fn complete_exceptionally(&self, e: anyhow::Error) -> bool {
        self.finish(Err(Arc::new(e)))
    }
    pub fn is_done(&self) -> bool {
        self.0.state.lock().0.is_some()
    }
    pub fn get_now(&self, default: T) -> T {
        match &self.0.state.lock().0 {
            Some(Ok(v)) => v.clone(),
            _ => default,
        }
    }
    /// Blocks until done (Java's `join`).
    pub fn join(&self) -> Result<T, Arc<anyhow::Error>> {
        let mut s = self.0.state.lock();
        while s.0.is_none() {
            self.0.done.wait(&mut s);
        }
        s.0.clone().unwrap()
    }
    /// Runs `f` with the result once it is there (now, if it already is): `whenComplete`.
    pub fn when_complete(&self, f: impl FnOnce(&Result<T, Arc<anyhow::Error>>) + Send + 'static) {
        let mut s = self.0.state.lock();
        if let Some(r) = s.0.clone() {
            drop(s);
            f(&r);
        } else {
            s.1.push(Box::new(f));
        }
    }
    pub fn then_apply<U: Clone + Send + 'static>(&self, f: impl FnOnce(T) -> U + Send + 'static) -> CompletableFuture<U> {
        let out = CompletableFuture::new();
        let o = out.clone();
        self.when_complete(move |r| match r {
            Ok(v) => {
                o.complete(f(v.clone()));
            }
            Err(e) => {
                o.finish(Err(e.clone()));
            }
        });
        out
    }
    pub fn then_accept(&self, f: impl FnOnce(T) + Send + 'static) -> CompletableFuture<()> {
        self.then_apply(f)
    }
}

/// A task executor (Java's `Executor`/`ExecutorService`): a rayon pool.
pub type Executor = Arc<rayon::ThreadPool>;

/// `java.util.concurrent.TimeUnit`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TimeUnit {
    NANOSECONDS,
    MICROSECONDS,
    MILLISECONDS,
    SECONDS,
    MINUTES,
    HOURS,
    DAYS,
}
impl TimeUnit {
    pub fn to_duration(self, n: i64) -> std::time::Duration {
        let n = n.max(0) as u64;
        match self {
            TimeUnit::NANOSECONDS => std::time::Duration::from_nanos(n),
            TimeUnit::MICROSECONDS => std::time::Duration::from_micros(n),
            TimeUnit::MILLISECONDS => std::time::Duration::from_millis(n),
            TimeUnit::SECONDS => std::time::Duration::from_secs(n),
            TimeUnit::MINUTES => std::time::Duration::from_secs(n * 60),
            TimeUnit::HOURS => std::time::Duration::from_secs(n * 3600),
            TimeUnit::DAYS => std::time::Duration::from_secs(n * 86400),
        }
    }
}

/// Moonrise's task priorities (Spottedleaf's `Priority`), highest first.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Priority {
    COMPLETING,
    BLOCKING,
    HIGHEST,
    HIGHER,
    HIGH,
    NORMAL,
    LOW,
    LOWER,
    LOWEST,
    IDLE,
}

/// A player's identity as the proxy forwards it (authlib's `GameProfile`, without Mojang services).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GameProfile {
    pub id: uuid::Uuid,
    pub name: String,
    pub properties: PropertyMap,
}
/// A signed profile property (skins).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}
pub type PropertyMap = indexmap::IndexMap<String, Vec<Property>>;


/// What the port still has to replace: library calls with no Rust counterpart yet (integration).
pub struct Unported;
