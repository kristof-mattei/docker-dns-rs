use std::time::Duration;

use color_eyre::Section as _;
use color_eyre::eyre::Report;
#[cfg(not(target_os = "windows"))]
use http::Uri;
use http_body_util::BodyExt as _;
use hyper::body::Incoming;
use hyper::{Method, Response};
use hyper_rustls::HttpsConnectorBuilder;
#[cfg(not(target_os = "windows"))]
use hyper_unix_socket::UnixSocketConnector;
use tokio::time::timeout;
use tokio_util::bytes::Buf as _;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::docker::Event;
use crate::docker::config::{Config, Endpoint};
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
        match self.config.endpoint {
            Endpoint::Direct {
                ref url,
                timeout_milliseconds,
            } => {
                let connector = HttpsConnectorBuilder::new()
                    .with_native_roots()?
                    .https_or_http()
                    .enable_http1()
                    .build();

                let request = build_request(url.clone(), path_and_query, method)?;

                let response = execute_request(connector, request);

                match timeout(Duration::from_millis(timeout_milliseconds), response).await {
                    Ok(Ok(o)) => Ok(o),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(e.into()),
                }
            },
            #[cfg(not(target_os = "windows"))]
            Endpoint::Socket(ref socket) => {
                let connector = UnixSocketConnector::new(socket.clone());

                let request =
                    build_request(Uri::from_static("http://localhost"), path_and_query, method)?;

                execute_request(connector, request).await
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

        let mut buffer = Vec::<u8>::new();

        // Inspired by https://github.com/EmbarkStudios/wasmtime/blob/056ccdec94f89d00325970d1239429a1b39ec729/crates/wasi-http/src/http_impl.rs#L246-L268
        loop {
            let frame = tokio::select! {
                frame = response.frame() => frame,
                () = token.cancelled() => {
                    return Err(color_eyre::Report::msg("Got cancellation event, stopping"));
                },
            };

            let frame = match frame {
                Some(Ok(frame)) => frame,
                Some(Err(err)) => {
                    event!(Level::ERROR, ?err, "Failed to read frame");
                    continue;
                },
                None => {
                    return Err(color_eyre::Report::msg(
                        "No more next frame, other side gone",
                    ));
                },
            };

            let Ok(data) = frame.into_data() else {
                // frame is trailers, ignored
                continue;
            };

            buffer.extend_from_slice(&data);

            while let Some(i) = buffer.iter().position(|b| b == &b'\n') {
                Daemon::decode_send(&buffer[0..=i], &sender).await?;

                buffer.drain(0..=i);
            }

            if !buffer.is_empty() {
                // sometimes we get multiple frames per event
                event!(
                    Level::TRACE,
                    leftover = ?String::from_utf8_lossy(&buffer),
                    "Buffer leftover"
                );
            }
        }
    }

    async fn decode_send(
        data: &[u8],
        sender: &tokio::sync::mpsc::Sender<Event>,
    ) -> Result<(), Report> {
        let decoded = match serde_json::from_slice(data) {
            Ok(o) => o,
            Err(e) => {
                let decoded_data = String::from_utf8_lossy(data);
                event!(Level::ERROR, err= ?e, ?decoded_data, "Failed to parse json to struct");

                return Ok(());
            },
        };

        sender
            .send(decoded)
            .await
            .map_err(|err| color_eyre::Report::msg("Channel closed").error(err))
    }
}
