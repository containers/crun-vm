# SPDX-License-Identifier: GPL-2.0-or-later

if [[ "$ENGINE" == docker ]]; then
    # docker doesn't support --rootfs
    __skip
fi

"$UTIL_DIR/extract-vm-image.sh" "${TEST_IMAGES[fedora]}" "$TEMP_DIR/image"

# Usage: __run <crun_vm_option> [<extra_podman_options...>]
__run() {
    __engine run --rm --detach --name persistent "${@:2}" --rootfs "$TEMP_DIR" "$1"
}

# Usage: __test <crun_vm_option> <condition>
__test() {
    id=$( __run "$1" )
    __engine exec persistent --as fedora "$2"
    __engine stop persistent

    if [[ "$ENGINE" != rootful-podman ]]; then
        # ensure user that invoked `engine run` can delete crun-vm state
        rm -r "$TEMP_DIR/crun-vm-$id"
    fi
}

__test ""           '[[ ! -e i-was-here ]] && touch i-was-here'
__test --persistent '[[ ! -e i-was-here ]] && touch i-was-here'
__test --persistent '[[ -e i-was-here ]]'
__test ""           '[[ -e i-was-here ]]'

# ensure --persistent is rejected iff the rootfs is configured as read-only

! RUST_LIB_BACKTRACE=0 __run --persistent --read-only

__run "" --read-only
__engine exec persistent --as fedora
