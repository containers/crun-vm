#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

if (( $# > 0 )); then
    >&2 echo "Usage: $0"
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

__log_and_run cargo fmt --all -- --check
__log_and_run cargo clippy --all-targets --all-features -- --deny warnings
