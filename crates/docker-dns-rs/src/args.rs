use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use hickory_server::proto::rr::Name;

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
            RawEndpoint::Socket(ref socket) => write!(f, "{}", socket.display()),
        }
    }
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
        help = "Docker socket timeout, in milliseconds, only used when connecting over tcp",
        value_parser = parse_duration
    )]
    pub timeout: Duration,
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    let milliseconds = value
        .parse()
        .map_err(|error| format!("Could not parse `{}`: {}", value, error))?;

    Ok(Duration::from_millis(milliseconds))
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
        return Err(eyre::Report::msg(format!(
            "On Windows, you can connect to docker with tcp. You tried to connect with \"{}\"",
            docker_socket_or_uri
        )));

        if value.is_empty() {
            return Err("Docker socket cannot be empty".to_owned());
        }

        // we're connecting over a socket, so the uri is localhost
        #[cfg(not(target_os = "windows"))]
        RawEndpoint::Socket(PathBuf::from(value))
    };

    Ok(endpoint)
}

fn parse_domain(raw_domain: &str) -> Result<Name, String> {
    let mut domain: Name = raw_domain.parse()?;
    domain.set_fqdn(true);

    Ok(domain)
}
