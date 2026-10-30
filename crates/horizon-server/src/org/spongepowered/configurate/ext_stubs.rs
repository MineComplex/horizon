//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CommentedConfigurationNode(pub std::marker::PhantomData<()>);
impl Clone for CommentedConfigurationNode { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConfigurationNode(pub std::marker::PhantomData<()>);
impl Clone for ConfigurationNode { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConfigurationOptions(pub std::marker::PhantomData<()>);
impl Clone for ConfigurationOptions { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct NodePath(pub std::marker::PhantomData<()>);
impl Clone for NodePath { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
