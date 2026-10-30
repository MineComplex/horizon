//! Port of `net.kyori.adventure.text.event.DataComponentValueConverterRegistry` (with `Provider`,
//! `Conversion`). Java's classes are `TypeId`s of `JRef<T>`; a conversion applies when the value can be
//! cast to its source type (its `jobject!` list), as Java walks the value's supertypes.
use crate::prelude::*;
use crate::net::kyori::adventure::key::Key;
use crate::net::kyori::adventure::text::event::DataComponentValue;
use std::any::TypeId;

// ---- interface net.kyori.adventure.text.event.DataComponentValueConverterRegistry.Provider
pub trait DataComponentValueConverterRegistryProvider: Send + Sync {
    fn id(&self) -> JRef<dyn Key>;
    fn conversions(&self) -> Vec<DataComponentValueConverterRegistryConversion<JObjectRef, JObjectRef>>;
}

// ---- interface net.kyori.adventure.text.event.DataComponentValueConverterRegistry.Conversion
pub struct DataComponentValueConverterRegistryConversion<I = JObjectRef, O = JObjectRef> {
    source_test: Arc<dyn Fn(&JObjectRef) -> bool + Send + Sync>,
    destination: TypeId,
    op: Arc<dyn Fn(JRef<dyn Key>, &JObjectRef) -> DataComponentValue + Send + Sync>,
    _p: std::marker::PhantomData<fn() -> (I, O)>,
}
impl<I, O> Clone for DataComponentValueConverterRegistryConversion<I, O> {
    fn clone(&self) -> Self {
        DataComponentValueConverterRegistryConversion {
            source_test: self.source_test.clone(),
            destination: self.destination,
            op: self.op.clone(),
            _p: std::marker::PhantomData,
        }
    }
}
impl DataComponentValueConverterRegistryConversion<JObjectRef, JObjectRef> {
    /// `Conversion.convert(Class<I> src, Class<O> dst, BiFunction<Key, I, O>)`: `I` and `O` are the
    /// source and destination types (`PaperAdventureDataComponentValueImpl<..>`, `dyn
    /// DataComponentValueTagSerializable`...); the function returns the converted value.
    pub fn convert<I: ?Sized + 'static, O: ?Sized + 'static>(
        op: impl Fn(JRef<dyn Key>, JRef<I>) -> DataComponentValue + Send + Sync + 'static,
    ) -> Self {
        DataComponentValueConverterRegistryConversion {
            source_test: Arc::new(|v| instance_of::<I, _>(v)),
            destination: TypeId::of::<JRef<O>>(),
            op: Arc::new(move |key, v| op(key, cast::<I, _>(v).expect("source type"))),
            _p: std::marker::PhantomData,
        }
    }
    pub fn destination(&self) -> TypeId {
        self.destination
    }
}

/// `DataComponentValueConverterRegistry.PROVIDERS`: Paper's `DataComponentValueConverterProviderImpl`.
pub static PROVIDERS: LazyLock<Vec<JRef<dyn DataComponentValueConverterRegistryProvider>>> = LazyLock::new(|| {
    vec![crate::io::papermc::paper::adventure::providers::data_component_value_converter_provider_impl::DataComponentValueConverterProviderImpl::new()]
});

pub struct DataComponentValueConverterRegistry;
impl DataComponentValueConverterRegistry {
    pub fn known_providers() -> HashSet<JRef<dyn Key>> {
        PROVIDERS.iter().map(|p| p.id()).collect()
    }
    /// `convert(Class<O> target, Key, DataComponentValue)`.
    pub fn convert<O: ?Sized + 'static>(key: JRef<dyn Key>, in_: DataComponentValue) -> DataComponentValue {
        if instance_of::<O, _>(&in_.0) {
            return in_;
        }
        let target = TypeId::of::<JRef<O>>();
        for provider in PROVIDERS.iter() {
            for conversion in provider.conversions() {
                if conversion.destination == target && (conversion.source_test)(&in_.0) {
                    return (conversion.op)(key, &in_.0);
                }
            }
        }
        panic!(
            "java.lang.IllegalArgumentException: There is no data holder converter registered to convert to {} (on field {})",
            std::any::type_name::<O>(),
            key.as_string()
        );
    }
}
impl DataComponentValueConverterRegistryProvider
    for crate::io::papermc::paper::adventure::providers::data_component_value_converter_provider_impl::DataComponentValueConverterProviderImpl
{
    fn id(&self) -> JRef<dyn Key> {
        crate::io::papermc::paper::adventure::providers::data_component_value_converter_provider_impl::DataComponentValueConverterProviderImpl::id(self)
    }
    fn conversions(&self) -> Vec<DataComponentValueConverterRegistryConversion<JObjectRef, JObjectRef>> {
        crate::io::papermc::paper::adventure::providers::data_component_value_converter_provider_impl::DataComponentValueConverterProviderImpl::conversions(self)
    }
}
