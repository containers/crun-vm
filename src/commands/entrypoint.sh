#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

mkdir -p \
    /etc/libvirt \
    /tmp \
    /var/lib/sss/db/ \
    /var/lock \
    /var/log/libvirt \
    /var/run/libvirt

# avoid "Unable to set XATTR trusted.libvirt.security.dac" error
echo 'remember_owner = 0' >> /etc/libvirt/qemu.conf

# avoid having libvirt change the VM image file's ownership, and run QEMU as the
# user running the container so that it can still access the image
echo 'dynamic_ownership = 0' >> /etc/libvirt/qemu.conf
echo 'user = "root"' >> /etc/libvirt/qemu.conf
echo 'group = "root"' >> /etc/libvirt/qemu.conf

# disable libvirt cgroups management, since we're already in a container
echo 'cgroup_controllers = []' >> /etc/libvirt/qemu.conf

virtlogd --daemon
virtqemud --daemon

# pass bind mounts through to the VM using virtiofs

mkdir -p /crun-qemu/mounts/virtiofsd

mapfile -t mount_ids < <( find /crun-qemu/mounts -name '*[0-9]' -printf '%f\n' )
for i in "${mount_ids[@]}"; do
    /usr/libexec/virtiofsd \
        --modcaps=-mknod \
        --shared-dir "/crun-qemu/mounts/$i" \
        --socket-path "/crun-qemu/mounts/virtiofsd/$i" \
        --sandbox chroot \
        &>"/crun-qemu/mounts/virtiofsd/$i.log" \
        &
done

# launch VM

function __ensure_tty() {
    if [[ -t 0 ]]; then
        "$@"
    else
        # 'virsh console' requires stdin to be a tty
        script --return --quiet /dev/null --command "${*@Q}"
    fi
}

__ensure_tty virsh \
    --connect qemu+unix:///session?socket=/run/libvirt/virtqemud-sock \
    --quiet \
    create \
    /crun-qemu/domain.xml \
    --console
