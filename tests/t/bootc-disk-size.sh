# SPDX-License-Identifier: GPL-2.0-or-later

image="${TEST_IMAGES[fedora-bootc]}"
user="${TEST_IMAGES_DEFAULT_USER[fedora-bootc]}"

__run() {
    __engine run --detach --name bootc-disk-size "$image" --bootc-disk-size "$1"
}

__run 1M
! __engine exec bootc-disk-size --as "$user"
__engine rm --force bootc-disk-size

__run 4G
__engine exec bootc-disk-size --as "$user"
__engine rm --force bootc-disk-size
