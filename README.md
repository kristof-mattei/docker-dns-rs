# docker-dns-rs

Rewrite of https://github.com/phensley/docker-dns in Rust.

Code uses [hickory-dns](https://github.com/hickory-dns/hickory-dns) as the DNS library, [hyper-unix-socket](https://github.com/kristof-mattei/hyper-unix-socket) to talk to Docker over a Unix socket and Tokio to be the Socket glue.

## Running

```shell
docker run \
    --detach \
    --name docker-dns-rs \
    --restart unless-stopped \
    --publish 53:53/udp \
    --publish 53:53/tcp \
    --volume /var/run/docker.sock:/var/run/docker.sock \
    ghcr.io/kristof-mattei/docker-dns-rs:latest
```

The same as a compose service:

```yaml
services:
  docker-dns-rs:
    image: ghcr.io/kristof-mattei/docker-dns-rs:latest
    restart: unless-stopped
    ports:
      - "53:53/udp"
      - "53:53/tcp"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
```

## Configuration

Every option is a command line flag or the environment variable next to it.

| Flag                            | Variable                    | Default                | Meaning                                                                    |
| ------------------------------- | --------------------------- | ---------------------- | -------------------------------------------------------------------------- |
| `--docker`                      | `DOCKER_HOST`               | `/var/run/docker.sock` | Path to the Docker socket, or a `tcp://` address                           |
| `--domain`                      | `DOMAIN`                    | `docker`               | Base domain the container names are registered under                       |
| `--record`                      | `RECORDS`                   |                        | A static record as `name:ip` or `name:[ipv6]`, repeated or comma separated |
| `--dns-bind`                    | `DNS_BIND`                  | `0.0.0.0:53`           | Address the DNS server binds                                               |
| `--timeout`                     | `timeout`                   | `30`                   | Docker request timeout in seconds, over `tcp://` only                      |
| `--cacert`                      | `CA`                        |                        | CA certificate that verifies the Docker daemon                             |
| `--client-key`, `--client-cert` | `CLIENT_KEY`, `CLIENT_CERT` |                        | Client credentials for mutual TLS, both or neither                         |

## License

MIT, see [LICENSE](./LICENSE)

`SPDX-License-Identifier: MIT`
