#!/usr/bin/env bash
set -o errexit -o nounset -o pipefail

: "${COMPOSE_PROJECT_NAME:?}"
: "${DOMAIN:?}"

DNS_SERVER=docker-dns-rs
DEFAULT_NETWORK="${COMPOSE_PROJECT_NAME}_default"
EXTRA_NETWORK="${COMPOSE_PROJECT_NAME}_extra"
TIMEOUT_SECONDS=30

# container_name <service>
container_name() {
    echo "${COMPOSE_PROJECT_NAME}-${1}-1"
}

# names <service> <container>: every name docker-dns-rs registers for the container
names() {
    printf '%s\n' \
        "${2}.${DOMAIN}." \
        "${1}.${COMPOSE_PROJECT_NAME}.${DOMAIN}." \
        "1.${1}.${COMPOSE_PROJECT_NAME}.${DOMAIN}."
}

# ipv4 <container> <network>
ipv4() {
    docker inspect --format "{{ (index .NetworkSettings.Networks \"${2}\").IPAddress }}" "${1}"
}

# ipv6 <container> <network>
ipv6() {
    docker inspect --format "{{ (index .NetworkSettings.Networks \"${2}\").GlobalIPv6Address }}" "${1}"
}

# sorted <values...>
sorted() {
    local -a values
    mapfile -t values < <(printf '%s\n' "$@" | sort)

    echo "${values[*]}"
}

# lookup <dig query...>: the response code followed by the sorted answer rdata, e.g. `NOERROR 172.18.0.2 172.19.0.2`
lookup() {
    local output
    output=$(dig "@${DNS_SERVER}" +noall +comments +answer +tries=1 +time=2 "$@" || true)

    if ! [[ ${output} =~ status:\ ([A-Z]+), ]]; then
        return 0
    fi

    local -a parts=("${BASH_REMATCH[1]}")
    mapfile -t -O 1 parts < <(awk '!/^;/ && NF { print $5 }' <<< "${output}" | sort)

    echo "${parts[*]}"
}

# expect <lookup output> <dig query...>: polls until the lookup matches
expect() {
    local expected=$1
    shift

    local deadline=$((SECONDS + TIMEOUT_SECONDS))
    local actual

    while true; do
        actual=$(lookup "$@")

        if [[ ${actual} == "${expected}" ]]; then
            echo "OK: $* -> ${actual}"
            return 0
        fi

        if ((SECONDS >= deadline)); then
            echo "ERR: $*: expected \`${expected}\`, got \`${actual}\`" >&2
            return 1
        fi

        sleep 1
    done
}

# expect_each <type> <lookup output> <names...>
expect_each() {
    local type=$1 expected=$2
    shift 2

    local name
    for name in "$@"; do
        expect "${expected}" -t "${type}" -q "${name}"
    done
}

main() {
    local running_at_startup started_later stopped_later renamed_later connected_later
    running_at_startup=$(container_name running-at-startup)
    started_later=$(container_name started-later)
    stopped_later=$(container_name stopped-later)
    renamed_later=$(container_name renamed-later)
    connected_later=$(container_name connected-later)

    local -a names
    local ip default_ip extra_ip extra_ip6

    echo "=== server answers over UDP and TCP ==="
    # the first SOA rdata field is the primary name server, docker-dns-rs sets it to the zone
    expect "NOERROR ${DOMAIN}." -t SOA -q "${DOMAIN}"
    expect "NOERROR ${DOMAIN}." +tcp -t SOA -q "${DOMAIN}"

    echo "=== initial scan ==="
    default_ip=$(ipv4 "${running_at_startup}" "${DEFAULT_NETWORK}")
    extra_ip=$(ipv4 "${running_at_startup}" "${EXTRA_NETWORK}")
    extra_ip6=$(ipv6 "${running_at_startup}" "${EXTRA_NETWORK}")
    mapfile -t names < <(names running-at-startup "${running_at_startup}")

    expect_each A "NOERROR $(sorted "${default_ip}" "${extra_ip}")" "${names[@]}"
    expect_each AAAA "NOERROR ${extra_ip6}" "${names[@]}"
    expect "NOERROR $(sorted "${names[@]}")" -x "${default_ip}"
    expect "NOERROR $(sorted "${names[@]}")" -x "${extra_ip}"
    expect "NOERROR $(sorted "${names[@]}")" -x "${extra_ip6}"

    echo "=== static records ==="
    expect "NOERROR 10.1.2.3" -t A -q static-v4.example
    expect "NOERROR fd00::1" -t AAAA -q static-v6.example
    expect "NOERROR static-v4.example." -x 10.1.2.3
    expect "NOERROR static-v6.example." -x fd00::1

    echo "=== start ==="
    docker start "${started_later}"
    ip=$(ipv4 "${started_later}" "${DEFAULT_NETWORK}")
    mapfile -t names < <(names started-later "${started_later}")

    expect_each A "NOERROR ${ip}" "${names[@]}"
    expect "NOERROR $(sorted "${names[@]}")" -x "${ip}"

    echo "=== die ==="
    ip=$(ipv4 "${stopped_later}" "${DEFAULT_NETWORK}")
    mapfile -t names < <(names stopped-later "${stopped_later}")
    expect_each A "NOERROR ${ip}" "${names[@]}"

    docker stop --timeout 1 "${stopped_later}"

    expect_each A "NXDOMAIN" "${names[@]}"
    expect "NXDOMAIN" -x "${ip}"

    echo "=== rename ==="
    ip=$(ipv4 "${renamed_later}" "${DEFAULT_NETWORK}")
    local renamed="${renamed_later}-renamed"

    docker rename "${renamed_later}" "${renamed}"
    mapfile -t names < <(names renamed-later "${renamed}")

    expect_each A "NOERROR ${ip}" "${names[@]}"
    expect "NXDOMAIN" -t A -q "${renamed_later}.${DOMAIN}"
    expect "NOERROR $(sorted "${names[@]}")" -x "${ip}"

    echo "=== network connect ==="
    default_ip=$(ipv4 "${connected_later}" "${DEFAULT_NETWORK}")
    mapfile -t names < <(names connected-later "${connected_later}")

    docker network connect "${EXTRA_NETWORK}" "${connected_later}"
    extra_ip=$(ipv4 "${connected_later}" "${EXTRA_NETWORK}")
    extra_ip6=$(ipv6 "${connected_later}" "${EXTRA_NETWORK}")

    expect_each A "NOERROR $(sorted "${default_ip}" "${extra_ip}")" "${names[@]}"
    expect_each AAAA "NOERROR ${extra_ip6}" "${names[@]}"
    expect "NOERROR $(sorted "${names[@]}")" -x "${extra_ip}"
    expect "NOERROR $(sorted "${names[@]}")" -x "${extra_ip6}"

    echo "=== network disconnect ==="
    docker network disconnect "${EXTRA_NETWORK}" "${connected_later}"

    expect_each A "NOERROR ${default_ip}" "${names[@]}"
    # the names still have A records, so AAAA answers NOERROR without rdata instead of NXDOMAIN
    expect_each AAAA "NOERROR" "${names[@]}"
    expect "NXDOMAIN" -x "${extra_ip}"
    expect "NXDOMAIN" -x "${extra_ip6}"

    echo "OK: all checks passed"
}

main
