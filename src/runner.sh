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

# disable libvirt cgroups management, since we're already in a container
echo 'cgroup_controllers = []' >> /etc/libvirt/qemu.conf

# we run virtiofsd as non-root so it doesn't try to drop capabilities, which
# doesn't seem to work in unprivileged containers
groupadd virtiofsd
useradd --system --key MAIL_DIR=/dev/null --group virtiofsd virtiofsd

# so qemu can access the virtiofsd sockets
usermod --append --groups virtiofsd qemu

virtlogd --daemon
virtqemud --daemon

# pass bind mounts through to the VM using virtiofs

mkdir -p /crun-qemu/mounts/virtiofsd
chown virtiofsd:virtiofsd /crun-qemu/mounts/virtiofsd

# we run virtiofsd as non-root (see Containerfile), thus we can't use
# --sandbox=chroot, but libvirt doesn't allow --sanbox=none, so we launch
# virtiofsd ourselves instead
mapfile -t mount_ids < <( find /crun-qemu/mounts -name '*[0-9]' -printf '%f\n' )
for i in "${mount_ids[@]}"; do
    runuser -u virtiofsd -g virtiofsd -- /usr/libexec/virtiofsd \
        --shared-dir "/crun-qemu/mounts/$i" \
        --socket-path "/crun-qemu/mounts/virtiofsd/$i" \
        --sandbox none \
        &>"/crun-qemu/mounts/virtiofsd/$i.log" \
        &
done

for i in "${mount_ids[@]}"; do
    while [[ ! -e "/crun-qemu/mounts/virtiofsd/$i" ]]; do sleep 0.1; done
    chmod g+rw "/crun-qemu/mounts/virtiofsd/$i"
done

# launch VM

virsh \
    --connect qemu+unix:///session?socket=/run/libvirt/virtqemud-sock \
    --quiet \
    create \
    /crun-qemu/domain.xml \
    --console
