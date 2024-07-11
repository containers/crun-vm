# SPDX-License-Identifier: GPL-2.0-or-later

for os in "${!TEST_IMAGES[@]}"; do

    image="${TEST_IMAGES[$os]}"
    user="${TEST_IMAGES_DEFAULT_USER[$os]}"
    home="${TEST_IMAGES_DEFAULT_USER_HOME[$os]}"

    echo hello > "$TEMP_DIR/file"

    __engine run \
        --rm --detach \
        --name "$TEST_ID-$os" \
        --volume "$TEMP_DIR/file:$home/file:z" \
        --volume "$TEMP_DIR:$home/dir:z" \
        --mount "type=tmpfs,dst=$home/tmp" \
        "$image"

    __test() {
        __engine exec "$TEST_ID-$os" --as "$user"

        __engine exec "$TEST_ID-$os" --as "$user" "
            set -e
            [[ -b $home/file ]]
            sudo cmp -n 6 $home/file <<< hello
            "

        __engine exec "$TEST_ID-$os" --as "$user" "
            set -e
            mount -l | grep '^virtiofs-0 on $home/dir type virtiofs'
            [[ -d $home/dir ]]
            sudo cmp $home/dir/file <<< hello
            "

        __engine exec "$TEST_ID-$os" --as "$user" "
            mount -l | grep '^tmpfs on $home/tmp type tmpfs'
            "
    }

    __test
    __engine restart "$TEST_ID-$os"
    __test

    __engine stop --time 0 "$TEST_ID-$os"

done
