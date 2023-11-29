use std::time::Duration;

use http::Uri;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Method, Response};
use hyper_tls::HttpsConnector;
use hyper_unix_socket::UnixSocketConnector;
use tokio::time::timeout;
use tokio_util::bytes::Buf;
use tokio_util::sync::CancellationToken;
use tracing::{event, Level};

use crate::docker::config::{Config, Endpoint};
use crate::docker::Event;
use crate::http_client::{build_request, execute_request};
use crate::models::container_inspect::ContainerInspect;

pub struct Daemon {
    config: Config,
}

impl Daemon {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    async fn send_request(
        &self,
        path_and_query: &str,
        method: Method,
    ) -> Result<Response<Incoming>, color_eyre::Report> {
        match &self.config.endpoint {
            Endpoint::Direct {
                url,
                timeout_milliseconds,
            } => {
                let connector = HttpsConnector::new();
                let request = build_request(url.clone(), path_and_query, method)?;

                let response = execute_request(connector, request);

                match timeout(Duration::from_millis(*timeout_milliseconds), response).await {
                    Ok(Ok(o)) => Ok(o),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(e.into()),
                }
            },
            Endpoint::Socket(socket) => {
                let connector = UnixSocketConnector::new(socket.clone());

                let request =
                    build_request(Uri::from_static("http://localhost"), path_and_query, method)?;

                execute_request(connector, request)
                    .await
                    .map_err(Into::into)
            },
        }
    }

    // pub async fn get_containers(&self) -> Result<Vec<Container>, color_eyre::Report> {
    //     let path_and_query = format!(
    //         "/containers/json?filters={}",
    //         "" /* self.encoded_filters */
    //     );

    //     let response = self.send_request(&path_and_query, Method::GET).await?;

    //     let reader = response.collect().await?.aggregate().reader();

    //     let result = serde_json::from_reader::<_, Vec<Container>>(reader)?;

    //     Ok(result)
    // }

    pub async fn inspect_container(
        &self,
        id: &str,
    ) -> Result<ContainerInspect, color_eyre::Report> {
        let path_and_query = format!("/containers/{id}/json");

        let response = self.send_request(&path_and_query, Method::GET).await?;

        let reader = response.collect().await?.aggregate().reader();
        let result = serde_json::from_reader::<_, ContainerInspect>(reader)?;

        Ok(result)
    }

    pub async fn produce_events(
        &self,
        sender: tokio::sync::mpsc::Sender<Event>,
        token: &CancellationToken,
    ) -> Result<(), color_eyre::Report> {
        let path_and_query = format!("/events{}", "");

        let mut response = self.send_request(&path_and_query, Method::GET).await?;

        let mut buffer = Vec::new();

        loop {
            let frame = tokio::select! {
                maybe_frame = response.frame() => {
                    if let Some(frame) = maybe_frame {
                        frame
                    } else {
                        return Err(color_eyre::Report::msg("No more next frame, other side gone"));
                    }
                },
                () = token.cancelled() => {
                    return Err(color_eyre::Report::msg("Got cancellation event, stopping"));

                },
            };

            let frame = match frame {
                Ok(o) => o,
                Err(e) => {
                    event!(Level::ERROR, message = "Failed to read frame", ?e);
                    continue;
                },
            };

            let Ok(data) = frame.into_data() else {
                // frame is trailers, ignored
                continue;
            };

            // TODO: https://github.com/EmbarkStudios/wasmtime/blob/056ccdec94f89d00325970d1239429a1b39ec729/crates/wasi-http/src/http_impl.rs#L246-L268
            buffer.extend_from_slice(&data);

            // sometimes we get multiple frames per event (?)

            let decoded = match serde_json::from_slice(&data) {
                Ok(o) => o,
                Err(e) => {
                    let f = String::from_utf8_lossy(&data);
                    event!(
                        Level::ERROR,
                        message = "Failed to parse json to struct",
                        ?e,
                        ?f
                    );
                    continue;
                },
            };

            match sender.send(decoded).await {
                Ok(()) => {
                    event!(Level::TRACE, message = "Sent Docker Event to Channel!");
                },
                Err(_) => {
                    return Err::<(), _>(color_eyre::Report::msg("Channel closed"));
                },
            }
        }
    }
}
