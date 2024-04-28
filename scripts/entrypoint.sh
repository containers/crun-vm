#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

trap 'exit 143' SIGTERM

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

if command -v virtqemud >/dev/null; then
    virtqemud --daemon
    socket=/run/libvirt/virtqemud-sock
else
    libvirtd --daemon
    socket=/run/libvirt/libvirt-sock
fi

# When running under Docker or rootful Podman, passt will realize that it is
# running as *actual* root and will switch to being user nobody, but this will
# make it fail to create its PID file because its directory was created by
# libvirt and is thus owned by root:root. Work around this by creating the
# directory ourselves and making it writable for others.
mkdir -p /run/libvirt/qemu/passt
chmod o+w /run/libvirt/qemu/passt

# add debugging helper script to run virsh
cat <<EOF >/crun-vm/virsh
#!/bin/bash
virsh --connect "qemu+unix:///session?socket=$socket" "\$@"
EOF
chmod +x /crun-vm/virsh

# launch VM

function __bg_ensure_tty() {
    if [[ -t 0 ]]; then
        # stay attach to tty when running in background
        "$@" <"$(tty)" &
    else
        # virsh console requires stdin to be a tty
        script --return --quiet /dev/null --command "${*@Q}" &
    fi
}

virsh=( virsh --connect "qemu+unix:///session?socket=$socket" --quiet )

if [[ -z "$( "${virsh[@]}" list --all --name )" ]]; then
    "${virsh[@]}" define /crun-vm/domain.xml
fi

# trigger graceful shutdown and wait for VM to terminate
function __shutdown() {
    (
        set -o errexit -o pipefail -o nounset
        "${virsh[@]}" shutdown domain 2>/dev/null
        while ! "${virsh[@]}" domstate domain 2>/dev/null |
            grep --quiet 'shut off'; do
            sleep 0.1
            # if we caught the VM booting, we may need to signal shutdown again
            "${virsh[@]}" shutdown domain 2>/dev/null
        done
    )
}

# We're running as PID 1, so if we run virsh in the foreground, SIGTERM will not
# be propagated to it. We thus run it in the background but keep our tty
# attached to its stdin. We then set up a trap that attempts to gracefully
# terminate the VM on SIGTERM, and finally block waiting for virsh to exit.
__bg_ensure_tty "${virsh[@]}" start domain --console
trap '__shutdown || true; exit 143' SIGTERM
wait
