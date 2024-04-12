# SPDX-License-Identifier: GPL-2.0-or-later

image="${TEST_IMAGES[coreos]}"
user="${TEST_IMAGES_DEFAULT_USER[coreos]}"
home="${TEST_IMAGES_DEFAULT_USER_HOME[coreos]}"

cat >"$TEMP_DIR/config.ign" <<EOF
{
  "ignition": {
    "version": "3.0.0"
  },
  "storage": {
    "files": [
      {
        "path": "$home/file",
        "mode": 420,
        "overwrite": true,
        "contents": {
          "source": "data:,hello%0A"
        }
      }
    ]
  }
}
EOF

__engine run \
    --rm --detach \
    --name ignition \
    "$image" \
    --ignition "$TEMP_DIR/config.ign"

__test() {
    __engine exec ignition --as "$user" "cmp $home/file <<< hello"
}

__test
__engine restart ignition
__test
