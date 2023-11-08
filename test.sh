#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-only

set -o errexit -o pipefail -o nounset

if (( $# > 1 )); then
    >&2 echo "Usage: $0 [<toolchain>]"
    exit 2
elif (( $# == 1 )); then
    rustup=( rustup run -- "$1" )
else
    rustup=()
fi

function __log_and_run() {
    printf '\033[0;33m%s\033[0m\n' "$*"
    "$@"
}

function __cargo() {
    __log_and_run "${rustup[@]}" cargo "$@"
}

export CARGO_TERM_COLOR=always

script_dir="$( dirname "$0" )"
if [[ "${script_dir}" != . ]]; then
    __log_and_run cd "${script_dir}"
fi

__cargo fmt --all -- --check
__cargo clippy --all-targets --all-features -- --deny warnings
__cargo test --all-targets --all-features
