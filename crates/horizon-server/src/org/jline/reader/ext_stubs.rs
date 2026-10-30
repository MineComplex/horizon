//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Candidate(pub std::marker::PhantomData<()>);
impl Clone for Candidate { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct CompletingParsedLine(pub std::marker::PhantomData<()>);
impl Clone for CompletingParsedLine { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LineReader(pub std::marker::PhantomData<()>);
impl Clone for LineReader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LineReaderBuilder(pub std::marker::PhantomData<()>);
impl Clone for LineReaderBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LineReaderOption(pub std::marker::PhantomData<()>);
impl Clone for LineReaderOption { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait ParsedLine: Send + Sync {}
