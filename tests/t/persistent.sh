# SPDX-License-Identifier: GPL-2.0-or-later

if [[ "$ENGINE" == docker ]]; then
    # docker doesn't support --rootfs
    __skip
fi

"$UTIL_DIR/extract-vm-image.sh" "${TEST_IMAGES[fedora]}" "$TEMP_DIR/image"

__test() {
    id=$( __engine run --detach --name persistent --rootfs "$TEMP_DIR" "$1" )

    __engine exec persistent --as fedora "$2"

    __engine stop persistent
    __engine rm persistent

    if [[ "$ENGINE" != rootful-podman ]]; then
        # ensure user that invoked `engine run` can delete crun-vm state
        rm -r "$TEMP_DIR/crun-vm-$id"
    fi
}

__test ""           '[[ ! -e i-was-here ]] && touch i-was-here'
__test --persistent '[[ ! -e i-was-here ]] && touch i-was-here'
__test --persistent '[[ -e i-was-here ]]'
__test ""           '[[ -e i-was-here ]]'
