# SPDX-License-Identifier: GPL-2.0-or-later

image="${TEST_IMAGES[fedora-bootc]}"
user="${TEST_IMAGES_DEFAULT_USER[fedora-bootc]}"

__run() {
    __engine run --detach --name bootc-disk-size "$image" --bootc-disk-size "$1"
}

! RUST_LIB_BACKTRACE=0 __run 0
__engine rm bootc-disk-size

for size in 1K 1M; do
    __run "$size"
    ! __engine exec bootc-disk-size --as "$user"
    __engine rm --force bootc-disk-size
done

for size in 1G 1T 1024T; do
    __run "$size"
    __engine exec bootc-disk-size --as "$user"
    __engine rm --force bootc-disk-size
done
