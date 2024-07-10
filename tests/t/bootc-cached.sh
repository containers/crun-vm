# SPDX-License-Identifier: GPL-2.0-or-later

image="${TEST_IMAGES[fedora-bootc]}"
user="${TEST_IMAGES_DEFAULT_USER[fedora-bootc]}"

__drop_cached() {
    mapfile -d '' -t __cached < <( __engine images --filter=label=crun-vm.from --format '{{.ID}}' --no-trunc )
    if (( ${#__cached[@]} > 0 )); then
        __engine rmi "${__cached[@]}"
    fi
}

__test() {
    __engine run --detach --name "$TEST_ID" "$image"
    __engine exec "$TEST_ID" --as "$user"
    __engine rm --force "$TEST_ID"
}

__drop_cached
__test
__test
