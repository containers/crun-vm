# SPDX-License-Identifier: GPL-2.0-or-later

"$UTIL_DIR/extract-vm-image.sh" "${TEST_IMAGES[fedora-bootc]}" "$TEMP_DIR/image"

__run() {
    __engine run --rm --detach --name bootc-rootfs "$@" --rootfs "$TEMP_DIR"
}

! __run
! __run --persistent
