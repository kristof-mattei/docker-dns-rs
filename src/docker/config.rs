use std::ffi::OsString;

use color_eyre::eyre;

use crate::utils::env::try_parse_env_variable_with_default;

pub struct Config {
    pub endpoint: Endpoint,
}

pub enum Endpoint {
    Direct {
        url: http::Uri,
        timeout_milliseconds: u64,
    },
    #[cfg(not(target_os = "windows"))]
    Socket(String),
}

impl Config {
    pub fn build() -> Result<Config, eyre::Report> {
        const TCP_START: &str = "tcp://";

        let mut docker_socket_or_uri = std::env::var_os("DOCKER_SOCK")
            .map_or_else(
                || Ok(String::from("/var/run/docker.sock")),
                OsString::into_string,
            )
            .map_err(|docker_sock_value| {
                eyre::Report::msg(format!(
                    "Could not convert \"{}\" to String",
                    docker_sock_value.display()
                ))
            })?;

        let timeout_milliseconds = try_parse_env_variable_with_default("CURL_TIMEOUT", 30)?;

        let endpoint = if docker_socket_or_uri.starts_with(TCP_START) {
            docker_socket_or_uri.replace_range(..TCP_START.len(), "http://");

            Endpoint::Direct {
                url: docker_socket_or_uri.parse().unwrap(),
                timeout_milliseconds,
            }
        } else {
            #[cfg(target_os = "windows")]
            panic!("Unix Sockets are not supported in Windows");

            // we're connecting over a socket, so the uri is localhost
            #[cfg(not(target_os = "windows"))]
            Endpoint::Socket(docker_socket_or_uri)
        };

        Ok(Config { endpoint })
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
