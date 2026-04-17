use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr as _;
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre;
use hickory_server::proto::ProtoError;
use hickory_server::proto::rr::Name;
use tracing::{Level, event};
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

    #[clap(long, env)]
    pub client_key: Option<PathBuf>,

    #[clap(long, env)]
    pub client_cert: Option<PathBuf>,

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
    pub client_key: Option<PathBuf>,
    pub client_cert: Option<PathBuf>,
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
            client_key: raw_config.client_key,
            client_cert: raw_config.client_cert,
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
