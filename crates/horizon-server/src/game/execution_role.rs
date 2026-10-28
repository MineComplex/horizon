//! A Game's execution role is fixed at construction. Connecting or disconnecting
//! changes `remote`, not this role. A client never simulates a world itself: its
//! worlds run on a server (`horizon-server`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExecutionRole {
    Client,
    DedicatedServer,
}

impl ExecutionRole {
    /// Only a client owns presentation work (audio, loading UI, chunk meshes).
    pub fn is_client(self) -> bool {
        self == Self::Client
    }
}
