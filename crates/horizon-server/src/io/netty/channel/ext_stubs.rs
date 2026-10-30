//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Channel(pub std::marker::PhantomData<()>);
impl Clone for Channel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChannelFuture(pub std::marker::PhantomData<()>);
impl Clone for ChannelFuture { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChannelFutureListener(pub std::marker::PhantomData<()>);
impl Clone for ChannelFutureListener { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChannelHandlerContext(pub std::marker::PhantomData<()>);
impl Clone for ChannelHandlerContext { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChannelInboundHandler(pub std::marker::PhantomData<()>);
impl Clone for ChannelInboundHandler { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChannelOutboundHandler(pub std::marker::PhantomData<()>);
impl Clone for ChannelOutboundHandler { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChannelPipeline(pub std::marker::PhantomData<()>);
impl Clone for ChannelPipeline { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChannelPromise(pub std::marker::PhantomData<()>);
impl Clone for ChannelPromise { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct EventLoopGroup(pub std::marker::PhantomData<()>);
impl Clone for EventLoopGroup { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct IoHandlerFactory(pub std::marker::PhantomData<()>);
impl Clone for IoHandlerFactory { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ServerChannel(pub std::marker::PhantomData<()>);
impl Clone for ServerChannel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SingleThreadEventLoop(pub std::marker::PhantomData<()>);
impl Clone for SingleThreadEventLoop { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
