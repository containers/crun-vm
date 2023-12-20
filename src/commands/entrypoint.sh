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

# libvirt doesn't let us pass --modcaps=-mknod to virtiofsd (which is necessary
# since we ourselves don't have that capability and virtiofsd would fail trying
# to add it), so we tell libvirt to use the /crun-qemu/virtiofsd script below

cat <<'EOF' >/crun-qemu/virtiofsd
#!/bin/bash
/usr/libexec/virtiofsd --modcaps=-mknod "$@"
EOF

chmod +x /crun-qemu/virtiofsd

# When running under Docker or rootful Podman, passt will realize that it is
# running as *actual* root and will switch to being user nobody, but this will
# make it fail to create its PID file because its directory was created by
# libvirt and is thus owned by root:root.
#
# We get around that by providing a wrapper script around passt that first gives
# "others" write access to the directory. This wrapper has already been bind
# mounted onto /usr/bin/passt at this point.
cat <<EOF >>/crun-qemu/passt/wrapper
#!/bin/bash
chmod o+w /run/libvirt/qemu/passt/ && /crun-qemu/passt/passt "\$@"
EOF
chmod +x /crun-qemu/passt/wrapper

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
