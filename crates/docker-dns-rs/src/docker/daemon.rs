use std::time::Duration;

use twistlock::client::{BuildError, Client, EventSubscription};
use twistlock::config::Endpoint;
use twistlock::endpoint::ApiEndpointCallError;
use twistlock::filters::Filters;
use twistlock::models::container::ContainerSummary;
use twistlock::models::container_inspect::ContainerInspect;
use twistlock::models::id::{ContainerRef, NetworkRef};
use twistlock::models::network::{NetworkInspect, NetworkSummary};

/// A [`Client`] with every call bounded by `timeout`.
pub(crate) struct Daemon {
    client: Client,
    timeout: Duration,
}

impl Daemon {
    pub(crate) fn build(endpoint: Endpoint, timeout: Duration) -> Result<Self, BuildError> {
        Ok(Self {
            client: Client::build(endpoint)?,
            timeout,
        })
    }

    pub(crate) async fn list_containers(
        &self,
        filters: &Filters,
    ) -> Result<Vec<ContainerSummary>, ApiEndpointCallError> {
        self.client
            .list_containers(filters)
            .timeout(self.timeout)
            .send()
            .await
    }

    pub(crate) async fn inspect_container<'r, C: Into<ContainerRef<'r>>>(
        &self,
        container: C,
    ) -> Result<ContainerInspect, ApiEndpointCallError> {
        self.client
            .inspect_container(container)
            .timeout(self.timeout)
            .send()
            .await
    }

    pub(crate) async fn list_networks(&self) -> Result<Vec<NetworkSummary>, ApiEndpointCallError> {
        self.client
            .list_networks()
            .timeout(self.timeout)
            .send()
            .await
    }

    pub(crate) async fn inspect_network<'r, N: Into<NetworkRef<'r>>>(
        &self,
        network: N,
    ) -> Result<NetworkInspect, ApiEndpointCallError> {
        self.client
            .inspect_network(network)
            .timeout(self.timeout)
            .send()
            .await
    }

    /// Bounds the response head only, the stream itself has no deadline.
    pub(crate) async fn subscribe_events(&self) -> Result<EventSubscription, ApiEndpointCallError> {
        self.client
            .subscribe_events()
            .timeout(self.timeout)
            .send()
            .await
    }
}
