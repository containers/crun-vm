#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

if (( $# == 0 )); then
    >&2 echo -n "\
Usage: $0 <engine...>

Examples:
   $ $0 podman         # rootless Podman
   $ sudo $0 podman    # rootful Podman
   $ $0 docker         # Docker
   $ $0 podman docker  # rootless Podman and Docker
"
    exit 2
fi

function __log_and_run() {
    printf '\033[0;33m%s\033[0m\n' "$*"
    "$@"
}

export CARGO_TERM_COLOR=always

script_dir="$( dirname "$0" )"
if [[ "${script_dir}" != . ]]; then
    __log_and_run cd "${script_dir}"
fi

images=(
    quay.io/containerdisks/fedora:39
    quay.io/crun-vm/example-fedora-coreos:39
)

# ensure that tests don't timeout because they're pulling images
for engine in "$@"; do
    for image in "${images[@]}"; do
        __log_and_run "${engine}" pull "${image}"
    done
done

nextest_run=(
    nextest run \
        --all-targets --all-features \
        -- "${@/#/test_run::engine_}"
    )

if command -v cargo-nextest &> /dev/null; then
    __log_and_run cargo "${nextest_run[@]}"
else
    __log_and_run "${nextest_run[@]}"
fi
