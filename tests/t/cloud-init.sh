# SPDX-License-Identifier: GPL-2.0-or-later

for os in fedora fedora-bootc; do

    image="${TEST_IMAGES[$os]}"
    user="${TEST_IMAGES_DEFAULT_USER[$os]}"
    home="${TEST_IMAGES_DEFAULT_USER_HOME[$os]}"

    cat >"$TEMP_DIR/user-data" <<-EOF
    #cloud-config
    write_files:
      - path: $home/file
        content: |
          hello
EOF

    cat >"$TEMP_DIR/meta-data" <<-EOF
EOF

    __engine run \
        --rm --detach \
        --name "$TEST_ID" \
        "$image" \
        --cloud-init "$TEMP_DIR"

    __test() {
        __engine exec "$TEST_ID" --as "$user" "cmp $home/file <<< hello"
    }

    __test
    __engine restart "$TEST_ID"
    __test

    __engine stop "$TEST_ID"

done
