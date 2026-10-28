//! A Game's execution role is fixed at construction. Connecting or disconnecting
//! changes `remote`, not this role. A client never simulates a world itself: its worlds run
//! on a server (the integrated server thread until the dedicated binary takes over).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExecutionRole {
    Client,
    IntegratedServer,
    DedicatedServer,
}

impl ExecutionRole {
    /// Only a client owns presentation work (audio, loading UI, chunk meshes).
    pub fn is_client(self) -> bool {
        self == Self::Client
    }

    /// A client's world runs on a server thread.
    pub fn starts_integrated_server(self) -> bool {
        self.is_client()
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionRole::*;

    #[test]
    fn only_clients_start_a_server_thread_for_their_worlds() {
        for (role, client, threaded) in [
            (Client, true, true),
            (IntegratedServer, false, false),
            (DedicatedServer, false, false),
        ] {
            assert_eq!(role.is_client(), client);
            assert_eq!(role.starts_integrated_server(), threaded);
        }
    }
}
