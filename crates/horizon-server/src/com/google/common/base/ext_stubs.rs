//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CharMatcher(pub std::marker::PhantomData<()>);
impl Clone for CharMatcher { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Converter(pub std::marker::PhantomData<()>);
impl Clone for Converter { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Function<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for Function<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Joiner(pub std::marker::PhantomData<()>);
impl Clone for Joiner { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Optional(pub std::marker::PhantomData<()>);
impl Clone for Optional { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Splitter(pub std::marker::PhantomData<()>);
impl Clone for Splitter { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Stopwatch(pub std::marker::PhantomData<()>);
impl Clone for Stopwatch { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Ticker(pub std::marker::PhantomData<()>);
impl Clone for Ticker { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
