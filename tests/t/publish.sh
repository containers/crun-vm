# SPDX-License-Identifier: GPL-2.0-or-later

trap '__engine stop "$TEST_ID"' EXIT

for os in fedora fedora-bootc; do

    image="${TEST_IMAGES[$os]}"
    user="${TEST_IMAGES_DEFAULT_USER[$os]}"

    __engine run --rm --detach --name "$TEST_ID" --publish 127.0.0.1::8000 "$image"

    endpoint=$( __engine port "$TEST_ID" | tee /dev/stderr | cut -d' ' -f3 )

    __engine exec "$TEST_ID" --as "$user"

    __log 'Ensuring curl fails...'
    ! curl "$endpoint" 2>/dev/null

    __engine exec "$TEST_ID" --as "$user" python -m http.server &

    __log 'Ensuring curl succeeds...'

    i=0
    max_tries=30

    until [[ "$( curl "$endpoint" 2>/dev/null )" == '<!DOCTYPE HTML>'* ]]; do
        (( ++i < max_tries ))
        sleep 1
    done

    __engine stop "$TEST_ID"

done
