//! jtracy 1.14.38 (`com.mojang.jtracy`): the Tracy profiler's client. The native library is never loaded
//! here, so `TracyClient::is_available()` is false and every zone, frame and plot is a no-op, as jtracy
//! behaves without its natives. `try (Zone z = TracyClient.beginZone(..))` -> `let _z = TracyClient::begin_zone(..);`.
pub mod continuous_frame;
pub mod discontinuous_frame;
pub mod plot;
pub mod tracy_client;
pub mod zone;
pub use self::continuous_frame::ContinuousFrame;
pub use self::discontinuous_frame::DiscontinuousFrame;
pub use self::plot::Plot;
pub use self::tracy_client::TracyClient;
pub use self::zone::Zone;
