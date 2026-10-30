//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct NamedTextColor(pub std::marker::PhantomData<()>);
impl Clone for NamedTextColor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ShadowColor(pub std::marker::PhantomData<()>);
impl Clone for ShadowColor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Style(pub std::marker::PhantomData<()>);
impl Clone for Style { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct StyleBuilder(pub std::marker::PhantomData<()>);
impl Clone for StyleBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait StyleBuilderApplicable: Send + Sync {}
pub struct TextColor(pub std::marker::PhantomData<()>);
impl Clone for TextColor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TextDecoration(pub std::marker::PhantomData<()>);
impl Clone for TextDecoration { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
