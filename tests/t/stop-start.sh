# SPDX-License-Identifier: GPL-2.0-or-later

__engine run --detach --name "$TEST_ID" "${TEST_IMAGES[fedora]}"

__engine exec "$TEST_ID" --as fedora '[[ ! -e i-was-here ]] && touch i-was-here'

for (( i = 0; i < 2; ++i )); do

    __engine stop "$TEST_ID"
    __engine start "$TEST_ID"

    __engine exec "$TEST_ID" --as fedora '[[ -e i-was-here ]]'

done
