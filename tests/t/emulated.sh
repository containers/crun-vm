# SPDX-License-Identifier: GPL-2.0-or-later

__engine run --detach --name emulated "${TEST_IMAGES[fedora]}" --emulated
__engine exec emulated --as fedora
