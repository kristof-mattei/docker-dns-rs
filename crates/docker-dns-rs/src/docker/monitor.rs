use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, LazyLock};

use color_eyre::eyre;
use hashbrown::HashMap;
use hickory_server::proto::rr::Name;
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use itertools::Either;
use regex::Regex;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::docker::daemon::Daemon;
use crate::docker::{Event, EventType};
use crate::models::container_inspect::{
    ContainerInspect, ContainerNetwork, ContainerNetworkSettings,
};
use crate::table::AuthorityWrapper;

static RE_VALIDNAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w\d.-]").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkIps {
    V4Only(Ipv4Addr),
    V6Only(Ipv6Addr),
    Both(Ipv4Addr, Ipv6Addr),
}

impl NetworkIps {
    fn from_network(network: &ContainerNetwork) -> Option<Self> {
        match (network.ip_address, network.global_ipv6_address) {
            (Some(v4), Some(v6)) => Some(Self::Both(v4, v6)),
            (Some(v4), None) => Some(Self::V4Only(v4)),
            (None, Some(v6)) => Some(Self::V6Only(v6)),
            (None, None) => None,
        }
    }

    fn ips(self) -> impl Iterator<Item = IpAddr> {
        match self {
            Self::V4Only(v4) => [Some(IpAddr::V4(v4)), None],
            Self::V6Only(v6) => [None, Some(IpAddr::V6(v6))],
            Self::Both(v4, v6) => [Some(IpAddr::V4(v4)), Some(IpAddr::V6(v6))],
        }
        .into_iter()
        .flatten()
    }
}

struct ContainerState {
    names: Arc<[Name]>,
    /// `network_name` to IPs.
    networks: HashMap<Box<str>, NetworkIps>,
}

pub struct Monitor {
    authority_wrapper: AuthorityWrapper,
    docker: Arc<Daemon>,
    domain: Name,
    /// `container_id` to `ContainerState`.
    /// Invariant: names and network entries are always co-located, you cannot have
    /// a network entry without its accompanying names.
    containers: Mutex<HashMap<Box<str>, ContainerState>>,
}

fn append_compose_names(
    mut names: Vec<Box<str>>,
    bag: &HashMap<Box<str>, Box<str>>,
) -> Vec<Box<str>> {
    let instance = bag
        .get("com.docker.compose.container-number")
        .and_then(|n| match n.parse::<usize>() {
            Ok(v) => Some(v),
            Err(error) => {
                event!(
                    Level::WARN,
                    label_value = %n,
                    ?error,
                    "Invalid value for com.docker.compose.container-number label, defaulting to 1"
                );
                None
            },
        })
        .unwrap_or(1);

    let service = bag.get("com.docker.compose.service");
    let project = bag.get("com.docker.compose.project");

    if let (Some(service), Some(project)) = (service, project) {
        names.push(format!("{}.{}.{}", instance, service, project).into_boxed_str());

        if instance == 1 {
            names.push(format!("{}.{}", service, project).into_boxed_str());
        }
    }

    names
}

fn get_all_names_from_event(event: &Event) -> Vec<Box<str>> {
    let mut names = vec![];

    if let Some(sanitized_name) = event
        .actor
        .attributes
        .get("name")
        .map(|name| RE_VALIDNAME.replace_all(name, "").to_string())
    {
        names.push(sanitized_name.into_boxed_str());
    }

    append_compose_names(names, &event.actor.attributes)
}

fn get_all_names_from_inspect(container_inspect: &ContainerInspect) -> Vec<Box<str>> {
    let mut names = vec![];

    let sanitized_name = RE_VALIDNAME.replace_all(&container_inspect.name, "");
    names.push(sanitized_name.into_owned().into_boxed_str());

    append_compose_names(names, &container_inspect.config.labels)
}

fn to_full_names(raw: Vec<Box<str>>, domain: &Name) -> Arc<[Name]> {
    raw.into_iter()
        .filter_map(|name| {
            let parsed = match name.parse::<Name>() {
                Ok(p) => p,
                Err(error) => {
                    event!(Level::WARN, %name, ?error, "Failed to parse container name as DNS name, skipping");
                    return None;
                },
            };
            match parsed.append_domain(domain) {
                Ok(full) => Some(full),
                Err(error) => {
                    event!(Level::WARN, %name, ?error, "Failed to append domain to container name, skipping");
                    None
                },
            }
        })
        .collect()
}

fn parse_subnet_ipv4(net: Ipv4Net) -> impl Iterator<Item = IpNet> {
    const OCTET_BOUNDARY: u8 = 8;

    // Round prefix up to the next octet boundary (min /8).
    let boundary = net.prefix_len().max(1).div_ceil(OCTET_BOUNDARY) * OCTET_BOUNDARY;
    net.subnets(boundary).into_iter().flatten().map(IpNet::V4)
}

