//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AxisAngle4f(pub std::marker::PhantomData<()>);
impl Clone for AxisAngle4f { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Matrix3f(pub std::marker::PhantomData<()>);
impl Clone for Matrix3f { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Matrix3fc(pub std::marker::PhantomData<()>);
impl Clone for Matrix3fc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Matrix4f(pub std::marker::PhantomData<()>);
impl Clone for Matrix4f { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Matrix4fc(pub std::marker::PhantomData<()>);
impl Clone for Matrix4fc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Quaternionf(pub std::marker::PhantomData<()>);
impl Clone for Quaternionf { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Quaternionfc(pub std::marker::PhantomData<()>);
impl Clone for Quaternionfc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector2fc(pub std::marker::PhantomData<()>);
impl Clone for Vector2fc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector3d(pub std::marker::PhantomData<()>);
impl Clone for Vector3d { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector3dc(pub std::marker::PhantomData<()>);
impl Clone for Vector3dc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector3f(pub std::marker::PhantomData<()>);
impl Clone for Vector3f { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector3fc(pub std::marker::PhantomData<()>);
impl Clone for Vector3fc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector3i(pub std::marker::PhantomData<()>);
impl Clone for Vector3i { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector3ic(pub std::marker::PhantomData<()>);
impl Clone for Vector3ic { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector4f(pub std::marker::PhantomData<()>);
impl Clone for Vector4f { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Vector4fc(pub std::marker::PhantomData<()>);
impl Clone for Vector4fc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
