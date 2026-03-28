use std::net::IpAddr;
use std::sync::{Arc, LazyLock};

use color_eyre::eyre;
use hickory_server::proto::rr::Name;
use regex::Regex;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::docker::daemon::Daemon;
use crate::docker::{Event, EventType};
use crate::models::container_inspect::ContainerNetworkSettings;
use crate::table::AuthorityWrapper;

static RE_VALIDNAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w\d.-]").unwrap());

pub struct Monitor {
    authority_wrapper: AuthorityWrapper,
    docker: Arc<Daemon>,
    domain: Name,
}

fn get_all_names(docker_event: &Event) -> Vec<Box<str>> {
    let mut names = vec![];

    if let Some(sanitized_name) = docker_event
        .actor
        .attributes
        .get("name")
        .map(|name| RE_VALIDNAME.replace_all(name.as_str(), "").to_string())
    {
        names.push(sanitized_name.into_boxed_str());
    }

    let instance = docker_event
        .actor
        .attributes
        .get("com.docker.compose.container-number")
        .map_or(1, |n| n.parse::<usize>().unwrap());

    let service = docker_event
        .actor
        .attributes
        .get("com.docker.compose.service");
    let project = docker_event
        .actor
        .attributes
        .get("com.docker.compose.project");

    if let (Some(service), Some(project)) = (service, project) {
        names.push(format!("{}.{}.{}", instance, service, project).into_boxed_str());

        if instance == 1 {
            names.push(format!("{}.{}", service, project).into_boxed_str());
        }
    }

    names
}

impl Monitor {
    pub fn new(docker: Arc<Daemon>, authority_wrapper: AuthorityWrapper, domain: Name) -> Self {
        Self {
            authority_wrapper,
            docker,
            domain,
        }
    }

    async fn handle_rename(&self, event: &Event) {
        // for some reason the old name needs to be sanitized (starts with `/`).
        // the new one doesn't
        let old_name = event
            .actor
            .attributes
            .get("oldName")
            .map(|n| RE_VALIDNAME.replace_all(n, "").to_string())
            .map(|n| format!("{}.{}", n, self.domain));

        let new_name = event
            .actor
            .attributes
            .get("name")
            .map(|n| format!("{}.{}", n, self.domain));

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
                        "Failure to rename container {} ({} -> {})",
                        event.actor.id,
                        o,
                        n
                    );
                }
            },
        }
    }

    async fn handle_die(&self, event: &Event) {
        let all_names = get_all_names(event);

        for name in &all_names {
            if let Err(error) = self
                .authority_wrapper
                .remove(&format!("{}.{}", name, self.domain))
                .await
            {
                event!(
                    Level::ERROR,
                    ?error,
                    "Something went wrong removing {}'s name {}.{}",
                    event.actor.id,
                    name,
                    self.domain,
                );
            }
        }
    }

    async fn handle_start(&self, event: &Event) {
        let all_names = get_all_names(event);

        match self.docker.inspect_container(event.actor.id.as_str()).await {
            Ok(container) if container.state.running => {
                self.add_container_addresses(
                    &container.id,
                    &all_names,
                    &container.network_settings,
                )
                .await;
            },
            Ok(container) => {
                event!(
                    Level::WARN,
                    ?container,
                    "Got start event with container id {} but the container is not running",
                    event.actor.id,
                );
            },
            Err(error) => {
                event!(
                    Level::WARN,
                    ?error,
                    "Got start event with container id {} but couldn't find it",
                    event.actor.id,
                );
            },
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

            if let EventType::Container = event.r#type {
                match event.action.as_str() {
                    "start" => self.handle_start(&event).await,
                    "rename" => self.handle_rename(&event).await,
                    "die" => self.handle_die(&event).await,
                    rest => {
                        event!(Level::TRACE, event = rest, "ignoring event");
                    },
                }
            }
        }
    }

    pub async fn start(&self) -> Result<(), eyre::Report> {
        for container in self.docker.get_containers().await? {
            if &*container.state == "running" {
                self.add_container_addresses(
                    &container.id,
                    &container.names,
                    &container.network_settings,
                )
                .await;
            }
        }

        Ok(())
    }

    async fn add_container_addresses(
        &self,
        container_id: &str,
        all_names: &[Box<str>],
        network_settings: &ContainerNetworkSettings,
    ) {
        for address in network_settings.networks.values().map(|cn| &cn.ip_address) {
            let parsed_address: IpAddr = match address.parse() {
                Ok(ip_addr) => ip_addr,
                Err(error) => {
                    event!(
                        Level::ERROR,
                        address,
                        container_id = container_id,
                        ?error,
                        "Failed to parse address to an IP address for container",
                    );

                    continue;
                },
            };

            for name in all_names {
                let name: Name = name.parse().unwrap();
                let name = name.append_domain(&self.domain).unwrap();

                self.authority_wrapper.add(name, parsed_address).await;
            }
        }
    }
}
