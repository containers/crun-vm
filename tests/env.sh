#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

start_time="$( date +%s%N )"

env_image_base=${CRUN_VM_TEST_ENV_BASE_IMAGE:-"quay.io/containerdisks/fedora:40"}
env_image=quay.io/crun-vm/test-env:latest
container_name=crun-vm-test-env

declare -A TEST_IMAGES
TEST_IMAGES=(
    [fedora]=quay.io/containerdisks/fedora:40               # uses cloud-init
    [coreos]=quay.io/crun-vm/example-fedora-coreos:40       # uses Ignition
    [fedora-bootc]=quay.io/crun-vm/example-fedora-bootc:40  # bootable container
)

declare -A TEST_IMAGES_DEFAULT_USER
TEST_IMAGES_DEFAULT_USER=(
    [fedora]=fedora
    [coreos]=core
    [fedora-bootc]=fedora
)

declare -A TEST_IMAGES_DEFAULT_USER_HOME
TEST_IMAGES_DEFAULT_USER_HOME=(
    [fedora]=/home/fedora
    [coreos]=/var/home/core
    [fedora-bootc]=/var/home/cloud-user
)

__bad_usage() {
    >&2 echo -n "\
Usage: $0 <command> [<args...>]

build
   $0 run <engine> <test_scripts...>
   $0 start
   $0 stop

COMMANDS

   build
      Build the test env VM container image.

   start
      Start the test env VM.

   restart
      Stop the test env VM if running, then start it.

   stop
      Stop the test env VM if running.

   run <engine> <test_script>
   run <engine> all
      Run a test script in the test env VM under the given engine. <engine> must
      be one of 'docker', 'podman', 'rootful-podman', or 'all'.

   ssh
      SSH into the test env VM for debugging.
"
    exit 2
}

# Usage: __elapsed
__elapsed() {
    local delta
    delta=$(( $( date +%s%N ) - start_time ))
    printf '%d.%09d' "$(( delta / 10**9 ))" "$(( delta % 10**9 ))"
}

# Usage: __small_log_without_time <color> <format> <args...>
__small_log_without_time() {
    # shellcheck disable=SC2059
    >&2 printf "\033[%sm--- %s\033[0m\n" \
        "$1" "$( printf "${@:2}" )"
}

# Usage: __log <color> <format> <args...>
__small_log() {
    # shellcheck disable=SC2059
    >&2 printf "\033[%sm--- [%6.1f] %s\033[0m\n" \
        "$1" "$( __elapsed )" "$( printf "${@:2}" )"
}

# Usage: __big_log <color> <format> <args...>
__big_log() {
    local text term_cols sep_len
    text="$( printf "${@:2}" )"
    term_cols="$( tput cols 2> /dev/null )" || term_cols=80
    sep_len="$(( term_cols - ${#text} - 16 ))"
    >&2 printf "\033[%sm--- [%6.1f] %s " "$1" "$( __elapsed )" "${text}"
    >&2 printf '%*s\033[0m\n' "$(( sep_len < 0 ? 0 : sep_len ))" '' | tr ' ' -
}

__log_without_time_and_run() {
    __small_log_without_time 36 '$ %s' "$*"
    "$@"
}

__log_and_run() {
    __small_log 36 '$ %s' "$*"
    "$@"
}

__rel() {
    realpath -s --relative-to=. "$1"
}

__build_runtime() {
    __big_log 33 'Building crun-vm...'
    __log_and_run cargo build
    runtime=$repo_root/target/debug/crun-vm
}

__extra_cleanup() { :; }

repo_root=$( readlink -e "$( dirname "$0" )/.." )
cd "$repo_root"

temp_dir=$( mktemp -d )
trap '__extra_cleanup; rm -fr "$temp_dir"' EXIT

export RUST_BACKTRACE=1 RUST_LIB_BACKTRACE=1

