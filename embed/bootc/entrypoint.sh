#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later

set -e

image_name=$1

# monkey-patch loopdev partition detection, given we're not running systemd
# (bootc runs `udevadm settle` as a way to wait until loopdev partitions are
# detected; we hijack that call and use partx to set up the partition devices)

original_udevadm=$( which udevadm )

mkdir -p /output/bin

cat >/output/bin/udevadm <<EOF
#!/bin/sh
${original_udevadm@Q} "\$@" && partx --add /dev/loop0
EOF

chmod +x /output/bin/udevadm

# default to an xfs root file system if there is no bootc config (some images
# don't currently provide any, for instance quay.io/fedora/fedora-bootc:40)

if ! find /usr/lib/bootc/install -mindepth 1 -maxdepth 1 | read; then
    # /usr/lib/bootc/install is empty

    cat >/usr/lib/bootc/install/00-crun-vm.toml <<-EOF
    [install.filesystem.root]
    type = "xfs"
EOF

fi

# build disk image using bootc-install

PATH=/output/bin:$PATH bootc install to-disk \
    --source-imgref oci-archive:/output/image.oci-archive \
    --target-imgref "$image_name" \
    --skip-fetch-check \
    --generic-image \
    --via-loopback \
    --karg console=tty0 \
    --karg console=ttyS0 \
    --karg selinux=0 \
    /output/image.raw

# communicate success by creating a file, since krun always exits successfully

touch /output/bootc-install-success
