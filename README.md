# docker-dns-rs

Rewrite of https://github.com/phensley/docker-dns in Rust.

Code uses [hickory-dns](https://github.com/hickory-dns/hickory-dns) as the DNS library, [hyper-unix-socket](https://github.com/kristof-mattei/hyper-unix-socket) to talk to Docker over a Unix socket and Tokio to be the Socket glue.
