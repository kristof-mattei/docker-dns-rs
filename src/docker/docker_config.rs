use std::ffi::OsString;

use crate::env::try_parse_env_variable_with_default;

pub struct DockerConfig {
    pub endpoint: Endpoint,
    pub options: Vec<String>,
}

pub enum Endpoint {
    Direct {
        url: http::Uri,
        timeout_milliseconds: u64,
    },
    Socket(String),
}

impl DockerConfig {
    pub fn build() -> Result<DockerConfig, color_eyre::Report> {
        const TCP_START: &str = "tcp://";
        let mut docker_socket_or_uri = std::env::var_os("DOCKER_SOCK")
            .map_or_else(
                || Ok(String::from("/var/run/docker.sock")),
                OsString::into_string,
            )
            .map_err(|err| {
                color_eyre::Report::msg(format!("Could not convert {:?} to String", err))
            })?;

        let timeout_milliseconds = try_parse_env_variable_with_default("CURL_TIMEOUT", 30)?;

        let endpoint = if docker_socket_or_uri.starts_with(TCP_START) {
            docker_socket_or_uri.replace_range(..TCP_START.len(), "https://");

            Endpoint::Direct {
                url: docker_socket_or_uri.parse().unwrap(),
                timeout_milliseconds,
            }
        } else {
            // we're connecting over a socket, so the uri is localhost
            Endpoint::Socket(docker_socket_or_uri)
        };

        // TODO check if docker socket exists

        Ok(DockerConfig {
            endpoint,
            options: vec![],
        })
    }

    // fn curl_options(&self) -> String {
    //     match self {
    //         ApiConfig::Tcp(_) => String::from(
    //             "--cacert /certs/ca.pem --key /certs/client-key.pem --cert /certs/client-cert.pem",
    //         ),
    //         ApiConfig::Socket(s) => format!("--unix-socket {}", s),
    //     }
    // }
}
