//! The parts of Netty the port keeps: `buffer` (ByteBuf), `util` (reference counting, attribute keys) and the
//! codec/timeout exception classes NMS names. Channels, pipelines and event loops are replaced by the tokio
//! transport in `net::minecraft::network::connection` (PORTING.md, "Left out").
pub mod buffer;
pub mod handler;
pub mod util;
