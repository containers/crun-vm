# SPDX-License-Identifier: GPL-2.0-or-later

image="${TEST_IMAGES[fedora]}"
user="${TEST_IMAGES_DEFAULT_USER[fedora]}"
home="${TEST_IMAGES_DEFAULT_USER_HOME[fedora]}"

cat >"$TEMP_DIR/user-data" <<EOF
#cloud-config
write_files:
  - path: $home/file
    content: |
      hello
EOF

cat >"$TEMP_DIR/meta-data" <<EOF
EOF

__engine run --detach --name cloud-init "$image" --cloud-init "$TEMP_DIR"

__test() {
    __engine exec cloud-init --as "$user" "cmp $home/file <<< hello"
}

__test
__engine restart cloud-init
__test