fn parse_subnet_ipv6(net: Ipv6Net) -> impl Iterator<Item = IpNet> {
    const NIBBLE_BOUNDARY: u8 = 4;

    // Round prefix up to the next nibble boundary (min /4).
    let boundary = net.prefix_len().max(1).div_ceil(NIBBLE_BOUNDARY) * NIBBLE_BOUNDARY;
    net.subnets(boundary).into_iter().flatten().map(IpNet::V6)
}

fn zone_name(network: IpNet) -> Option<Name> {
    match network {
        IpNet::V4(net) => {
            static DOMAIN: LazyLock<Name> =
                LazyLock::new(|| Name::from_str_relaxed("in-addr.arpa").unwrap());

            let byte_count = usize::from(net.prefix_len()) / 8;

            let labels = net
                .addr()
                .octets()
                .into_iter()
                .take(byte_count)
                .rev()
                .map(|n| n.to_string());

            Name::from_labels(labels).ok()?.append_domain(&DOMAIN).ok()
        },
        IpNet::V6(net) => {
            static DOMAIN: LazyLock<Name> =
                LazyLock::new(|| Name::from_str_relaxed("ip6.arpa").unwrap());

            let nibble_count = usize::from(net.prefix_len()) / 4;
            let octets = net.addr().octets();
            let labels = (0..nibble_count).rev().map(move |i| {
                let nibble = if i % 2 == 0 {
                    octets[i / 2] >> 4
                } else {
                    octets[i / 2] & 0x0f
                };
                format!("{nibble:x}")
            });

            Name::from_labels(labels).ok()?.append_domain(&DOMAIN).ok()
        },
    }
}

/// Expand an `IpNet` into `(IpNet, Name)` pairs, one per reverse zone.
///
/// ## IPv4:
/// Zones are aligned to /8 octet boundaries: a /8, /16, or /24 prefix produces 1 zone.
///
/// Any prefix length that is not cleanly divisible by 8 is rounded up to the next octet boundary and then unrolled.
///
/// More generally speaking: a /12 (between /8 and /16) is rounded up to /16 and produces 2^(16-12) = 16 zones.
///
/// E.g. `172.16.66.123/18` becomes `172.16.64.0/24..=172.16.127.0/24` = 64 new subnets.
///
///
/// ## IPv6:
/// Zones are at the /4 nibble boundary with the same rounding logic as IPv4.
#[cfg_attr(not(test), expect(unused, reason = "Library Code"))]
fn parse_subnet(network: IpNet) -> impl Iterator<Item = (IpNet, Name)> {
    let iter = match network {
        IpNet::V4(net) => Either::Left(parse_subnet_ipv4(net)),
        IpNet::V6(net) => Either::Right(parse_subnet_ipv6(net)),
    };

    iter.filter_map(|n| {
        let Some(name) = zone_name(n) else {
            event!(Level::WARN, %n, "Failed to compute reverse zone name for subnet, skipping");

            return None;
        };

        Some((n, name))
    })
}

impl Monitor {
    pub fn new(docker: Arc<Daemon>, authority_wrapper: AuthorityWrapper, domain: Name) -> Self {
        Self {
            authority_wrapper,
            docker,
            domain,
            containers: Mutex::new(HashMap::new()),
        }
    }

    async fn register_container_networks(
        &self,
        container_id: &str,
        full_names: Arc<[Name]>,
        network_settings: ContainerNetworkSettings,
    ) {
        let mut containers = self.containers.lock().await;

        let container_state =
            containers
                .entry_ref(container_id)
                .or_insert_with(|| ContainerState {
                    names: full_names,
                    networks: HashMap::new(),
                });

        for (network_name, network) in network_settings.networks {
            let Some(network_ips) = NetworkIps::from_network(&network) else {
                continue;
            };

            for ip in network_ips.ips() {
                for name in &*container_state.names {
                    self.authority_wrapper.add(name, ip).await;
                }
            }

            container_state.networks.insert(network_name, network_ips);
        }
    }

