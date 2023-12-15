#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

virtlogd --daemon
virtqemud --daemon

chmod u+w /vm/image

# provide NoCloud cloud-init config to the VM

if [[ -e /vm/cloud-init ]]; then
    find /vm/cloud-init -mindepth 1 -print0 | xargs -0 genisoimage \
        -output /vm/cloud-init.iso \
        -volid cidata \
        -joliet \
        -rock \
        -quiet
fi

# pass bind mounts through to the VM using virtiofs

mkdir -p /vm/mounts/virtiofsd
chown virtiofsd:virtiofsd /vm/mounts/virtiofsd

# we run virtiofsd as non-root (see Containerfile), thus we can't use
# --sandbox=chroot, but libvirt doesn't allow --sanbox=none, so we launch
# virtiofsd ourselves instead
mapfile -t mount_ids < <( find /vm/mounts -name '*[0-9]' -printf '%f\n' )
for i in "${mount_ids[@]}"; do
    sudo -u virtiofsd /usr/libexec/virtiofsd \
        --shared-dir "/vm/mounts/$i" \
        --socket-path "/vm/mounts/virtiofsd/$i" \
        --sandbox none \
        2>"/vm/mounts/virtiofsd/$i.log" \
        &
done

for i in "${mount_ids[@]}"; do
    while [[ ! -e "/vm/mounts/virtiofsd/$i" ]]; do sleep 0.1; done
    chmod g+rw "/vm/mounts/virtiofsd/$i"
done

# launch VM

virsh --connect qemu:///session --quiet create /vm/domain.xml --console
