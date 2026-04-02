use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use hickory_server::proto::ProtoError;
use hickory_server::proto::rr::Name;
use tracing::{Level, event};

const DEFAULT_DOCKER_HOST: &str = "/var/run/docker.sock";
const DNS_BINDADDR: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 53);

#[derive(Clone, Debug)]
pub enum RawEndpoint {
    Direct(http::Uri),
    #[cfg(not(target_os = "windows"))]
    Socket(PathBuf),
}

impl std::fmt::Display for RawEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            RawEndpoint::Direct(ref uri) => write!(f, "{}", uri),
            #[cfg(not(target_os = "windows"))]
            RawEndpoint::Socket(ref socket) => write!(f, "{}", socket.display()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawRecord {
    pub name: Name,
    pub addr: IpAddr,
}

#[derive(Parser, Debug)]
pub struct RawConfig {
    #[arg(env, default_value = DEFAULT_DOCKER_HOST, value_parser = parse_docker, help = "Path to docker TCP/UNIX socket", long="docker")]
    pub docker_host: RawEndpoint,

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

    #[arg(
        env = "timeout",
        default_value = "30",
        long,
        help = "Docker socket timeout, in seconds, only used when connecting over tcp",
        value_parser = parse_duration
    )]
    pub timeout: Duration,
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

fn parse_duration(value: &str) -> Result<Duration, String> {
    let seconds = value
        .parse()
        .map_err(|error| format!("Could not parse `{}`: {}", value, error))?;

    Ok(Duration::from_secs(seconds))
}

fn parse_docker(value: &str) -> Result<RawEndpoint, String> {
    const TCP_START: &str = "tcp://";

    let endpoint = if let Some(stripped) = value.strip_prefix(TCP_START) {
        let uri = format!("http://{}", stripped);

        RawEndpoint::Direct(
            uri.parse()
                .map_err(|error| format!("Failed to convert `{}` to URL: {}", uri, error))?,
        )
    } else {
        #[cfg(target_os = "windows")]
        {
            return Err(format!(
                "On Windows, you can connect to docker with tcp. You tried to connect with \"{}\"",
                value
            ));
        }

        #[cfg(not(target_os = "windows"))]
        {
            if value.is_empty() {
                return Err("Docker socket cannot be empty".to_owned());
            }

            RawEndpoint::Socket(PathBuf::from(value))
        }
    };

    Ok(endpoint)
}

fn parse_domain(raw_domain: &str) -> Result<Name, String> {
    let mut domain: Name = raw_domain.parse()?;
    domain.set_fqdn(true);

    Ok(domain)
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
