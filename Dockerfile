FROM --platform=$BUILDPLATFORM rust:1.82.0@sha256:7e1bc0eced4786d15c442dc6bbc32522171621bfb417b7f218999a6f101d64f4 AS builder

ARG TARGET=x86_64-unknown-linux-musl
ARG APPLICATION_NAME

RUN rustup target add ${TARGET}

RUN rm -f /etc/apt/apt.conf.d/docker-clean; echo 'Binary::apt::APT::Keep-Downloaded-Packages "true";' > /etc/apt/apt.conf.d/keep-cache

# borrowed (Ba Dum Tss!) from
# https://github.com/pablodeymo/rust-musl-builder/blob/7a7ea3e909b1ef00c177d9eeac32d8c9d7d6a08c/Dockerfile#L48-L49
RUN --mount=type=cache,target=/var/cache/apt --mount=type=cache,target=/var/lib/apt \
    dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get --no-install-recommends install -y \
    build-essential \
    musl-dev \
    musl-tools \
    libc6-dev-arm64-cross \
    gcc-aarch64-linux-gnu

# The following block
# creates an empty app, and we copy in Cargo.toml and Cargo.lock as they represent our dependencies
# This allows us to copy in the source in a different layer which in turn allows us to leverage Docker's layer caching
# That means that if our dependencies don't change rebuilding is much faster
WORKDIR /build
RUN cargo new ${APPLICATION_NAME}
WORKDIR /build/${APPLICATION_NAME}
COPY .cargo ./.cargo
COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,id=cargo-dependencies,target=/build/${APPLICATION_NAME}/target \
    cargo build --release --target ${TARGET}

# now we copy in the source which is more prone to changes and build it
COPY src ./src

# --release not needed, it is implied with install
RUN --mount=type=cache,id=full-build,target=/build/${APPLICATION_NAME}/target \
    cargo install --path . --target ${TARGET} --root /output

# Create a separate file because we don't want to copy over the
# whole one to our scratch container
RUN echo "root:x:0:0:root:/dev/null:/sbin/nologin" > /tmp/passwd

FROM scratch

ARG APPLICATION_NAME

# We're explicitely wanting to be root, because most consumers will just
# run the container expecting it to work. Since Docker runs as root, we match
# We fetch the passwrd file from the builder as we don't have sh here to create the file
COPY --from=builder /tmp/passwd /etc/passwd
USER root

WORKDIR /app
COPY --from=builder /output/bin/${APPLICATION_NAME} /app/entrypoint

ENV RUST_BACKTRACE=full
ENTRYPOINT ["/app/entrypoint"]
