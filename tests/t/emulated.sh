# SPDX-License-Identifier: GPL-2.0-or-later

__engine run --detach --name "$TEST_ID" "${TEST_IMAGES[fedora]}" --emulated
__engine exec "$TEST_ID" --as fedora
