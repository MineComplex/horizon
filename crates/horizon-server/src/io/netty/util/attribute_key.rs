//! Port of `io.netty.util.AttributeKey<T>` (a `ConstantPool` constant): the key of an `Attribute<T>` on a
//! channel. Keys with the same name are the same key (`valueOf`), compared by id.
use crate::prelude::*;
use std::marker::PhantomData;

static POOL: LazyLock<parking_lot::Mutex<HashMap<String, i32>>> = LazyLock::new(Default::default);
static NEXT_ID: AtomicI32 = AtomicI32::new(1);

pub struct AttributeKey<T = JObjectRef> {
    id: i32,
    name: Arc<str>,
    _t: PhantomData<fn() -> T>,
}

impl<T> Clone for AttributeKey<T> {
    fn clone(&self) -> Self {
        AttributeKey {
            id: self.id,
            name: self.name.clone(),
            _t: PhantomData,
        }
    }
}
impl<T> PartialEq for AttributeKey<T> {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
    }
}
impl<T> Eq for AttributeKey<T> {}
impl<T> std::hash::Hash for AttributeKey<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.id.hash(h)
    }
}
impl<T> std::fmt::Debug for AttributeKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl<T> AttributeKey<T> {
    fn of(id: i32, name: &str) -> Self {
        AttributeKey {
            id,
            name: Arc::from(name),
            _t: PhantomData,
        }
    }
    /// `AttributeKey.valueOf(name)`: the singleton key for `name`.
    pub fn value_of(name: &str) -> AttributeKey<T> {
        let mut pool = POOL.lock();
        let id = *pool
            .entry(name.to_string())
            .or_insert_with(|| NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed));
        Self::of(id, name)
    }
    /// `AttributeKey.exists(name)`.
    pub fn exists(name: &str) -> bool {
        POOL.lock().contains_key(name)
    }
    /// `AttributeKey.newInstance(name)`: a new key; panics (IllegalArgumentException) if `name` is in use.
    pub fn new_instance(name: &str) -> AttributeKey<T> {
        let mut pool = POOL.lock();
        if pool.contains_key(name) {
            panic!("IllegalArgumentException: '{name}' is already in use");
        }
        let id = NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
        pool.insert(name.to_string(), id);
        Self::of(id, name)
    }
    pub fn id(&self) -> i32 {
        self.id
    }
    pub fn name(&self) -> String {
        self.name.to_string()
    }
    pub fn to_string(&self) -> String {
        self.name()
    }
    pub fn compare_to(&self, o: &AttributeKey<T>) -> i32 {
        self.id.cmp(&o.id) as i32
    }
}
