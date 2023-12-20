# The `crun-qemu` OCI runtime

This is an **experimental** [OCI Runtime] that enables Podman to run VM images.
The objective is to make running VMs (in simple configurations) as easy as
running containers.

## Building the runtime

```console
$ dnf install bash coreutils crun genisoimage libvirt-client libvirt-daemon-driver-qemu libvirt-daemon-log qemu-img shadow-utils util-linux virtiofsd
$ cargo build
```

## Overview

Below we overview some of the major features provided by `crun-qemu`.

### Booting VMs

#### From regular VM image files

First, obtain a QEMU-compatible VM image and place it in a directory by itself:

```console
$ mkdir my-vm-image
$ curl -LO --output-dir my-vm-image https://download.fedoraproject.org/pub/fedora/linux/releases/39/Cloud/x86_64/images/Fedora-Cloud-Base-39-1.5.x86_64.qcow2
```

Then try it out:

```console
$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    --rootfs my-vm-image \
    ""
```

The VM console should take over your terminal. To abort the VM, press `ctrl-]`.

You can also detach from the VM without terminating it by pressing `ctrl-p,
ctrl-q`. Afterwards, reattach by running:

```console
$ podman attach --latest
```

This command also works when you start the VM in detached mode using
podman-run's `-d`/`--detach` flags.

It's also possible to omit flags `-i`/`--interactive` and `-t`/`--tty` to
podman-run, in which case you won't be able to interact with the VM but can
still observe its console. Note that pressing `ctrl-]` will have no effect, so
use `podman container rm --force --time=0 ...` to terminate the VM instead.

#### From VM image files packaged into container images

This runtime also works with container images that contain a VM image file with
any name under `/` or under `/disk/`. No other files may exist in those
directories. Containers built for use as [KubeVirt `containerDisk`s] follow this
convention, so you can use those here:

```console
$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    quay.io/containerdisks/fedora:39 \
    ""
```

You can also use `util/package-vm-image.sh` to easily package a VM image into a
container image, and `util/extract-vm-image.sh` to extract a VM image contained
in a container image.

### First-boot customization

#### cloud-init

In the examples above, you were able to boot the VM but not to login. To fix
this and do other first-boot customization, you can provide a [cloud-init]
NoCloud configuration to the VM by passing in the non-standard option
`--cloud-init` *after* the image specification:

```console
$ ls examples/cloud-init/config/
meta-data  user-data  vendor-data

$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    quay.io/containerdisks/fedora:39 \
    --cloud-init examples/cloud-init/config
```

You should now be able to login with the default `fedora` username and password
`pass`.

#### Ignition

Similarly, you can provide an [Ignition] configuration to the VM by passing in
the `--ignition` option:

```console
$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    quay.io/crun-qemu/fedora-coreos:39 \
    --ignition examples/ignition/config.ign
```

You should now be able to login with the default `core` username and password
`pass`.

### SSH'ing into the VM

Assuming the VM supports cloud-init and runs an SSH server, you can `ssh` into
it using podman-exec as whatever user cloud-init considers to be the default for
your VM image:

```console
$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    --detach --rm \
    quay.io/containerdisks/fedora:39 \
    ""
47e7aab2b1a52054f5904ccb108854db23885258f1bd0d87241740212f4fc9af

$ podman exec --latest fedora whoami
fedora

$ podman exec --latest -it fedora
[fedora@ibm-p8-kvm-07-guest-07 ~]$
```

The `fedora` argument to podman-exec above, which would typically correspond to
the command to be executed, determines instead the name of the user to `ssh`
into the guest as. A command can optionally be specified with further arguments.
If no command is specified, a login shell is initiated. In this case, you
probably also want to pass flags `-it` to podman-exec.

If you actually just want to exec into the container in which the VM is running
(probably to debug some problem with `crun-qemu` itself), pass in `-` as the
username.

### Passing things through to the VM

#### Directory bind mounts

Bind mounts are passed through to the VM as [virtiofs] file systems:

```console
$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    -v ./util:/home/fedora/util \
    quay.io/containerdisks/fedora:39 \
    --cloud-init examples/cloud-init/config
```

If the VM image support cloud-init, the volume will automatically be mounted
inside the guest at the given destination path. Otherwise, you can mount it
manually with:

```console
mount -t virtiofs /home/fedora/util /home/fedora/util
```

#### Block devices

If cloud-init is available, it is possible to pass block devices through to the
VM at a specific path using podman-run's `--device` flag (this example assumes
`/dev/ram0` to exist and to be accessible by the current user):

```console
$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    --device /dev/ram0:/path/in/vm/my-disk \
    quay.io/containerdisks/fedora:39 \
    --cloud-init examples/cloud-init/config
```

You can also pass them in as bind mounts using the `-v`/`--volume` or `--mount`
flags.

#### Mediated (mdev) vfio-pci devices

Mediated vfio-pci devices (such as vGPUs) can be passed through to the VM by
specifying the non-standard `--vfio-pci-mdev` option with a path to the mdev's
sysfs directory:

```console
$ podman run \
    --runtime "$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    quay.io/containerdisks/fedora:39 \
    --vfio-pci-mdev /sys/bus/pci/devices/0000:00:02.0/5fa530b9-9fdf-4cde-8eb7-af73fcdeeaae
```

## How it works

Internally, `crun-qemu` uses [crun] to run a different container that in turn
uses [libvirt] to run a [QEMU] guest using the user-specified VM image.

## License

This project is released under the GPL 2.0 (or later) license. See
[LICENSE](LICENSE).

[cloud-init]: https://cloud-init.io/
[crun]: https://github.com/containers/crun
[KubeVirt `containerDisk`s]: https://kubevirt.io/user-guide/virtual_machines/disks_and_volumes/#containerdisk
[libvirt]: https://libvirt.org/
[Ignition]: https://coreos.github.io/ignition/
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
[QEMU]: https://www.qemu.org/
[virtiofs]: https://virtio-fs.gitlab.io/