case "${1:-}" in
build)
    if (( $# != 1 )); then
        __bad_usage
    fi

    __build_runtime

    __big_log 33 'Building test env image...'

    # extract base image file

    __log_and_run "$( __rel "$repo_root/util/extract-vm-image.sh" )" \
        "$env_image_base" \
        "$temp_dir/image"

    # expand base image

    __log_and_run qemu-img create -f qcow2 "$temp_dir/image.qcow2" 50G
    __log_and_run virt-resize \
        --quiet \
        --expand /dev/sda4 \
        "$temp_dir/image" \
        "$temp_dir/image.qcow2"

    rm "$temp_dir/image"

    # launch VM from base image file

    __log_and_run podman run \
        --name "$container_name-build" \
        --runtime "$runtime" \
        --memory 8g \
        --rm -dit \
        --rootfs "$temp_dir" \
        --persistent

    # shellcheck disable=SC2317
    __extra_cleanup() {
        __log_and_run podman stop --time 0 "$container_name-build"
    }

    # customize VM

    __exec() {
        __log_and_run podman exec "$container_name-build" --as fedora "$@"
    }

    # generate an ssh keypair for users fedora and root so crun-vm containers
    # get a predictable keypair
    __exec 'ssh-keygen -q -f .ssh/id_rsa -N "" && sudo cp -r .ssh /root/'

    __exec sudo dnf update -y
    __exec sudo dnf install -y \
        bash \
        coreutils \
        crun \
        crun-krun \
        docker \
        genisoimage \
        grep \
        htop \
        libselinux-devel \
        libvirt-client \
        libvirt-daemon-driver-qemu \
        libvirt-daemon-log \
        lsof \
        openssh-clients \
        podman \
        qemu-img \
        qemu-system-x86-core \
        shadow-utils \
        util-linux \
        virtiofsd
    __exec sudo dnf clean all

    daemon_json='{ "runtimes": { "crun-vm": { "path": "/home/fedora/target/debug/crun-vm" } } }'
    __exec sudo mkdir -p /etc/docker
    __exec "echo ${daemon_json@Q} | sudo tee /etc/docker/daemon.json"

    __exec sudo cloud-init clean --logs  # run cloud-init again on next boot

    __exec sudo poweroff || true

    # sparsify image file

    __log_and_run podman wait --ignore "$container_name-build"
    __extra_cleanup() { :; }

    __log_and_run virt-sparsify --quiet --in-place "$temp_dir/image.qcow2"

    # package new image file

    __log_and_run "$( __rel "$repo_root/util/package-vm-image.sh" )" \
        "$temp_dir/image.qcow2" \
        "$env_image"

    __big_log 33 'Done.'
    ;;

start)
    if (( $# != 1 )); then
        __bad_usage
    fi

    if podman container exists "$container_name"; then
        >&2 echo "Already started."
        exit 0
    fi

    __build_runtime

    # launch VM

    __log_and_run podman run \
        --name "$container_name" \
        --pull never \
        --runtime "$runtime" \
        --memory 8g \
        --rm -dit \
        -v "$temp_dir":/home/fedora/images:z \
        -v "$repo_root/target":/home/fedora/target:z \
        "$env_image"

    # shellcheck disable=SC2317
    __extra_cleanup() {
        __log_and_run podman stop --time 0 "$container_name"
    }

    # load test images onto VM

    __exec() {
        __log_and_run podman exec "$container_name" --as fedora "$@"
    }

    chmod a+rx "$temp_dir"  # so user "fedora" in guest can access it

    for image in "${TEST_IMAGES[@]}"; do
        __log_and_run podman pull "$image"
        __log_and_run podman save "$image" -o "$temp_dir/image.tar"

        __exec cp /home/fedora/images/image.tar image.tar
        __exec sudo docker load -i image.tar
        __exec podman      load -i image.tar
        __exec sudo podman load -i image.tar
        __exec rm image.tar

        rm "$temp_dir/image.tar"
    done

    __extra_cleanup() { :; }
    ;;

restart)
    "$0" stop
    "$0" start
    ;;

stop)
    if (( $# != 1 )); then
        __bad_usage
    fi

    __log_and_run podman stop --ignore "$container_name"
    __log_and_run podman wait --ignore "$container_name"
    ;;

run)
    if (( $# < 3 )); then
        __bad_usage
    fi

    case "$2" in
    docker|podman|rootful-podman)
        engines=( "$2" )
        ;;
    all)
        engines=( docker podman rootful-podman )
        ;;
    *)
        __bad_usage
        ;;
    esac

    if (( $# == 3 )) && [[ "$3" == all ]]; then
        mapfile -d '' -t tests < <( find "$repo_root/tests/t" -type f -print0 | sort -z )
    else
        tests=( "${@:3}" )
    fi

    if ! podman container exists "$container_name"; then
        >&2 echo "The test environment VM isn't running. Start it with:"
        >&2 echo "   \$ $0 start"
        exit 1
    fi

    __build_runtime

    for t in "${tests[@]}"; do
        for engine in "${engines[@]}"; do

            __big_log 33 'Running test %s under %s...' "$( __rel "$t" )" "$engine"

            case "$engine" in
            docker)
                engine_cmd=( sudo docker )
                runtime_in_env=crun-vm
                ;;
            podman)
                engine_cmd=( podman )
                runtime_in_env=/home/fedora/target/debug/crun-vm
                ;;
            rootful-podman)
                engine_cmd=( sudo podman )
                runtime_in_env=/home/fedora/target/debug/crun-vm
                ;;
            esac

            # generate random label for containers created by test script
            label=$( mktemp --dry-run | xargs basename )

            # shellcheck disable=SC2317
            __engine() {
                if [[ "$1" == run ]]; then
                    __log_and_run "${engine_cmd[@]}" run \
                        --runtime "$runtime_in_env" \
                        --pull never \
                        --label "$label" \
                        "${@:2}"
                else
                    __log_and_run "${engine_cmd[@]}" "$@"
                fi
            }

            __exec() {
                podman exec -i "$container_name" --as fedora "$@"
            }

            # shellcheck disable=SC2088
            __exec mkdir "$label.temp" "$label.util"

            # copy util scripts

            for file in $repo_root/util/*; do
                contents=$( cat "$file" )
                path_in_vm=$label.util/$( basename "$file" )
                __exec "echo ${contents@Q} > $path_in_vm && chmod +x $path_in_vm"
            done

            # run test

            full_script="\
                set -o errexit -o pipefail -o nounset
                $(
                    declare -p \
                        TEST_IMAGES TEST_IMAGES_DEFAULT_USER TEST_IMAGES_DEFAULT_USER_HOME \
                        engine_cmd runtime_in_env label start_time
                    )
                $( declare -f __elapsed __engine __log_and_run __small_log )
                __skip() {
                    exit 0
                }
                __log() {
                    __small_log 36 \"\$@\"
                }
                TEMP_DIR=~/$label.temp
                UTIL_DIR=~/$label.util
                ENGINE=$engine
                export RUST_BACKTRACE=1 RUST_LIB_BACKTRACE=1
                $( cat "$t" )\
                "

            exit_code=0
            __exec <<< "$full_script" || exit_code=$?

            # remove any leftover containers

            __small_log 36 'Cleaning up...'

            full_script="\
                set -o errexit -o pipefail -o nounset
                ${engine_cmd[*]} ps --filter label=$label --format '{{.Names}}' |
                    xargs --no-run-if-empty ${engine_cmd[*]} stop --time 0 \
                    >/dev/null 2>&1
                ${engine_cmd[*]} ps --filter label=$label --format '{{.Names}}' --all |
                    xargs --no-run-if-empty ${engine_cmd[*]} rm --force \
                    >/dev/null 2>&1 \
                    || true  # avoid 'removal already in progress' docker errors
                ${engine_cmd[*]} ps --filter label=$label --format '{{.Names}}' --all |
                    xargs --no-run-if-empty false  # fail if containers still exist
                sudo rm -fr $label.temp $label.util
                "

            __exec <<< "$full_script"

            # report test result

            if (( exit_code == 0 )); then
                __small_log 36 'Test succeeded.'
            else
                __small_log 36 'Test failed.'
                __big_log 31 'A test failed.'
                exit "$exit_code"
            fi

        done
    done

    __big_log 32 'All tests succeeded.'
    ;;

ssh)
    __log_and_run podman exec -it "$container_name" --as fedora "${@:2}"
    ;;

*)
    __bad_usage
    ;;
esac
