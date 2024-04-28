# SPDX-License-Identifier: GPL-2.0-or-later

__engine run \
    --detach \
    --name "$TEST_ID" \
    "${TEST_IMAGES[fedora]}" \
    --random-ssh-key-pair

__engine exec "$TEST_ID" --as fedora
__engine restart "$TEST_ID"
__engine exec "$TEST_ID" --as fedora
