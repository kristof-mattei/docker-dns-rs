use hashbrown::HashMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerInspect {
    #[expect(unused, reason = "Library Code")]
    pub name: Box<str>,
    pub id: Box<str>,
    #[expect(unused, reason = "Library Code")]
    pub config: ContainerConfig,
    pub state: ContainerState,
    pub network_settings: ContainerNetworkSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerConfig {
    #[expect(unused, reason = "Library Code")]
    pub hostname: Box<str>,
    #[expect(unused, reason = "Library Code")]
    pub labels: HashMap<Box<str>, Box<str>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerState {
    pub running: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerNetworkSettings {
    pub networks: HashMap<Box<str>, ContainerNetwork>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerNetwork {
    #[serde(rename(deserialize = "IPAddress"))]
    pub ip_address: Box<str>,
}
