use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr as _;
use std::time::Duration;

use clap::{Args, Parser};
use color_eyre::eyre;
use hickory_server::proto::ProtoError;
use hickory_server::proto::rr::Name;
use tracing::{Level, event};
use twistlock::client::ClientCredentialPaths;
use twistlock::config::Endpoint;

const DEFAULT_DOCKER_HOST: &str = "/var/run/docker.sock";
const DNS_BINDADDR: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 53);

#[derive(Clone, Debug)]
pub struct RawRecord {
    pub name: Name,
    pub addr: IpAddr,
}

#[derive(Parser, Debug)]
pub struct RawConfig {
    #[arg(env, default_value = DEFAULT_DOCKER_HOST, value_parser = parse_docker_host, help = "Path to docker TCP/UNIX socket", long="docker")]
    pub docker_host: Endpoint,

    #[arg(
        env,
        default_value = "docker",
        help = "Base domain name for registered services",
        long,
        value_parser = parse_domain
    )]
    pub domain: Name,

    #[arg(
        env = "RECORDS",
        help = "Add a static record as `name:ip` (IPv4) or `name:[ipv6]` (IPv6), separated by commas or repeated flags",
        long = "record",
        name = "RECORD",
        value_parser = parse_record,
        value_delimiter = ',',
        action = clap::ArgAction::Append,
    )]
    pub records: Vec<RawRecord>,

    #[arg(
        env,
        default_value_t = DNS_BINDADDR,
        long,
        help = "Bind address for DNS server",
    )]
    pub dns_bind: SocketAddr,

    #[clap(long, env = "CA")]
    pub cacert: Option<PathBuf>,

    #[command(flatten)]
    pub client_credentials: Option<ClientCredentialArgs>,

    #[arg(
        env = "timeout",
        default_value = "30",
        long,
        help = "Docker socket timeout, in seconds, only used when connecting over tcp",
        value_parser = parse_duration
    )]
    pub timeout: Duration,
}

// flattened as `Option<Self>`. clap still marks the non-`Option` fields required, hence `required = false`. The group enforces both-or-neither
#[derive(Args, Debug)]
#[group(requires_all = ["client_key", "client_cert"])]
pub struct ClientCredentialArgs {
    #[arg(
        long,
        env,
        required = false,
        help = "Path to the client private key for mutual TLS with the Docker daemon"
    )]
    pub client_key: PathBuf,

    #[arg(
        long,
        env,
        required = false,
        help = "Path to the client certificate for mutual TLS with the Docker daemon"
    )]
    pub client_cert: PathBuf,
}

impl From<ClientCredentialArgs> for ClientCredentialPaths {
    fn from(args: ClientCredentialArgs) -> Self {
        ClientCredentialPaths {
            key: args.client_key,
            cert: args.client_cert,
        }
    }
}

impl RawConfig {
    pub fn print(&self) {
        event!(Level::INFO, docker_host = %self.docker_host, "Daemon");
        event!(Level::INFO, domain = %self.domain, "Domain");
        event!(Level::INFO, dns_bind = %self.dns_bind, "DNS Bind Address");

        for r in &self.records {
            event!(Level::INFO, forward = %r.name, reverse = %r.addr, "Static record");
        }
    }
}

fn parse_docker_host(value: &str) -> Result<Endpoint, String> {
    Endpoint::from_str(value)
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    let seconds = value
        .parse()
        .map_err(|error| format!("Could not parse `{}`: {}", value, error))?;

    Ok(Duration::from_secs(seconds))
}

fn parse_domain(raw_domain: &str) -> Result<Name, String> {
    match raw_domain.parse::<Name>() {
        Ok(mut domain) => {
            domain.set_fqdn(true);

            Ok(domain)
        },
        Err(error) => Err(format!(
            "Failed convert `{}` to a FQDN Domain name, error: {:?}",
            raw_domain, error
        )),
    }
}

fn parse_record(value: &str) -> Result<RawRecord, String> {
    let (name_str, addr_str) = value
        .split_once(':')
        .ok_or_else(|| format!("expected `name:ip` or `name:[ipv6]`, got `{}`", value))?;

    let addr: IpAddr = if addr_str.starts_with('[') && addr_str.ends_with(']') {
        #[expect(
            clippy::string_slice,
            reason = "We've asserted that the first and last character are non-composite"
        )]
        addr_str[1..addr_str.len() - 1]
            .parse::<Ipv6Addr>()
            .map(Into::into)
    } else {
        (addr_str).parse::<Ipv4Addr>().map(Into::into)
    }
    .map_err(|error| error.to_string())?;

    let mut name: Name = name_str
        .parse()
        .map_err(|error: ProtoError| error.to_string())?;
    name.set_fqdn(true);

    Ok(RawRecord { name, addr })
}

pub struct DockerConfig {
    pub docker_host: Endpoint,
    pub cacert: Option<PathBuf>,
    pub client_credentials: Option<ClientCredentialPaths>,
    pub timeout: Duration,
}

pub struct AppConfig {
    pub docker_config: DockerConfig,
    pub domain: Name,
    pub dns_bind: SocketAddr,
    pub records: Vec<RawRecord>,
}

impl AppConfig {
    pub fn build() -> Result<AppConfig, eyre::Report> {
        let raw_config = RawConfig::try_parse()?;

        raw_config.print();

        let docker_config = DockerConfig {
            docker_host: raw_config.docker_host,
            cacert: raw_config.cacert,
            client_credentials: raw_config.client_credentials.map(Into::into),
            timeout: raw_config.timeout,
        };

        Ok(AppConfig {
            docker_config,
            domain: raw_config.domain,
            dns_bind: raw_config.dns_bind,
            records: raw_config.records,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser as _;
    use clap::error::ErrorKind;
    use pretty_assertions::assert_eq;

    use super::RawConfig;

    // without `--docker`, clap parses `DEFAULT_DOCKER_HOST`, a unix socket, which `Endpoint` rejects on Windows (no `Socket` variant there). tcp parses on every platform
    fn parse(args: &[&str]) -> Result<RawConfig, clap::Error> {
        RawConfig::try_parse_from(
            ["docker-dns-rs", "--docker", "tcp://127.0.0.1:2375"]
                .into_iter()
                .chain(args.iter().copied()),
        )
    }

    #[test]
    fn client_credentials_absent() {
        let config = parse(&[]).unwrap();

        assert!(config.client_credentials.is_none());
    }

    #[test]
    fn client_credentials_both_present() {
        let config = parse(&[
            "--client-key",
            "/certs/key.pem",
            "--client-cert",
            "/certs/cert.pem",
        ])
        .unwrap();

        let credentials = config.client_credentials.unwrap();

        assert_eq!(credentials.client_key, Path::new("/certs/key.pem"));
        assert_eq!(credentials.client_cert, Path::new("/certs/cert.pem"));
    }

    #[test]
    fn client_key_without_client_cert_is_rejected() {
        let error = parse(&["--client-key", "/certs/key.pem"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn client_cert_without_client_key_is_rejected() {
        let error = parse(&["--client-cert", "/certs/cert.pem"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    }
}