    async fn handle_container_rename(&self, event: Event) {
        // for some reason the old name needs to be sanitized (starts with `/`).
        // the new one doesn't
        let old_name = event.actor.attributes.get("oldName").map(|name| {
            use std::fmt::Write as _;

            let mut replaced = RE_VALIDNAME.replace_all(name, "").into_owned();
            write!(&mut replaced, ".{}", self.domain).expect("Writing to string never fails");

            replaced
        });

        let new_name = event.actor.attributes.get("name").map(|name| {
            use std::fmt::Write as _;

            let mut name = name.to_string();
            write!(&mut name, ".{}", self.domain).expect("Writing to string never fails");

            name
        });

        match (old_name, new_name) {
            (None, None) => event!(Level::WARN, "Rename event without oldName & without name"),
            (None, Some(n)) => event!(Level::WARN, "Rename event without oldName (? -> {})", n),
            (Some(o), None) => event!(Level::WARN, "Rename event without name ({} -> ?)", o),
            (Some(o), Some(n)) => {
                if let Err(error) = self.authority_wrapper.rename(&o, &n).await {
                    event!(
                        Level::WARN,
                        ?error,
                        ?event,
                        container_id = %event.actor.id,
                        old_name = %o,
                        new_name = %n,
                        "Failure to rename container",
                    );
                } else {
                    let new_names = to_full_names(get_all_names_from_event(&event), &self.domain);
                    if let Some(state) = self.containers.lock().await.get_mut(&*event.actor.id) {
                        state.names = new_names;
                    }
                }
            },
        }
    }

    async fn handle_container_start(&self, event: Event) {
        match self.docker.inspect_container(&event.actor.id).await {
            Ok(container) => {
                let full_names =
                    to_full_names(get_all_names_from_inspect(&container), &self.domain);

                self.register_container_networks(
                    &event.actor.id,
                    full_names,
                    container.network_settings,
                )
                .await;
            },
            Err(error) => {
                event!(
                    Level::WARN,
                    ?error,
                    container_id = %event.actor.id,
                    "container:start: failed to inspect container",
                );
            },
        }
    }

    async fn handle_container_die(&self, event: Event) {
        let Some(state) = self.containers.lock().await.remove(&*event.actor.id) else {
            return;
        };

        for (_, network_ips) in state.networks {
            for ip in network_ips.ips() {
                for name in &*state.names {
                    self.authority_wrapper.remove_address(name, ip).await;
                }
            }
        }
    }

    async fn handle_network_connect(&self, event: Event) {
        let Some(container_id) = event.actor.attributes.get("container") else {
            event!(
                Level::WARN,
                ?event,
                "Got network connect event, but event did not contain container id"
            );
            return;
        };

        let Some(network_name) = event.actor.attributes.get("name") else {
            event!(
                Level::WARN,
                ?event,
                "Got network connect event, but event did not contain network name"
            );
            return;
        };

        match self.docker.inspect_container(container_id).await {
            Ok(container) => {
                let Some(network) = container.network_settings.networks.get(&**network_name) else {
                    event!(
                        Level::WARN,
                        %container_id,
                        %network_name,
                        "Got network connect event, but network not found in container inspect",
                    );
                    return;
                };

                let Some(network_ips) = NetworkIps::from_network(network) else {
                    event!(
                        Level::WARN,
                        %container_id,
                        %network_name,
                        "Network connect event: network has no IP addresses",
                    );
                    return;
                };

                let mut containers = self.containers.lock().await;

                let state =
                    containers
                        .entry_ref(&**container_id)
                        .or_insert_with(|| ContainerState {
                            names: to_full_names(
                                get_all_names_from_inspect(&container),
                                &self.domain,
                            ),
                            networks: HashMap::new(),
                        });

                // If the same IPs were previously registered for this network (e.g. startup race between start()'s container list and this event), skip.
                // If different IPs were registered, remove the stale DNS records first.
                match state.networks.insert(network_name.clone(), network_ips) {
                    Some(old_ips) if old_ips == network_ips => return,
                    Some(old_ips) => {
                        for ip in old_ips.ips() {
                            for name in &*state.names {
                                self.authority_wrapper.remove_address(name, ip).await;
                            }
                        }
                    },
                    None => {},
                }

                for ip in network_ips.ips() {
                    for name in &*state.names {
                        self.authority_wrapper.add(name, ip).await;
                    }
                }
            },
            Err(error) => {
                event!(
                    Level::WARN,
                    ?error,
                    %container_id,
                    "Got connect event, but could not find container",
                );
            },
        }
    }

    async fn handle_network_disconnect(&self, event: Event) {
        let Some(container_id) = event.actor.attributes.get("container") else {
            event!(
                Level::WARN,
                ?event,
                "Got network disconnect event, but event did not contain container id"
            );
            return;
        };

        let Some(network_name) = event.actor.attributes.get("name") else {
            event!(
                Level::WARN,
                ?event,
                "Got network disconnect event, but event did not contain network name"
            );
            return;
        };

        let mut containers = self.containers.lock().await;

        let Some(state) = containers.get_mut(&**container_id) else {
            event!(
                Level::WARN,
                %container_id,
                %network_name,
                "Got disconnect event but no container cache entry found",
            );
            return;
        };

        let Some(network_ips) = state.networks.remove(&**network_name) else {
            event!(
                Level::WARN,
                %container_id,
                %network_name,
                "Got disconnect event but no network cache entry found",
            );
            return;
        };

        for ip in network_ips.ips() {
            for name in &*state.names {
                self.authority_wrapper.remove_address(name, ip).await;
            }
        }
    }

