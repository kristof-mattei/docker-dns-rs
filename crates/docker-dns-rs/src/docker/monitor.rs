use std::net::IpAddr;
use std::sync::{Arc, LazyLock};

use color_eyre::eyre;
use hashbrown::{Equivalent, HashMap};
use hickory_server::proto::rr::Name;
use regex::Regex;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::docker::daemon::Daemon;
use crate::docker::{Event, EventType};
use crate::models::container_inspect::ContainerInspect;
use crate::table::AuthorityWrapper;

static RE_VALIDNAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w\d.-]").unwrap());

#[derive(Hash, PartialEq, Eq)]
struct Query<'a>(&'a str, &'a str);

type NetworkKey = (Arc<str>, Box<str>);

impl Equivalent<NetworkKey> for Query<'_> {
    fn equivalent(&self, key: &NetworkKey) -> bool {
        self.0 == &*key.0 && self.1 == &*key.1
    }
}

pub struct Monitor {
    authority_wrapper: AuthorityWrapper,
    docker: Arc<Daemon>,
    domain: Name,
    /// `container_id` to list of names.
    /// Populated on container `start`, removed on container `die`.
    names: Mutex<HashMap<Arc<str>, Arc<[Name]>>>,
    /// (`container_id`, `network_name`) to `ip_address mapping`.
    /// Populated on network `connect`, removed on network `disconnect`.
    networks: Mutex<HashMap<NetworkKey, IpAddr>>,
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
        .filter_map(|n| {
            let parsed = match n.parse::<Name>() {
                Ok(p) => p,
                Err(error) => {
                    event!(Level::WARN, name = %n, ?error, "Failed to parse container name as DNS name, skipping");
                    return None;
                },
            };
            match parsed.append_domain(domain) {
                Ok(full) => Some(full),
                Err(error) => {
                    event!(Level::WARN, name = %n, ?error, "Failed to append domain to container name, skipping");
                    None
                },
            }
        })
        .collect()
}

impl Monitor {
    pub fn new(docker: Arc<Daemon>, authority_wrapper: AuthorityWrapper, domain: Name) -> Self {
        Self {
            authority_wrapper,
            docker,
            domain,
            names: Mutex::new(HashMap::new()),
            networks: Mutex::new(HashMap::new()),
        }
    }

    async fn handle_rename(&self, event: Event) {
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
                    self.names
                        .lock()
                        .await
                        .insert(Arc::from(&*event.actor.id), new_names);
                }
            },
        }
    }

    async fn handle_start(&self, event: Event) {
        let full_names = to_full_names(get_all_names_from_event(&event), &self.domain);

        self.names
            .lock()
            .await
            .insert(Arc::from(&*event.actor.id), full_names);
    }

    async fn handle_die(&self, event: Event) {
        let container_id = &*event.actor.id;

        let names = self.names.lock().await.remove(container_id);

        // drain any network entries not already removed by disconnect events
        let dangling_ips: Vec<_> = self
            .networks
            .lock()
            .await
            .extract_if(|&(ref id, _), _| &**id == container_id)
            .map(|(_, ip)| ip)
            .collect();

        if let Some(names) = names {
            for ip in dangling_ips {
                for name in &*names {
                    self.authority_wrapper.remove_address(name, ip).await;
                }
            }
        }
    }

    async fn handle_connect(&self, event: Event) {
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

        // read cached names before inspecting, don't hold the lock across an await
        let cached_names = self.names.lock().await.get(&**container_id).cloned();

        match self.docker.inspect_container(container_id).await {
            Ok(container) if container.state.running => {
                let Some(network) = container.network_settings.networks.get(&**network_name) else {
                    event!(
                        Level::WARN,
                        %container_id,
                        %network_name,
                        "Got network connect event, but network not found in container inspect",
                    );
                    return;
                };

                let ip: IpAddr = match network.ip_address.parse() {
                    Ok(ip) => ip,
                    Err(error) => {
                        event!(
                            Level::ERROR,
                            ?error,
                            %container_id,
                            address = %network.ip_address,
                            "Failed to parse IP address for container on network",
                        );
                        return;
                    },
                };

                // Use cached names if available. Docker fires network:connect before container:start, so the names cache is typically not populated yet when this runs.
                // Fall back to inspect, which also covers containers that were already running when the monitor started (no start event seen for them).
                let full_names = if let Some(names) = cached_names {
                    names
                } else {
                    let names = to_full_names(get_all_names_from_inspect(&container), &self.domain);
                    self.names
                        .lock()
                        .await
                        .insert(Arc::from(&**container_id), Arc::clone(&names));
                    names
                };

                // startup race: connect event buffered while start() was running
                if self
                    .networks
                    .lock()
                    .await
                    .get(&Query(container_id, network_name))
                    .copied()
                    == Some(ip)
                {
                    return;
                }

                for name in &*full_names {
                    self.authority_wrapper.add(name, ip).await;
                }

                self.networks
                    .lock()
                    .await
                    .insert((Arc::from(&**container_id), network_name.clone()), ip);
            },
            Ok(container) => {
                event!(
                    Level::WARN,
                    ?container,
                    %container_id,
                    "Got connect event, but container was not running",
                );
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

    async fn handle_disconnect(&self, event: Event) {
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

        let ip = self
            .networks
            .lock()
            .await
            .remove(&Query(container_id, network_name));

        let Some(ip) = ip else {
            event!(
                Level::WARN,
                %container_id,
                %network_name,
                "Got disconnect event but no network cache entry found",
            );
            return;
        };

        let names = self.names.lock().await.get(&**container_id).cloned();

        let Some(names) = names else {
            event!(
                Level::WARN,
                %container_id,
                "Got disconnect event but no names cache entry found",
            );
            return;
        };

        for name in &*names {
            self.authority_wrapper.remove_address(name, ip).await;
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
                    "start" => self.handle_start(event).await,
                    "rename" => self.handle_rename(event).await,
                    "die" => self.handle_die(event).await,
                    rest => {
                        event!(Level::TRACE, r#type = %event.r#type, event = rest, "ignoring event");
                    },
                },
                EventType::Network => match &*event.action {
                    "connect" => self.handle_connect(event).await,
                    "disconnect" => self.handle_disconnect(event).await,
                    rest => {
                        event!(Level::TRACE, r#type = %event.r#type, event = rest, "ignoring event");
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

            let container_id = Arc::from(container.id);
            let full_names = to_full_names(Vec::from(container.names), &self.domain);

            self.names
                .lock()
                .await
                .insert(Arc::clone(&container_id), Arc::clone(&full_names));

            for (network_name, network) in container.network_settings.networks {
                let ip: IpAddr = match network.ip_address.parse() {
                    Ok(ip) => ip,
                    Err(error) => {
                        event!(
                            Level::ERROR,
                            ?error,
                            %container_id,
                            %network_name,
                            address = %network.ip_address,
                            "Failed to parse IP address during startup",
                        );
                        continue;
                    },
                };

                for name in &*full_names {
                    self.authority_wrapper.add(name, ip).await;
                }

                self.networks
                    .lock()
                    .await
                    .insert((Arc::clone(&container_id), network_name), ip);
            }
        }

        Ok(())
    }
}
