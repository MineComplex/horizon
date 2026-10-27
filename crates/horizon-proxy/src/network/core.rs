//! The connection of this proxy to Core (Core/Client's Connection): one gRPC channel, the node's
//! session stream (reopened after any failure, with a resync after each reconnection) and unary
//! calls carrying `authorization` and `x-session-id`.
use crate::log;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
use tonic::{
    Request,
    metadata::MetadataValue,
    transport::{Channel, Endpoint},
};

/// Core's protocol (Core/Protocol/src/main/proto).
#[allow(clippy::all, missing_docs)]
pub mod pb {
    tonic::include_proto!("minecomplex.core.v1");
}

/// ProtocolVersion.CURRENT of Core.
pub const PROTOCOL_VERSION: i32 = 1;
/// The deadline of every unary call.
const DEADLINE: Duration = Duration::from_secs(5);
const MIN_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// What the session stream tells the plugin.
pub enum SessionEvent {
    /// Welcome: `resync` when Core has no state of this node.
    Connected {
        resync: bool,
    },
    Event(pb::Event),
    Lost(String),
}

#[derive(Clone)]
pub struct Core {
    channel: Channel,
    token: Arc<str>,
    session: Arc<Mutex<Option<String>>>,
    outbound: Arc<Mutex<Option<mpsc::UnboundedSender<pb::NodeMessage>>>>,
}

macro_rules! client {
    ($name:ident, $module:ident, $client:ident) => {
        pub fn $name(&self) -> pb::$module::$client<Channel> {
            pb::$module::$client::new(self.channel.clone()).max_decoding_message_size(MAX_MESSAGE)
        }
    };
}

impl Core {
    /// A lazy channel: it connects on the first call and after every failure.
    pub fn new(host: &str, port: u16, token: &str) -> Result<Self, String> {
        let endpoint = Endpoint::from_shared(format!("http://{host}:{port}"))
            .map_err(|e| e.to_string())?
            .connect_timeout(DEADLINE)
            // Detects a dead core without waiting for the next call.
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_timeout(Duration::from_secs(5))
            .keep_alive_while_idle(true);
        Ok(Self {
            channel: endpoint.connect_lazy(),
            token: token.into(),
            session: Arc::new(Mutex::new(None)),
            outbound: Arc::new(Mutex::new(None)),
        })
    }

    fn metadata<T>(&self, mut request: Request<T>) -> Request<T> {
        if let Ok(v) = MetadataValue::try_from(format!("Bearer {}", self.token)) {
            request.metadata_mut().insert("authorization", v);
        }
        if let Some(session) = self.session.lock().unwrap().clone() {
            if let Ok(v) = MetadataValue::try_from(session) {
                request.metadata_mut().insert("x-session-id", v);
            }
        }
        request
    }
    /// A unary request with the node's metadata and the deadline.
    pub fn request<T>(&self, message: T) -> Request<T> {
        let mut request = self.metadata(Request::new(message));
        request.set_timeout(DEADLINE);
        request
    }

    pub fn is_connected(&self) -> bool {
        self.session.lock().unwrap().is_some()
    }

    /// Send through the session; dropped while it is closed.
    pub fn send(&self, message: pb::node_message::Message) {
        if let Some(tx) = &*self.outbound.lock().unwrap() {
            let _ = tx.send(pb::NodeMessage {
                message: Some(message),
            });
        }
    }

    client!(accounts, account_service_client, AccountServiceClient);
    client!(actions, action_service_client, ActionServiceClient);
    client!(
        maintenance,
        maintenance_service_client,
        MaintenanceServiceClient
    );
    client!(
        permissions,
        permission_service_client,
        PermissionServiceClient
    );
    client!(presence, presence_service_client, PresenceServiceClient);
    client!(
        punishments,
        punishment_service_client,
        PunishmentServiceClient
    );
    client!(servers, server_service_client, ServerServiceClient);
    client!(store, store_service_client, StoreServiceClient);

    /// The session stream, forever: Hello, then events, reopened with a backoff.
    pub async fn run_session(self, hello: pb::Hello, events: mpsc::UnboundedSender<SessionEvent>) {
        let mut backoff = MIN_BACKOFF;
        loop {
            let reason = match self.open(&hello, &events, &mut backoff).await {
                Ok(()) => "the session is completed by the core".to_owned(),
                Err(e) => e,
            };
            let was_connected = self.session.lock().unwrap().take().is_some();
            *self.outbound.lock().unwrap() = None;
            if was_connected {
                log(&format!("Connection to the core is lost: {reason}"));
                if events.send(SessionEvent::Lost(reason)).is_err() {
                    return;
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }

    async fn open(
        &self,
        hello: &pb::Hello,
        events: &mpsc::UnboundedSender<SessionEvent>,
        backoff: &mut Duration,
    ) -> Result<(), String> {
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(pb::NodeMessage {
            message: Some(pb::node_message::Message::Hello(hello.clone())),
        })
        .map_err(|e| e.to_string())?;
        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        let mut client =
            pb::session_service_client::SessionServiceClient::new(self.channel.clone())
                .max_decoding_message_size(MAX_MESSAGE);
        let mut inbound = client
            .connect(self.metadata(Request::new(stream)))
            .await
            .map_err(|s| s.message().to_owned())?
            .into_inner();
        *self.outbound.lock().unwrap() = Some(tx);
        while let Some(message) = inbound
            .message()
            .await
            .map_err(|s| s.message().to_owned())?
        {
            match message.message {
                Some(pb::core_message::Message::Welcome(welcome)) => {
                    *self.session.lock().unwrap() = Some(welcome.session_id);
                    *backoff = MIN_BACKOFF;
                    log(&format!("Connected to the core as PROXY {}", hello.node_id));
                    let resync = welcome.resync;
                    events
                        .send(SessionEvent::Connected { resync })
                        .map_err(|e| e.to_string())?;
                }
                Some(pb::core_message::Message::Event(event)) => {
                    events
                        .send(SessionEvent::Event(event))
                        .map_err(|e| e.to_string())?;
                }
                Some(pb::core_message::Message::Goodbye(goodbye)) => {
                    log(&format!("Core has closed the session: {}", goodbye.reason));
                }
                None => {}
            }
        }
        Ok(())
    }
}
