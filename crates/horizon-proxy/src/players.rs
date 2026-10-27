//! The players on the proxy (VelocityServer.connectionsByUuid): one entry per UUID, with the
//! server each is on and a handle that reaches their session (message, kick, send).
use horizon_protocol::packets::login::Property;
use horizon_world::nbt::Tag;
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
};

/// Who a connection is: the profile and where it comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub uuid: u128,
    pub name: String,
    pub address: SocketAddr,
    pub properties: Vec<Property>,
}

/// What another task asks of a player's session.
#[derive(Clone, Debug)]
pub enum Order {
    Kick(Tag),
    /// Connect to a server (createConnectionRequest).
    Send(String),
    Message(Tag),
    Title {
        title: Tag,
        subtitle: Tag,
    },
    /// New profile properties (a new skin), used from the next server on.
    Properties(Vec<Property>),
}

/// Delivers an order to a session; false once it is gone.
pub type Mailbox = Arc<dyn Fn(Order) -> bool + Send + Sync>;

struct Entry {
    info: PlayerInfo,
    server: Option<String>,
    mailbox: Mailbox,
    /// Which login of this UUID the entry belongs to.
    id: u64,
}

/// A player as other tasks see them.
#[derive(Clone)]
pub struct PlayerHandle {
    pub info: PlayerInfo,
    pub server: Option<String>,
    mailbox: Mailbox,
}
impl PlayerHandle {
    pub fn order(&self, order: Order) -> bool {
        (self.mailbox)(order)
    }
    pub fn message(&self, message: Tag) {
        self.order(Order::Message(message));
    }
    pub fn disconnect(&self, reason: Tag) {
        self.order(Order::Kick(reason));
    }
    pub fn connect(&self, server: &str) {
        self.order(Order::Send(server.to_owned()));
    }
    pub fn ip(&self) -> IpAddr {
        self.info.address.ip()
    }
}

#[derive(Default)]
pub struct Players {
    entries: Mutex<(BTreeMap<u128, Entry>, u64)>,
}

/// A player's place in [`Players`]; dropping it removes them.
pub struct Ticket {
    players: Arc<Players>,
    uuid: u128,
    id: u64,
}

impl Players {
    /// registerConnection: `None` when a player with this UUID or name is online and
    /// `kick_existing` is off; with it on, the other one is kicked.
    pub fn join(
        self: &Arc<Self>,
        info: PlayerInfo,
        mailbox: Mailbox,
        kick_existing: Option<Tag>,
    ) -> Option<Ticket> {
        let mut guard = self.entries.lock().unwrap();
        let (entries, next) = &mut *guard;
        let existing: Vec<u128> = entries
            .iter()
            .filter(|(u, e)| **u == info.uuid || e.info.name.eq_ignore_ascii_case(&info.name))
            .map(|(u, _)| *u)
            .collect();
        if !existing.is_empty() {
            let reason = kick_existing?;
            for u in existing {
                if let Some(e) = entries.remove(&u) {
                    (e.mailbox)(Order::Kick(reason.clone()));
                }
            }
        }
        *next += 1;
        let uuid = info.uuid;
        entries.insert(
            uuid,
            Entry {
                info,
                server: None,
                mailbox,
                id: *next,
            },
        );
        Some(Ticket {
            players: self.clone(),
            uuid,
            id: *next,
        })
    }

    pub fn count(&self) -> usize {
        self.entries.lock().unwrap().0.len()
    }

    /// Connections from one address.
    pub fn count_ip(&self, ip: IpAddr) -> usize {
        let guard = self.entries.lock().unwrap();
        guard
            .0
            .values()
            .filter(|e| e.info.address.ip() == ip)
            .count()
    }

    /// Players on a server.
    pub fn count_on(&self, server: &str) -> usize {
        let guard = self.entries.lock().unwrap();
        guard
            .0
            .values()
            .filter(|e| e.server.as_deref() == Some(server))
            .count()
    }

    /// Up to `n` names, for the server list's sample.
    pub fn sample(&self, n: usize) -> Vec<(u128, String)> {
        let guard = self.entries.lock().unwrap();
        guard
            .0
            .iter()
            .take(n)
            .map(|(u, e)| (*u, e.info.name.clone()))
            .collect()
    }

