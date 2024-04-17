# SPDX-License-Identifier: GPL-2.0-or-later

__engine run \
    --detach \
    --name random-ssh-key-pair \
    "${TEST_IMAGES[fedora]}" \
    --random-ssh-key-pair

__engine exec random-ssh-key-pair --as fedora
