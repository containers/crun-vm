# SPDX-License-Identifier: GPL-2.0-or-later

for os in fedora coreos; do

    image="${TEST_IMAGES[$os]}"
    user="${TEST_IMAGES_DEFAULT_USER[$os]}"

    # default hostname

    id=$( __engine run --detach --name "hostname-$os-default" "$image" )

    __test() {
        __engine exec "hostname-$os-default" --as "$user" \
            "set -x && [[ \$( hostname ) == ${id::12} ]]"
    }

    __test
    __engine restart "hostname-$os-default"
    __test

    __engine stop --time 0 "hostname-$os-default"
    __engine rm "hostname-$os-default"

    # custom hostname

    __engine run \
        --detach \
        --name "hostname-$os-custom" \
        --hostname my-test-vm \
        "$image"

    __test() {
        __engine exec "hostname-$os-custom" --as "$user" \
            "set -x && [[ \$( hostname ) == my-test-vm ]]"
    }

    __test
    __engine restart "hostname-$os-custom"
    __test

    __engine stop --time 0 "hostname-$os-custom"
    __engine rm "hostname-$os-custom"

done
