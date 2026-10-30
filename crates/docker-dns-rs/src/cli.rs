use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr as _;
use std::time::Duration;

use clap::{Args, Parser};
use hickory_server::proto::ProtoError;
use hickory_server::proto::rr::Name;
use twistlock::config::{ClientCredentialPaths, Endpoint, ParseError, Tls};

use crate::config::{Config, DockerConfig, StaticRecord};

const DNS_BINDADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 53);

#[derive(Parser, Debug)]
#[command(about, version)]
pub(crate) struct Cli {
    #[arg(
        env,
        default_value = "",
        hide_default_value = true,
        help = "Docker daemon address",
        long,
        value_parser = parse_docker_host
    )]
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
    pub records: Vec<StaticRecord>,

    #[arg(
        env,
        default_value_t = DNS_BINDADDR,
        long,
        help = "Bind address for DNS server",
    )]
    pub bind: SocketAddr,

    #[arg(
        long,
        env,
        help = "Path to the CA certificate used to verify the Docker daemon, enables TLS"
    )]
    pub cacert: Option<PathBuf>,

    #[command(flatten)]
    pub client_credentials: Option<ClientCredentialArgs>,

    #[arg(
        env,
        default_value = "30s",
        long,
        help = "Timeout for a Docker API call",
        value_parser = humantime::parse_duration
    )]
    pub timeout: Duration,
}

// flattened as `Option<Self>`. clap still marks the non-`Option` fields required, hence `required = false`. The group enforces both-or-neither
#[derive(Args, Debug)]
#[group(requires_all = ["client_key", "client_cert"])]
pub(crate) struct ClientCredentialArgs {
    #[arg(
        long,
        env,
        required = false,
        help = "Path to the client private key for mutual TLS with the Docker daemon, enables TLS"
    )]
    pub client_key: PathBuf,

    #[arg(
        long,
        env,
        required = false,
        help = "Path to the client certificate for mutual TLS with the Docker daemon, enables TLS"
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

impl From<Cli> for Config {
    fn from(cli: Cli) -> Self {
        let docker_host = match (cli.cacert, cli.client_credentials) {
            (None, None) => cli.docker_host,
            (ca_cert, client_credentials) => cli.docker_host.with_tls(Tls {
                verify: true,
                ca_cert,
                client_credentials: client_credentials.map(Into::into),
            }),
        };

        let docker = DockerConfig {
            docker_host,
            timeout: cli.timeout,
        };

        Config {
            docker,
            domain: cli.domain,
            dns_bind: cli.bind,
            records: cli.records,
        }
    }
}

fn parse_docker_host(value: &str) -> Result<Endpoint, ParseError> {
    Endpoint::from_str(value)
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

fn parse_record(value: &str) -> Result<StaticRecord, String> {
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

    Ok(StaticRecord { name, addr })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use clap::Parser as _;
    use clap::error::ErrorKind;
    use pretty_assertions::assert_eq;

    use super::Cli;

    // `--docker-host` prevents the tests from parsing `DOCKER_HOST`
    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(
            ["docker-dns-rs", "--docker-host", "tcp://127.0.0.1:2375"]
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

    #[test]
    fn timeout_default() {
        let config = parse(&[]).unwrap();

        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[test]
    fn timeout_with_units() {
        let config = parse(&["--timeout", "1m 30s"]).unwrap();

        assert_eq!(config.timeout, Duration::from_secs(90));
    }

    #[test]
    fn timeout_without_unit_is_rejected() {
        let error = parse(&["--timeout", "30"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::ValueValidation);
    }
}
