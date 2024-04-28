# SPDX-License-Identifier: GPL-2.0-or-later

image="${TEST_IMAGES[fedora-bootc]}"
user="${TEST_IMAGES_DEFAULT_USER[fedora-bootc]}"

__run() {
    __engine run --detach --name "$TEST_ID" "$image" --bootc-disk-size "$1"
}

__run 1M
! __engine exec "$TEST_ID" --as "$user"
__engine rm --force "$TEST_ID"

__run 4G
__engine exec "$TEST_ID" --as "$user"
__engine rm --force "$TEST_ID"