    /// Server name to the names of the players on it.
    pub fn by_server(&self) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in self.entries.lock().unwrap().0.values() {
            if let Some(server) = &e.server {
                out.entry(server.clone())
                    .or_default()
                    .push(e.info.name.clone());
            }
        }
        out.values_mut()
            .for_each(|names| names.sort_by_key(|n| n.to_lowercase()));
        out
    }

    fn handle(e: &Entry) -> PlayerHandle {
        PlayerHandle {
            info: e.info.clone(),
            server: e.server.clone(),
            mailbox: e.mailbox.clone(),
        }
    }
    pub fn get(&self, uuid: u128) -> Option<PlayerHandle> {
        self.entries.lock().unwrap().0.get(&uuid).map(Self::handle)
    }
    pub fn find(&self, name: &str) -> Option<PlayerHandle> {
        let guard = self.entries.lock().unwrap();
        guard
            .0
            .values()
            .find(|e| e.info.name.eq_ignore_ascii_case(name))
            .map(Self::handle)
    }
    pub fn all(&self) -> Vec<PlayerHandle> {
        self.entries
            .lock()
            .unwrap()
            .0
            .values()
            .map(Self::handle)
            .collect()
    }

    /// Hand an order to the player with this name; false when nobody has it.
    pub fn order(&self, name: &str, order: Order) -> bool {
        self.find(name).is_some_and(|p| p.order(order))
    }

    /// Hand an order (made per player) to everyone.
    pub fn order_all(&self, make: impl Fn() -> Order) {
        for p in self.all() {
            p.order(make());
        }
    }
}

impl Ticket {
    pub fn set_server(&self, server: &str) {
        let mut guard = self.players.entries.lock().unwrap();
        if let Some(e) = guard.0.get_mut(&self.uuid).filter(|e| e.id == self.id) {
            e.server = Some(server.to_owned());
        }
    }
    pub fn set_properties(&self, properties: &[Property]) {
        let mut guard = self.players.entries.lock().unwrap();
        if let Some(e) = guard.0.get_mut(&self.uuid).filter(|e| e.id == self.id) {
            e.info.properties = properties.to_vec();
        }
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        let mut guard = self.players.entries.lock().unwrap();
        if guard.0.get(&self.uuid).is_some_and(|e| e.id == self.id) {
            guard.0.remove(&self.uuid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn info(uuid: u128, name: &str) -> PlayerInfo {
        PlayerInfo {
            uuid,
            name: name.into(),
            address: "127.0.0.1:1".parse().unwrap(),
            properties: vec![],
        }
    }

    #[test]
    fn duplicates_are_refused_or_kicked() {
        let players = Arc::new(Players::default());
        let mailbox = |tx: std::sync::mpsc::Sender<Order>| -> Mailbox {
            let tx = Mutex::new(tx);
            Arc::new(move |o| tx.lock().unwrap().send(o).is_ok())
        };
        let (tx, rx) = channel();
        let first = players.join(info(1, "Steve"), mailbox(tx), None).unwrap();
        first.set_server("lobby");
        let (tx2, _rx2) = channel();
        assert!(
            players
                .join(info(2, "steve"), mailbox(tx2.clone()), None)
                .is_none()
        );
        let second = players
            .join(info(2, "steve"), mailbox(tx2), Some(Tag::Byte(0)))
            .unwrap();
        assert!(matches!(rx.try_recv(), Ok(Order::Kick(_))));
        // The kicked session's ticket no longer owns an entry.
        drop(first);
        assert_eq!(players.count(), 1);
        second.set_server("pvp");
        assert_eq!(players.by_server()["pvp"], ["steve"]);
        assert_eq!(players.count_on("pvp"), 1);
        assert_eq!(players.count_ip("127.0.0.1".parse().unwrap()), 1);
        assert_eq!(
            players.find("STEVE").unwrap().server.as_deref(),
            Some("pvp")
        );
        drop(second);
        assert_eq!(players.count(), 0);
    }
}
