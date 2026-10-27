//! Core's permissions as a node computes them (PermissionCalculator, PermissionData,
//! PermissionNode of Core's API): groups inherited through `group.<name>` nodes, the user's own
//! nodes first, wildcards, contexts and expiry.
use super::core::pb;
use crate::plugin::Tristate;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const GROUP: &str = "group.";
pub const DEFAULT_GROUP: &str = "default";

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub key: String,
    pub value: bool,
    /// Millis, 0 for a permanent node.
    pub expiry: i64,
    pub contexts: BTreeMap<String, String>,
}

/// isInt of PermissionNode.
fn is_int(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 9
        && value
            .chars()
            .enumerate()
            .all(|(i, c)| c.is_ascii_digit() || (i == 0 && c == '-' && value.len() > 1))
}

impl Node {
    pub fn new(key: &str, value: bool, expiry: i64, contexts: BTreeMap<String, String>) -> Self {
        let contexts = contexts
            .into_iter()
            .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_lowercase()))
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .collect();
        Self {
            key: normalize_key(key),
            value,
            expiry: expiry.max(0),
            contexts,
        }
    }
    pub fn from_view(view: &pb::PermissionNodeView) -> Self {
        let contexts = view
            .contexts
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self::new(&view.key, view.value, view.expiry, contexts)
    }
    pub fn group(name: &str) -> Self {
        Self::new(&format!("{GROUP}{name}"), true, 0, BTreeMap::new())
    }
    pub fn view(&self) -> pb::PermissionNodeView {
        pb::PermissionNodeView {
            key: self.key.clone(),
            value: self.value,
            expiry: self.expiry,
            contexts: self.contexts.clone().into_iter().collect(),
        }
    }
    fn is_permanent(&self) -> bool {
        self.expiry == 0
    }
    fn is_expired(&self, now: i64) -> bool {
        self.expiry != 0 && self.expiry <= now
    }
    /// appliesIn for the proxy's context (global: no context values).
    fn applies_in(&self, context: &BTreeMap<String, String>) -> bool {
        self.contexts.iter().all(|(k, v)| context.get(k) == Some(v))
    }
    pub fn group_name(&self) -> Option<&str> {
        self.key.strip_prefix(GROUP).filter(|g| !g.is_empty())
    }
    fn is_chat(&self) -> bool {
        ["prefix.", "suffix."].iter().any(|t| {
            self.key.starts_with(t)
                && matches!(self.key.splitn(3, '.').collect::<Vec<_>>()[..], [_, p, _] if is_int(p))
        })
    }
    pub fn same_as(&self, other: &Node) -> bool {
        self.key == other.key && self.contexts == other.contexts
    }
}

/// normalizeKey: lower case, except the text of a prefix or suffix.
fn normalize_key(key: &str) -> String {
    let leading = key.trim_start();
    let lower = leading.to_lowercase();
    for kind in ["prefix.", "suffix."] {
        if lower.starts_with(kind) {
            let parts: Vec<&str> = leading.splitn(3, '.').collect();
            if let [_, priority, text] = parts[..] {
                if is_int(priority) {
                    let priority: i32 = priority.parse().unwrap_or(0);
                    return format!("{kind}{priority}.{text}");
                }
            }
        }
    }
    lower.trim().to_owned()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    pub weight: i32,
    pub nodes: Vec<Node>,
}
impl Group {
    pub fn from_view(view: &pb::PermissionGroupView) -> Self {
        Self {
            name: view.name.to_lowercase(),
            weight: view.weight,
            nodes: view.nodes.iter().map(Node::from_view).collect(),
        }
    }
}

/// The permissions of a player at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {
    permissions: HashMap<String, bool>,
    pub groups: Vec<String>,
}

impl Data {
    /// PermissionData.check: the exact key, then `a.b.*` wildcards from the longest, then `*`.
    pub fn check(&self, permission: &str) -> Tristate {
        let key = permission.to_lowercase();
        let found = |k: &str| self.permissions.get(k).copied();
        let tri = |v: bool| if v { Tristate::True } else { Tristate::False };
        if let Some(v) = found(&key) {
            return tri(v);
        }
        let mut node = key.as_str();
        while let Some(dot) = node.rfind('.').filter(|d| *d > 0) {
            node = &node[..dot];
            if let Some(v) = found(&format!("{node}.*")) {
                return tri(v);
            }
        }
        found("*").map_or(Tristate::Undefined, tri)
    }
}

/// SPECIFIC_FIRST: more contexts first, then temporary before permanent.
fn applicable(nodes: &[Node], context: &BTreeMap<String, String>, now: i64) -> Vec<Node> {
    let mut out: Vec<Node> = nodes
        .iter()
        .filter(|n| !n.is_expired(now) && n.applies_in(context))
        .cloned()
        .collect();
    out.sort_by_key(|n| (std::cmp::Reverse(n.contexts.len()), n.is_permanent()));
    out
}

