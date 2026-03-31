use ipnet::IpNet;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[cfg_attr(not(test), expect(unused, reason = "Library Code"))]
pub struct NetworkInspect {
    pub id: Box<str>,
    pub name: Box<str>,
    #[serde(rename = "IPAM")]
    pub ipam: NetworkIpam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[cfg_attr(not(test), expect(unused, reason = "Library Code"))]
pub struct NetworkIpam {
    pub config: Vec<NetworkIpamConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[cfg_attr(not(test), expect(unused, reason = "Library Code"))]
pub struct NetworkIpamConfig {
    pub subnet: Option<IpNet>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[expect(unused, reason = "Library Code")]
pub struct NetworkSummary {
    pub id: Box<str>,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{NetworkInspect, NetworkIpamConfig};

    fn parse_config(json: &str) -> Result<NetworkIpamConfig, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn parse_inspect(json: &str) -> Result<NetworkInspect, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn ipam_config_ipv4_subnet() {
        let config = parse_config(r#"{"Subnet":"192.168.1.0/24"}"#).unwrap();

        assert_eq!(config.subnet, Some("192.168.1.0/24".parse().unwrap()));
    }

    #[test]
    fn ipam_config_ipv6_subnet() {
        let config = parse_config(r#"{"Subnet":"2001:db8::/32"}"#).unwrap();

        assert_eq!(config.subnet, Some("2001:db8::/32".parse().unwrap()));
    }

    #[test]
    fn ipam_config_subnet_null() {
        let config = parse_config(r#"{"Subnet":null}"#).unwrap();

        assert_eq!(config.subnet, None);
    }

    #[test]
    fn ipam_config_subnet_absent() {
        let config = parse_config("{}").unwrap();

        assert_eq!(config.subnet, None);
    }

    #[test]
    fn ipam_config_invalid_subnet_is_error() {
        parse_config(r#"{"Subnet":"not-a-cidr"}"#).unwrap_err();
    }

    #[test]
    fn network_inspect_ipam_rename() {
        // Verifies the IPAM field (all-caps) deserializes correctly and
        // that multi-config entries are all captured.
        let inspect = parse_inspect(
            r#"{
                "Id": "abc123",
                "Name": "my-network",
                "IPAM": {
                    "Config": [
                        {"Subnet": "10.0.0.0/8"},
                        {"Subnet": "fd00::/8"}
                    ]
                }
            }"#,
        )
        .unwrap();

        assert_eq!(inspect.id.as_ref(), "abc123");
        assert_eq!(inspect.name.as_ref(), "my-network");
        assert_eq!(inspect.ipam.config.len(), 2);
        assert_eq!(
            inspect.ipam.config[0].subnet,
            Some("10.0.0.0/8".parse().unwrap())
        );
        assert_eq!(
            inspect.ipam.config[1].subnet,
            Some("fd00::/8".parse().unwrap())
        );
    }

    #[test]
    fn network_inspect_empty_ipam_config() {
        let inspect = parse_inspect(
            r#"{
                "Id": "def456",
                "Name": "empty-net",
                "IPAM": {"Config": []}
            }"#,
        )
        .unwrap();

        assert_eq!(inspect.ipam.config.len(), 0);
    }
}
