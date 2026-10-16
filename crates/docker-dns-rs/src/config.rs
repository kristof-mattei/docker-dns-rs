use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use hickory_server::proto::rr::Name;
use tracing::{Level, event};
use twistlock::client::ClientCredentialPaths;
use twistlock::config::Endpoint;

#[derive(Clone, Debug)]
pub struct StaticRecord {
    pub name: Name,
    pub addr: IpAddr,
}

pub struct DockerConfig {
    pub docker_host: Endpoint,
    pub cacert: Option<PathBuf>,
    pub client_credentials: Option<ClientCredentialPaths>,
    pub timeout: Duration,
}

pub struct Config {
    pub docker: DockerConfig,
    pub domain: Name,
    pub dns_bind: SocketAddr,
    pub records: Vec<StaticRecord>,
}

impl Config {
    pub fn log(&self) {
        event!(Level::INFO, docker_host = %self.docker.docker_host, "Daemon");
        event!(Level::INFO, domain = %self.domain, "Domain");
        event!(Level::INFO, dns_bind = %self.dns_bind, "DNS Bind Address");

        for r in &self.records {
            event!(Level::INFO, forward = %r.name, reverse = %r.addr, "Static record");
        }
    }
}
