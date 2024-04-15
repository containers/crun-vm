# SPDX-License-Identifier: GPL-2.0-or-later

__engine run --detach --name stop-start "${TEST_IMAGES[fedora]}" ""

__engine exec stop-start --as fedora '[[ ! -e i-was-here ]] && touch i-was-here'

for (( i = 0; i < 2; ++i )); do

    __engine stop stop-start
    __engine start stop-start

    __engine exec stop-start --as fedora '[[ -e i-was-here ]]'

done
