use std::time::Duration;

use color_eyre::eyre;
use tokio::time::timeout;
use twistlock::client::{BuildError, Client, EventSubscription};
use twistlock::config::Endpoint;
use twistlock::endpoint::ApiEndpointCallError;
use twistlock::filters::Filters;
use twistlock::models::container::ContainerSummary;
use twistlock::models::container_inspect::ContainerInspect;
use twistlock::models::id::{ContainerRef, NetworkRef};
use twistlock::models::network::{NetworkInspect, NetworkSummary};

async fn with_timeout<T, F>(limit: Duration, call: F) -> Result<T, eyre::Report>
where
    F: Future<Output = Result<T, ApiEndpointCallError>>,
{
    match timeout(limit, call).await {
        Ok(result) => Ok(result?),
        Err(elapsed) => Err(eyre::Report::new(elapsed).wrap_err(format!(
            "The Docker daemon did not answer within {:?}",
            limit
        ))),
    }
}

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
    ) -> Result<Vec<ContainerSummary>, eyre::Report> {
        with_timeout(self.timeout, self.client.list_containers(filters)).await
    }

    pub(crate) async fn inspect_container<'r, C: Into<ContainerRef<'r>>>(
        &self,
        container: C,
    ) -> Result<ContainerInspect, eyre::Report> {
        with_timeout(self.timeout, self.client.inspect_container(container)).await
    }

    pub(crate) async fn list_networks(&self) -> Result<Vec<NetworkSummary>, eyre::Report> {
        with_timeout(self.timeout, self.client.list_networks()).await
    }

    pub(crate) async fn inspect_network<'r, N: Into<NetworkRef<'r>>>(
        &self,
        network: N,
    ) -> Result<NetworkInspect, eyre::Report> {
        with_timeout(self.timeout, self.client.inspect_network(network)).await
    }

    /// Bounds the response head only, the stream itself has no deadline.
    pub(crate) async fn subscribe_events(&self) -> Result<EventSubscription, eyre::Report> {
        with_timeout(self.timeout, self.client.subscribe_events()).await
    }
}
