use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use hashbrown::HashMap;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerInspect {
    pub name: Box<str>,
    #[expect(unused, reason = "Library Code")]
    pub id: Box<str>,
    pub config: ContainerConfig,
    #[expect(unused, reason = "Library Code")]
    pub state: ContainerState,
    pub network_settings: ContainerNetworkSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerConfig {
    #[expect(unused, reason = "Library Code")]
    pub hostname: Box<str>,
    pub labels: HashMap<Box<str>, Box<str>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[expect(unused, reason = "Library Code")]
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
    #[serde(
        rename(deserialize = "IPAddress"),
        deserialize_with = "deserialize_empty_as_none"
    )]
    pub ip_address: Option<Ipv4Addr>,

    #[serde(
        rename(deserialize = "GlobalIPv6Address"),
        deserialize_with = "deserialize_empty_as_none"
    )]
    pub global_ipv6_address: Option<Ipv6Addr>,
}

// Docker passes empty strings if value absent
fn deserialize_empty_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    // both absent and "" are None
    match Option::<&str>::deserialize(deserializer)? {
        None | Some("") => Ok(None),
        Some(s) => T::from_str(s).map(Some).map_err(Error::custom),
    }
}
