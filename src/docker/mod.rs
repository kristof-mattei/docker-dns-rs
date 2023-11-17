use std::collections::HashMap;

pub mod docker;
pub mod docker_config;
pub mod docker_monitor;

#[derive(serde::Deserialize, Debug)]
pub struct DockerEvent {
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

#[derive(serde::Deserialize, Debug)]
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

#[derive(serde::Deserialize, Debug)]
struct EventActor {
    #[serde(rename(deserialize = "ID"))]
    id: String,
    #[serde(rename(deserialize = "Attributes"))]
    attributes: HashMap<String, String>,
}

#[derive(serde::Deserialize, Debug)]
enum EventScope {
    #[serde(rename(deserialize = "local"))]
    Local,
    #[serde(rename(deserialize = "swarm"))]
    Swarm,
}
