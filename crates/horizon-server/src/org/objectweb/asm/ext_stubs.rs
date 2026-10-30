//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AnnotationVisitor(pub std::marker::PhantomData<()>);
impl Clone for AnnotationVisitor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ClassVisitor(pub std::marker::PhantomData<()>);
impl Clone for ClassVisitor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MethodVisitor(pub std::marker::PhantomData<()>);
impl Clone for MethodVisitor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Type(pub std::marker::PhantomData<()>);
impl Clone for Type { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
