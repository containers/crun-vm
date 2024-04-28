# SPDX-License-Identifier: GPL-2.0-or-later

image="${TEST_IMAGES[fedora]}"
user="${TEST_IMAGES_DEFAULT_USER[fedora]}"

__engine run \
    --rm --detach \
    --name publish \
    --publish 127.0.0.1::8000 \
    "$image"

endpoint=$( __engine port publish | tee /dev/stderr | cut -d' ' -f3 )

__engine exec publish --as "$user"

__log 'Ensuring curl fails...'
! curl "$endpoint" 2>/dev/null

__engine exec publish --as "$user" python -m http.server &
trap '__engine stop publish' EXIT

__log 'Ensuring curl succeeds...'

i=0
max_tries=30

until [[ "$( curl "$endpoint" 2>/dev/null )" == '<!DOCTYPE HTML>'* ]]; do
    (( ++i < max_tries ))
    sleep 1
done
