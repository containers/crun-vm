#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

if (( $# != 2 )); then
    >&2 echo "Usage: $0 <container_image_tag> <output_vm_image_file>"
    >&2 echo "Extract a VM image file from a container image."
    exit 2
fi

container_image_tag=$1
output_vm_image_file=$2

temp_dir=$( mktemp -d )
trap 'rm -fr "${temp_dir}"' EXIT

container_id=$( podman container create --quiet "${container_image_tag}" "" )
trap 'podman container rm "${container_id}" >/dev/null; rm -fr "${temp_dir}"' EXIT

podman container export -o "${temp_dir}/root.tar" "${container_id}"
mapfile -t candidates < <( tar -tf "${temp_dir}/root.tar" | grep -xP '[^/]+|disk/[^/]+' )

if (( ${#candidates[@]} == 0 )); then
    >&2 echo "Error: found no VM image file in the container image"
    exit 1
elif (( ${#candidates[@]} > 1 )); then
    >&2 echo "Error: found more than one VM image file in the container image"
    exit 1
fi

tar -C "${temp_dir}" -xf "${temp_dir}/root.tar" "${candidates[0]}"
chmod +w "${temp_dir}/${candidates[0]}"

mkdir -p "$( dirname "${output_vm_image_file}" )"
mv -f "${temp_dir}/${candidates[0]}" "${output_vm_image_file}"