fn denies(nodes: &[Node], group: &str) -> bool {
    let key = format!("{GROUP}{group}");
    nodes
        .iter()
        .find(|n| n.key == key)
        .is_some_and(|n| !n.value)
}

fn parents(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter(|n| n.value)
        .filter_map(|n| n.group_name())
        .filter(|g| !denies(nodes, g))
        .map(str::to_owned)
        .collect()
}

/// PermissionCalculator.calculate.
pub fn calculate(
    user: &[Node],
    groups: &HashMap<String, Group>,
    context: &BTreeMap<String, String>,
    now: i64,
) -> Data {
    let user = applicable(user, context, now);
    let mut parents_of_user = parents(&user);
    if !denies(&user, DEFAULT_GROUP) && !parents_of_user.iter().any(|g| g == DEFAULT_GROUP) {
        parents_of_user.push(DEFAULT_GROUP.into());
    }
    let mut visited = HashSet::new();
    let mut inherited: Vec<(String, Vec<Node>)> = Vec::new();
    inherit(
        &parents_of_user,
        groups,
        context,
        now,
        &mut visited,
        &mut inherited,
    );
    let mut permissions = HashMap::new();
    let mut put = |nodes: &[Node]| {
        for n in nodes.iter().filter(|n| !n.is_chat()) {
            permissions.entry(n.key.clone()).or_insert(n.value);
        }
    };
    put(&user);
    for (_, nodes) in &inherited {
        put(nodes);
    }
    for (name, _) in &inherited {
        permissions.entry(format!("{GROUP}{name}")).or_insert(true);
    }
    Data {
        permissions,
        groups: inherited.into_iter().map(|(n, _)| n).collect(),
    }
}

fn inherit(
    names: &[String],
    groups: &HashMap<String, Group>,
    context: &BTreeMap<String, String>,
    now: i64,
    visited: &mut HashSet<String>,
    inherited: &mut Vec<(String, Vec<Node>)>,
) {
    let mut found: Vec<&Group> = names
        .iter()
        .filter(|n| !visited.contains(*n))
        .filter_map(|n| groups.get(n))
        .collect();
    // Heaviest first; the sort is stable like Java's.
    found.sort_by_key(|g| std::cmp::Reverse(g.weight));
    for group in found {
        if !visited.insert(group.name.clone()) {
            continue;
        }
        let nodes = applicable(&group.nodes, context, now);
        let next = parents(&nodes);
        inherited.push((group.name.clone(), nodes));
        inherit(&next, groups, context, now, visited, inherited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(key: &str, value: bool) -> Node {
        Node::new(key, value, 0, BTreeMap::new())
    }
    fn group(name: &str, weight: i32, nodes: Vec<Node>) -> (String, Group) {
        (
            name.into(),
            Group {
                name: name.into(),
                weight,
                nodes,
            },
        )
    }

    #[test]
    fn inheritance_wildcards_and_user_first() {
        let groups: HashMap<String, Group> = [
            group("default", 0, vec![node("network.chat", true)]),
            group(
                "admin",
                100,
                vec![node("network.*", true), node("group.mod", true)],
            ),
            group("mod", 50, vec![node("network.maintenance", false)]),
        ]
        .into_iter()
        .collect();
        let global = BTreeMap::new();
        let user = vec![node("group.admin", true), node("Network.Staff", false)];
        let data = calculate(&user, &groups, &global, 0);
        assert_eq!(data.groups, ["admin", "mod", "default"]);
        assert_eq!(data.check("network.staff"), Tristate::False);
        // The heavier group's wildcard is not more specific than mod's exact node.
        assert_eq!(data.check("network.maintenance"), Tristate::False);
        assert_eq!(data.check("network.admin"), Tristate::True);
        assert_eq!(data.check("group.mod"), Tristate::True);
        assert_eq!(data.check("velocity.command.glist"), Tristate::Undefined);
        // Denying the default group and a server context that doesn't apply on the proxy.
        let mut lobby = BTreeMap::new();
        lobby.insert("server".into(), "lobby".into());
        let user = vec![
            node("group.default", false),
            Node::new("velocity.command.glist", true, 0, lobby),
        ];
        let data = calculate(&user, &groups, &global, 0);
        assert!(data.groups.is_empty());
        assert_eq!(data.check("velocity.command.glist"), Tristate::Undefined);
        // Expired nodes are gone.
        let user = vec![Node::new("network.vip", true, 10, BTreeMap::new())];
        assert_eq!(
            calculate(&user, &groups, &global, 5).check("network.vip"),
            Tristate::True
        );
        assert_eq!(
            calculate(&user, &groups, &global, 10).check("network.vip"),
            Tristate::Undefined
        );
    }

    #[test]
    fn keys() {
        assert_eq!(node(" Prefix.10.&a[VIP] ", true).key, "prefix.10.&a[VIP] ");
        assert!(node("prefix.10.&a", true).is_chat());
        assert!(!node("prefix.x.y", true).is_chat());
        assert_eq!(node("GROUP.Admin", true).group_name(), Some("admin"));
    }
}
