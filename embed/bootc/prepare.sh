#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

engine=$1
container_id=$2
original_root=$3
priv_dir=$4
disk_size=$5

__step() {
    printf "\033[36m%s\033[0m\n" "$*"
}

bootc_dir=$priv_dir/root/crun-vm/bootc

mkfifo "$bootc_dir/progress"
exec > "$bootc_dir/progress" 2>&1

# this blocks here until the named pipe above is opened by entrypoint.sh

# get info about the container *image*

image_info=$(
    "$engine" container inspect \
        --format '{{.Config.Image}}'$'\t''{{.Image}}' \
        "$container_id"
    )

image_name=$( cut -f1 <<< "$image_info" )
# image_name=${image_name#sha256:}

image_id=$( cut -f2 <<< "$image_info" )

# check if VM image is cached

container_name=crun-vm-$container_id

cache_image_label=containers.crun-vm.from=$image_id
cache_image_id=$( "$engine" images --filter "label=$cache_image_label" --format '{{.Id}}' )

if [[ -n "$cache_image_id" ]]; then

    # retrieve VM image from cached containerdisk

    __step "Retrieving cached VM image..."

    trap '"$engine" rm --force "$container_name" >/dev/null 2>&1 || true' EXIT

    "$engine" create --quiet --name "$container_name" "$cache_image_id" >/dev/null
    "$engine" export "$container_name" | tar -C "$bootc_dir" -x image.qcow2
    "$engine" rm "$container_name" >/dev/null 2>&1

    trap '' EXIT

else

    __step "Converting $image_name into a VM image..."

    # save container *image* as an archive

    echo -n 'Preparing container image...'

    "$engine" save --output "$bootc_dir/image.docker-archive" "$image_id" 2>&1 \
        | sed -u 's/.*/./' \
        | stdbuf -o0 tr -d '\n'

    echo

    # adjust krun config

    __sed() {
        sed -i "s|$1|$2|" "$bootc_dir/config.json"
    }

    __sed "<IMAGE_NAME>"    "$image_name"
    __sed "<ORIGINAL_ROOT>" "$original_root"
    __sed "<PRIV_DIR>"      "$priv_dir"

    # run bootc-install under krun

    if [[ -z "$disk_size" ]]; then
        container_image_size=$(
            "$engine" image inspect --format '{{.VirtualSize}}' "$image_id"
            )

        # use double the container image size to allow for in-place updates
        disk_size=$(( container_image_size * 2 ))

        # round up to 1 MiB
        alignment=$(( 2**20 ))
        disk_size=$(( (disk_size + alignment - 1) / alignment * alignment ))
    fi

    truncate --size "$disk_size" "$bootc_dir/image.raw"

    trap 'krun delete --force "$container_name" >/dev/null 2>&1 || true' EXIT
    krun run --config "$bootc_dir/config.json" "$container_name" </dev/ptmx
    trap '' EXIT

    [[ -e "$bootc_dir/bootc-install-success" ]]

    # convert image to qcow2 to get a lower file length

    qemu-img convert -f raw -O qcow2 "$bootc_dir/image.raw" "$bootc_dir/image.qcow2"
    rm "$bootc_dir/image.raw"

    # cache VM image file as containerdisk

    __step "Caching VM image as a containerdisk..."

    id=$(
        "$engine" build --quiet --file - --label "$cache_image_label" "$bootc_dir" <<-'EOF'
        FROM scratch
        COPY image.qcow2 /
        ENTRYPOINT ["no-entrypoint"]
EOF
    )

    echo "Stored as untagged container image with ID $id"

fi

__step "Booting VM..."

touch "$bootc_dir/success"
