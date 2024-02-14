#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -e

__ssh() {
    ssh \
        -o LogLevel=ERROR \
        -o StrictHostKeyChecking=no \
        -l "$1" \
        localhost \
        "${@:2}"
}

if [[ ! -e /crun-vm/ssh-successful ]]; then

    # retry ssh for some time, ignoring some common errors

    for (( i = 0; i < 60; ++i )); do

        set +e
        output=$( __ssh "$1" </dev/null 2>&1 )
        exit_code=$?
        set -e

        sleep 1

        if (( exit_code != 255 )) ||
            ! grep -iqE "Connection refused|Connection reset by peer|Connection closed by remote host" <<< "$output"; then
            break
        fi

    done

    if (( exit_code != 0 )); then
        >&2 printf '%s\n' "$output"
        exit "$exit_code"
    fi

    # avoid these steps next time

    touch /crun-vm/ssh-successful

fi

__ssh "$1" -- "${@:2}"
