#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

original_root=$1
priv_dir=$2
container_id=$3

__step() {
    printf "\033[36m%s\033[0m\n" "$*"
}

bootc_dir=$priv_dir/root/crun-vm/bootc

mkfifo "$bootc_dir/progress"
exec > "$bootc_dir/progress" 2>&1

# this blocks here until the named pipe above is opened by entrypoint.sh

# get info about the container *image*

image_info=$(
    podman container inspect \
        --format '{{.ImageName}}\t{{.Image}}' \
        "$container_id"
    )

image_name=$( cut -f1 <<< "$image_info" )
image_id=$( cut -f2 <<< "$image_info" )

# check if VM image is cached

container_name=crun-vm-$container_id

cache_image_label=crun-vm.from=$image_id
cache_image_id=$( podman images --filter "label=$cache_image_label" --format '{{.ID}}' --no-trunc )

if [[ -n "$cache_image_id" ]]; then

    # retrieve VM image from cached containerdisk

    __step "Retrieving cached VM image..."

    trap 'podman rm --force "$container_name" >/dev/null 2>&1 || true' EXIT

    podman create --quiet --name "$container_name" "$cache_image_id" </dev/null >/dev/null
    podman export "$container_name" | tar -C "$bootc_dir" -x image.qcow2
    podman rm "$container_name" >/dev/null 2>&1

    trap '' EXIT

else

    __step "Converting $image_name into a VM image..."

    # save container *image* as an OCI archive

    echo -n 'Preparing container image...'

    podman save \
        --format oci-archive \
        --output "$bootc_dir/image.oci-archive" \
        "$image_id" \
        </dev/null 2>&1 \
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

    truncate --size 10G "$bootc_dir/image.raw"  # TODO: allow adjusting disk size

    trap 'krun delete --force "$container_name" >/dev/null 2>&1 || true' EXIT
    krun run --config "$bootc_dir/config.json" "$container_name" </dev/ptmx
    trap '' EXIT

    [[ -e "$bootc_dir/bootc-install-success" ]]

    # convert image to qcow2 to get a lower file size

    qemu-img convert -f raw -O qcow2 "$bootc_dir/image.raw" "$bootc_dir/image.qcow2"
    rm "$bootc_dir/image.raw"

    # cache VM image file as containerdisk

    __step "Caching VM image as a containerdisk..."

    id=$(
        podman build --quiet --file - --label "$cache_image_label" "$bootc_dir" <<-'EOF'
        FROM scratch
        COPY image.qcow2 /
        ENTRYPOINT ["no-entrypoint"]
EOF
    )

    echo "Stored as untagged container image with ID $id"

fi

__step "Booting VM..."

touch "$bootc_dir/success"
