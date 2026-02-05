use std::path::PathBuf;

use clap::Parser;
use hickory_server::proto::rr::Name;

const DOCKER_SOCK: &str = "/var/run/docker.sock";

#[derive(Parser, Debug)]

pub struct Args {
    #[arg(default_value = DOCKER_SOCK, help = "Path to docker TCP/UNIX socket", long="docker")]
    pub docker: PathBuf,

    #[arg(
        default_value = "docker",
        help = "Base domain name for registered services",
        long = "domain",
        value_parser = parse_domain
    )]
    pub domain: Name,
}

fn parse_domain(raw_domain: &str) -> Result<Name, String> {
    let mut domain: Name = raw_domain.parse()?;
    domain.set_fqdn(true);

    Ok(domain)
}

// docker_url = os.environ.get("DOCKER_HOST")
// if not docker_url:
//     docker_url = DOCKER_SOCK
// parser = argparse.ArgumentParser(
//     PROCESS, epilog=EPILOG, formatter_class=argparse.ArgumentDefaultsHelpFormatter
// )
// parser.add_argument(
//     "--dns-bind", default=DNS_BINDADDR, help="Bind address for DNS server"
// )
// parser.add_argument(
//     "-q", "--quiet", action="store_const", const=1, help="Quiet mode"
// )
