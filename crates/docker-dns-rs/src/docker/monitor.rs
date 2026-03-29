use std::net::IpAddr;
use std::sync::{Arc, LazyLock};

use color_eyre::eyre;
use hashbrown::HashMap;
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

struct ContainerState {
    names: Arc<[Name]>,
    /// `network_name` to `ip`.
    networks: HashMap<Box<str>, IpAddr>,
}

pub struct Monitor {
    authority_wrapper: AuthorityWrapper,
    docker: Arc<Daemon>,
    domain: Name,
    /// `container_id` to `ContainerState`.
    /// Invariant: names and network entries are always co-located, you cannot have
    /// a network entry without its accompanying names.
    containers: Mutex<HashMap<Arc<str>, ContainerState>>,
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
            containers: Mutex::new(HashMap::new()),
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
        let full_names = to_full_names(get_all_names_from_event(&event), &self.domain);

        // Only insert if no entry yet, handle_connect may have already populated names
        // from inspect (Docker fires network:connect before container:start).
        self.containers
            .lock()
            .await
            .entry(Arc::from(&*event.actor.id))
            .or_insert_with(|| ContainerState {
                names: full_names,
                networks: HashMap::new(),
            });
    }

    async fn handle_container_die(&self, event: Event) {
        let Some(state) = self.containers.lock().await.remove(&*event.actor.id) else {
            return;
        };

        for (_, ip) in state.networks {
            for name in &*state.names {
                self.authority_wrapper.remove_address(name, ip).await;
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

        // read cached names before inspecting, release the lock before the async call.
        let cached_names = self
            .containers
            .lock()
            .await
            .get(&**container_id)
            .map(|s| Arc::clone(&s.names));

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

                // Use cached names if available, fall back to inspect.
                // Docker fires network:connect before container:start, so the cache is
                // typically empty when this runs for a new container.
                let full_names = cached_names.unwrap_or_else(|| {
                    to_full_names(get_all_names_from_inspect(&container), &self.domain)
                });

                // Record the network entry. Returns the previously registered IP, if any.
                // Also ensures names are present whenever network entries exist.
                let replaced_ip = {
                    let mut containers = self.containers.lock().await;
                    let state = containers
                        .entry(Arc::from(&**container_id))
                        .or_insert_with(|| ContainerState {
                            names: Arc::clone(&full_names),
                            networks: HashMap::new(),
                        });

                    if state.networks.get(&**network_name).copied() == Some(ip) {
                        // startup race: start() already registered this exact IP
                        return;
                    }

                    state.networks.insert(network_name.clone(), ip)
                };

                // If a different IP was previously registered for this network (e.g. Docker
                // reassigned the IP between start()'s container list and this event),
                // remove the stale DNS records before adding the new ones.
                if let Some(old_ip) = replaced_ip {
                    for name in &*full_names {
                        self.authority_wrapper.remove_address(name, old_ip).await;
                    }
                }

                for name in &*full_names {
                    self.authority_wrapper.add(name, ip).await;
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

        let (ip, names) = {
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

            let Some(ip) = state.networks.remove(&**network_name) else {
                event!(
                    Level::WARN,
                    %container_id,
                    %network_name,
                    "Got disconnect event but no network cache entry found",
                );
                return;
            };

            (ip, Arc::clone(&state.names))
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
                    "start" => self.handle_container_start(event).await,
                    "rename" => self.handle_container_rename(event).await,
                    "die" => self.handle_container_die(event).await,
                    rest => {
                        event!(Level::TRACE, r#type = %event.r#type, event = rest, "ignoring event");
                    },
                },
                EventType::Network => match &*event.action {
                    "connect" => self.handle_network_connect(event).await,
                    "disconnect" => self.handle_network_disconnect(event).await,
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

            // Insert names upfront so any handle_connect events racing with start()
            // can find them in the cache.
            self.containers
                .lock()
                .await
                .entry(Arc::clone(&container_id))
                .or_insert_with(|| ContainerState {
                    names: Arc::clone(&full_names),
                    networks: HashMap::new(),
                });

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

                if let Some(state) = self.containers.lock().await.get_mut(&*container_id) {
                    state.networks.insert(network_name, ip);
                }
            }
        }

        Ok(())
    }
}
