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

__step 'Storing the container image as an OCI archive...'

image_info=$(
    podman container inspect \
        --format '{{.ImageName}}\t{{.Image}}' \
        "$container_id"
    )

image_name=$( cut -f1 <<< "$image_info" )
image_id=$( cut -f2 <<< "$image_info" )

oci_archive=$bootc_dir/image.oci-archive

# save container *image* as an OCI archive

podman save --format oci-archive --output "$oci_archive.tmp" "$image_id" </dev/null
mv "$oci_archive.tmp" "$oci_archive"

# adjust krun config

__step 'Generating a VM image from the container image...'

__sed() {
    sed -i "s|$1|$2|" "$bootc_dir/config.json"
}

__sed "<IMAGE_NAME>"    "$image_name"
__sed "<ORIGINAL_ROOT>" "$original_root"
__sed "<PRIV_DIR>"      "$priv_dir"

# run bootc-install under krun

truncate --size 10G "$bootc_dir/image.raw"  # TODO: allow adjusting disk size

krun run \
    --config "$bootc_dir/config.json" \
    "crun-vm-$container_id" \
    </dev/ptmx

[[ -e "$bootc_dir/success" ]]

__step 'Booting VM...'
