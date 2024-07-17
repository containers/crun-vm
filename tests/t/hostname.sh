# SPDX-License-Identifier: GPL-2.0-or-later

for os in "${!TEST_IMAGES[@]}"; do

    image="${TEST_IMAGES[$os]}"
    user="${TEST_IMAGES_DEFAULT_USER[$os]}"

    # default hostname

    id=$( __engine run --rm --detach --name "$TEST_ID-$os-default" "$image" )

    __test() {
        __engine exec "$TEST_ID-$os-default" --as "$user" \
            "set -x && [[ \$( hostname ) == ${id::12} ]]"
    }

    __test
    __engine restart "$TEST_ID-$os-default"
    __test

    __engine stop --time 0 "$TEST_ID-$os-default"

    # custom hostname

    __engine run \
        --rm --detach \
        --name "$TEST_ID-$os-custom" \
        --hostname my-test-vm \
        "$image"

    __test() {
        __engine exec "$TEST_ID-$os-custom" --as "$user" \
            "set -x && [[ \$( hostname ) == my-test-vm ]]"
    }

    __test
    __engine restart "$TEST_ID-$os-custom"
    __test

    __engine stop --time 0 "$TEST_ID-$os-custom"

done
