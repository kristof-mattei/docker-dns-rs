use std::sync::Arc;

use hickory_server::proto::rr::Name;
use once_cell::sync::Lazy;
use regex::Regex;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{event, Level};

use crate::docker::daemon::Daemon;
use crate::docker::Event;
use crate::table::AuthorityWrapper;

static RE_VALIDNAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^\w\d.-]").unwrap());

pub struct Monitor {
    authority_wrapper: AuthorityWrapper,
    docker: Arc<Daemon>,
    domain: Name,
}

fn get_all_names(docker_event: &Event) -> Vec<String> {
    let mut names = vec![];

    if let Some(sanitized_name) = docker_event
        .actor
        .attributes
        .get("name")
        .map(|name| RE_VALIDNAME.replace_all(name.as_str(), "").to_string())
    {
        names.push(sanitized_name);
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
        names.push(format!("{}.{}.{}", instance, service, project));

        if instance == 1 {
            names.push(format!("{}.{}", service, project));
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
                if let Err(e) = self.authority_wrapper.rename(&o, &n).await {
                    event!(
                        Level::WARN,
                        ?e,
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
            if let Err(e) = self
                .authority_wrapper
                .remove(&format!("{}.{}", name, self.domain))
                .await
            {
                event!(
                    Level::ERROR,
                    ?e,
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
                for address in container
                    .network_settings
                    .networks
                    .values()
                    .map(|cn| &cn.ip_address)
                {
                    let parsed_address = match address.parse() {
                        Ok(o) => o,
                        Err(e) => {
                            event!(Level::ERROR, "Failed to parse address {} to an IP address for container {}: {:?}", address, event.actor.id, e);
                            continue;
                        },
                    };

                    for name in &all_names {
                        if let Err(e) = self
                            .authority_wrapper
                            .add(format!("{}.{}", name, self.domain), parsed_address)
                            .await
                        {
                            event!(
                                Level::ERROR,
                                ?e,
                                "Something went wrong adding {}.{} -> {}",
                                name,
                                self.domain,
                                parsed_address
                            );
                        }
                    }
                }
            },
            Ok(container) => {
                event!(
                    Level::WARN,
                    ?container,
                    "Got start event with container id {} but the container is not running",
                    event.actor.id,
                );
            },
            Err(e) => {
                event!(
                    Level::WARN,
                    ?e,
                    "Got start event with container id {} but couldn't find it",
                    event.actor.id,
                );
            },
        };
    }

    pub async fn consume_events(&self, mut receiver: Receiver<Event>, token: &CancellationToken) {
        loop {
            let event = tokio::select! {
                () = token.cancelled() => {
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

            if let crate::docker::EventType::Container = event.r#type {
                match event.action.as_str() {
                    "start" => self.handle_start(&event).await,
                    "rename" => self.handle_rename(&event).await,
                    "die" => self.handle_die(&event).await,
                    _ => {},
                }
            }
        }
    }
}
