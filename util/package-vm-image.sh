#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

if (( $# != 2 )); then
    >&2 echo "Usage: $0 <vm_image_file> <container_image_tag>"
    >&2 echo "Package a given VM image file into a container image and tag it."
    exit 2
fi

vm_image_file=$1
container_image_tag=$2

image_path_in_container=/disk/$( basename "${vm_image_file}" )

podman image build --file=- --tag="${container_image_tag}" . <<EOF
FROM scratch
COPY ${vm_image_file@Q} ${image_path_in_container@Q}
EOF
