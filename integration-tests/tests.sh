#!/usr/bin/env bash
set -o errexit -o nounset -o xtrace -o pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

COMPOSE_PROJECT_NAME=${1:-docker-dns-rs-test}
export COMPOSE_PROJECT_NAME

DOMAIN=docker.test
export DOMAIN

COMPOSE_FILE="docker-compose.yml:docker-compose.watch.yml:"

if [[ -n ${IMAGE_ID+x} ]]; then
    # CI sets IMAGE_ID to the image it built
    COMPOSE_FILE+="docker-compose.image.yml"
else
    # the Dockerfile needs version-bump.patch
    touch ../version-bump.patch

    COMPOSE_FILE+="docker-compose.build.yml"
fi

export COMPOSE_FILE

docker compose config

function cleanup() {
    exit_status=$?
    echo "exit was $exit_status"
    if (( exit_status != 0 )); then
        docker compose logs docker-dns-rs
    fi
    docker compose down --timeout 1 || true
    exit "$exit_status"
}

trap cleanup EXIT
docker compose build
docker compose up --no-start --quiet-pull --force-recreate

# the initial scan of docker-dns-rs has to find these running
docker compose start running-at-startup stopped-later renamed-later connected-later
docker compose start docker-dns-rs

docker compose up --abort-on-container-exit --exit-code-from watch-docker-dns-rs watch-docker-dns-rs