    pub async fn consume_events(
        &self,
        mut receiver: Receiver<Event>,
        cancellation_token: &CancellationToken,
    ) {
        loop {
            let event = tokio::select! {
                () = cancellation_token.cancelled() => {
                    event!(Level::INFO, "Listener cancelled");
                    break;
                },
                r = receiver.recv() => {
                    let Some(event) = r else {
                        event!(Level::INFO, "Channel closed / dropped");
                        break;
                    };

                    event
                }
            };

            match event.r#type {
                EventType::Container => match &*event.action {
                    "start" => self.handle_container_start(event).await,
                    "rename" => self.handle_container_rename(event).await,
                    "die" => self.handle_container_die(event).await,
                    rest => {
                        event!(Level::TRACE, r#type = ?event.r#type, event = rest, "ignoring event");
                    },
                },
                EventType::Network => match &*event.action {
                    "connect" => self.handle_network_connect(event).await,
                    "disconnect" => self.handle_network_disconnect(event).await,
                    rest => {
                        event!(Level::TRACE, r#type = ?event.r#type, event = rest, "ignoring event");
                    },
                },
                EventType::Builder
                | EventType::Config
                | EventType::Daemon
                | EventType::Image
                | EventType::Node
                | EventType::Plugin
                | EventType::Secret
                | EventType::Service
                | EventType::Volume => {
                    event!(Level::TRACE, ?event, "Ignoring event");
                },
            }
        }
    }

    pub async fn start(&self) -> Result<(), eyre::Report> {
        for container in self.docker.get_containers().await? {
            if &*container.state != "running" {
                continue;
            }

            let full_names = to_full_names(Vec::from(container.names), &self.domain);

            self.register_container_networks(&container.id, full_names, container.network_settings)
                .await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use hickory_server::proto::rr::Name;
    use ipnet::IpNet;
    use pretty_assertions::assert_eq;

    use crate::docker::monitor::parse_subnet;

    fn subnet(s: &str) -> IpNet {
        s.parse().unwrap()
    }

    #[test]
    fn parse_subnet_ipv4() {
        let ranges = parse_subnet(subnet("172.16.66.123/18")).collect::<Vec<_>>();

        let expected = (64..=127)
            .map(|octet| {
                (
                    IpNet::from_str(&format!("172.16.{}.0/24", octet)).unwrap(),
                    Name::from_str(&format!("{}.16.172.in-addr.arpa.", octet)).unwrap(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(ranges, expected);
    }

    #[test]
    fn parse_subnet_ipv6_aligned() {
        let input = subnet("fd00::/64");
        let ranges = parse_subnet(input).collect::<Vec<_>>();

        let expected = vec![(
            IpNet::from_str("fd00::/64").unwrap(),
            Name::from_str("0.0.0.0.0.0.0.0.0.0.0.0.0.0.d.f.ip6.arpa.").unwrap(),
        )];

        assert_eq!(ranges, expected);
    }

    #[test]
    fn parse_subnet_ipv6_non_aligned() {
        let input = subnet("fd00::/62");
        let ranges = parse_subnet(input).collect::<Vec<_>>();

        let expected = (0_u16..4)
            .map(|i| {
                (
                    IpNet::from_str(&format!("fd00:0:0:{i}::/64")).unwrap(),
                    Name::from_str(&format!("{i}.0.0.0.0.0.0.0.0.0.0.0.0.0.d.f.ip6.arpa."))
                        .unwrap(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(ranges, expected);
    }

    #[test]
    fn parse_subnet_ipv6_odd_nibble_count() {
        // /52 = 13 nibbles — odd count, so the last nibble lands on the high nibble of a byte
        let input = subnet("fd00::/52");
        let ranges = parse_subnet(input).collect::<Vec<_>>();

        let expected = vec![(
            IpNet::from_str("fd00::/52").unwrap(),
            Name::from_str("0.0.0.0.0.0.0.0.0.0.0.d.f.ip6.arpa.").unwrap(),
        )];

        assert_eq!(ranges, expected);
    }

    #[test]
    fn parse_subnet_ipv6_doc_prefix() {
        let input = subnet("2001:db8::/48");
        let ranges = parse_subnet(input).collect::<Vec<_>>();

        let expected = vec![(
            IpNet::from_str("2001:db8::/48").unwrap(),
            Name::from_str("0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa.").unwrap(),
        )];

        assert_eq!(ranges, expected);
    }
}
