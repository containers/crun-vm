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

if command -v virtqemud; then
    virtqemud --daemon
    socket=/run/libvirt/virtqemud-sock
else
    libvirtd --daemon
    socket=/run/libvirt/libvirt-sock
fi

# libvirt doesn't let us pass --modcaps to virtiofsd (which we use to avoid
# having virtiofsd unsuccessfully attempt to acquire additional capabilities),
# so we tell libvirt to use the /crun-qemu/virtiofsd script below.
cat <<'EOF' >/crun-qemu/virtiofsd
#!/bin/bash
/usr/libexec/virtiofsd --modcaps=-mknod:-setfcap "$@"
EOF
chmod +x /crun-qemu/virtiofsd

# When running under Docker or rootful Podman, passt will realize that it is
# running as *actual* root and will switch to being user nobody, but this will
# make it fail to create its PID file because its directory was created by
# libvirt and is thus owned by root:root. Work around this by creating the
# directory ourselves and making it writable for others.
mkdir -p /run/libvirt/qemu/passt
chmod o+w /run/libvirt/qemu/passt

# add debugging helper script to run virsh
cat <<EOF >/crun-qemu/virsh
#!/bin/bash
virsh --connect "qemu+unix:///session?socket=$socket" "\$@"
EOF
chmod +x /crun-qemu/virsh

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
    --connect "qemu+unix:///session?socket=$socket" \
    --quiet \
    create \
    /crun-qemu/domain.xml \
    --console
