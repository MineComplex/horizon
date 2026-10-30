//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Component(pub std::marker::PhantomData<()>);
impl Clone for Component { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait ComponentLike: Send + Sync {}
pub struct KeybindComponent(pub std::marker::PhantomData<()>);
impl Clone for KeybindComponent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct NBTComponent<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for NBTComponent<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct NBTComponentBuilder<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for NBTComponentBuilder<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ObjectComponent(pub std::marker::PhantomData<()>);
impl Clone for ObjectComponent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ScoreComponent(pub std::marker::PhantomData<()>);
impl Clone for ScoreComponent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SelectorComponent(pub std::marker::PhantomData<()>);
impl Clone for SelectorComponent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TextComponent(pub std::marker::PhantomData<()>);
impl Clone for TextComponent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TextComponentBuilder(pub std::marker::PhantomData<()>);
impl Clone for TextComponentBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TranslatableComponent(pub std::marker::PhantomData<()>);
impl Clone for TranslatableComponent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TranslationArgument(pub std::marker::PhantomData<()>);
impl Clone for TranslationArgument { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
