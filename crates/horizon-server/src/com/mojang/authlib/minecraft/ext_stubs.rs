//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct SessionService(pub std::marker::PhantomData<()>);
impl Clone for SessionService { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct UserApiService(pub std::marker::PhantomData<()>);
impl Clone for UserApiService { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
