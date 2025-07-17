use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerInspect {
    #[expect(dead_code, reason = "Unused")]
    pub name: String,
    #[expect(dead_code, reason = "Unused")]
    pub id: String,
    #[expect(dead_code, reason = "Unused")]
    pub config: ContainerConfig,
    pub state: ContainerState,
    pub network_settings: ContainerNetworkSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerConfig {
    #[expect(dead_code, reason = "Unused")]
    pub hostname: String,
    #[expect(dead_code, reason = "Unused")]
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerState {
    pub running: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerNetworkSettings {
    pub networks: HashMap<String, ContainerNetwork>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerNetwork {
    #[serde(rename(deserialize = "IPAddress"))]
    pub ip_address: String,
}
