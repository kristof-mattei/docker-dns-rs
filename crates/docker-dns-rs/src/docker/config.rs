#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
use std::time::Duration;

use crate::args::RawEndpoint;

pub struct Config {
    pub endpoint: Endpoint,
}

pub enum Endpoint {
    Direct {
        url: http::Uri,
        timeout: Duration,
    },
    #[cfg(not(target_os = "windows"))]
    Socket(PathBuf),
}

impl Config {
    pub fn build(raw_endpoint: RawEndpoint, timeout: Duration) -> Config {
        let endpoint = match raw_endpoint {
            RawEndpoint::Direct(uri) => Endpoint::Direct { url: uri, timeout },
            #[cfg(not(target_os = "windows"))]
            RawEndpoint::Socket(path_buf) => Endpoint::Socket(path_buf),
        };

        Config { endpoint }
    }
}
