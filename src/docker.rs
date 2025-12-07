use hashbrown::HashMap;
use serde::Deserialize;

pub mod config;
pub mod daemon;
pub mod monitor;

#[derive(Deserialize, Debug)]
#[expect(unused, reason = "Unused, for now")]
pub struct Event {
    #[serde(rename(deserialize = "Type"))]
    r#type: EventType,
    #[serde(rename(deserialize = "Action"))]
    action: EventAction,
    #[serde(rename(deserialize = "Actor"))]
    actor: EventActor,
    scope: EventScope,
    time: u64,
    #[serde(rename(deserialize = "timeNano"))]
    time_nano: u64,
}

#[derive(Deserialize, Debug)]
enum EventType {
    #[serde(rename(deserialize = "builder"))]
    Builder,
    #[serde(rename(deserialize = "config"))]
    Config,
    #[serde(rename(deserialize = "container"))]
    Container,
    #[serde(rename(deserialize = "daemon"))]
    Daemon,
    #[serde(rename(deserialize = "image"))]
    Image,
    #[serde(rename(deserialize = "network"))]
    Network,
    #[serde(rename(deserialize = "node"))]
    Node,
    #[serde(rename(deserialize = "plugin"))]
    Plugin,
    #[serde(rename(deserialize = "secret"))]
    Secret,
    #[serde(rename(deserialize = "service"))]
    Service,
    #[serde(rename(deserialize = "volume"))]
    Volume,
}

type EventAction = String;

#[derive(Deserialize, Debug)]
struct EventActor {
    #[serde(rename(deserialize = "ID"))]
    id: String,
    #[serde(rename(deserialize = "Attributes"))]
    attributes: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
enum EventScope {
    #[serde(rename(deserialize = "local"))]
    Local,
    #[serde(rename(deserialize = "swarm"))]
    Swarm,
}
