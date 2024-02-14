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

# libvirt doesn't let us pass --modcaps to virtiofsd (which we use to avoid
# having virtiofsd unsuccessfully attempt to acquire additional capabilities),
# so we tell libvirt to use the /crun-vm/virtiofsd script below.
cat <<'EOF' >/crun-vm/virtiofsd
#!/bin/bash
/usr/libexec/virtiofsd --modcaps=-mknod:-setfcap "$@"
EOF
chmod +x /crun-vm/virtiofsd

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

cat <<'EOF' >/crun-vm/exec.sh
#!/bin/bash

set -e

__ssh() {
    ssh \
        -o LogLevel=ERROR \
        -o StrictHostKeyChecking=no \
        -l "$1" \
        localhost \
        "${@:2}"
}

if [[ ! -e /crun-vm/ssh-successful ]]; then

    # retry ssh for some time, ignoring some common errors

    for (( i = 0; i < 60; ++i )); do

        set +e
        output=$( __ssh "$1" </dev/null 2>&1 )
        exit_code=$?
        set -e

        sleep 1

        if (( exit_code != 255 )) ||
            ! grep -iqE "Connection refused|Connection reset by peer|Connection closed by remote host" <<< "$output"; then
            break
        fi

    done

    if (( exit_code != 0 )); then
        >&2 printf '%s\n' "$output"
        exit "$exit_code"
    fi

    # avoid these steps next time

    touch /crun-vm/ssh-successful

fi

__ssh "$1" -- "${@:2}"
EOF
chmod +x /crun-vm/exec.sh

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

# If our container was stopped and is being restarted, the domain may still be
# defined from the previous run, which would cause `virsh define` below to fail,
# so we first undefine it.
"${virsh[@]}" undefine domain &>/dev/null || true

"${virsh[@]}" define /crun-vm/domain.xml

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
