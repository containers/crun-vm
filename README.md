# The `crun-qemu` OCI runtime

This is an **experimental** [OCI Runtime] that enables `podman run` to work with
VM images. The objective is to make running VMs (in simple configurations) as
easy as running containers.

## Trying it out

First build the runtime:

```console
$ dnf install bash coreutils crun genisoimage libvirt-client libvirt-daemon-driver-qemu libvirt-daemon-log qemu-img shadow-utils util-linux virtiofsd
$ cargo build
```

Then obtain a QEMU-compatible VM image and place it in a directory by itself:

```console
$ mkdir my-vm-image
$ curl -LO --output-dir my-vm-image https://download.fedoraproject.org/pub/fedora/linux/releases/39/Cloud/x86_64/images/Fedora-Cloud-Base-39-1.5.x86_64.qcow2
```

And try it out:

```console
$ podman run \
    --runtime="$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    --rootfs my-vm-image \
    unused
```

The VM console should take over your terminal. To abort the VM, press `ctrl-]`.

You can also detach from the VM without terminating it by pressing `ctrl-p,
ctrl-q`. Afterwards, reattach by running:

```console
$ podman attach --latest
```

## Using containerized VM images

This runtime also works with container images that contain a VM image file with
any name under `/` or under `/disk/`. No other files may exist in those
directories. Containers built for use as [KubeVirt `containerDisk`s] follow this
convention, so you can use those here:

```console
$ podman run \
    --runtime="$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    quay.io/containerdisks/fedora:39 \
    unused
```

You can also use `util/package-vm-image.sh` to easily package a VM image into a
container image, and `util/extract-vm-image.sh` to extract a VM image contained
in a container image.

## Bind mounts

Bind mounts are passed through to the VM as [virtiofs] file systems:

```console
$ podman run \
    --runtime="$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    -v ./util:/home/fedora/util \
    quay.io/containerdisks/fedora:39 \
    unused
```

If the VM image support cloud-init, the volume will automatically be mounted
inside the guest at the given path. Otherwise, you can mount it with:

```console
mount -t virtiofs /home/fedora/util /home/fedora/util
```

## cloud-init

You can provide a [cloud-init] NoCloud configuration to the VM by configuring a
bind mount with the special destination `/cloud-init`:

```console
$ ls examples/cloud-init/config/
meta-data  user-data  vendor-data

$ podman run \
    --runtime="$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    -v ./examples/cloud-init/config:/cloud-init \
    quay.io/containerdisks/fedora:39 \
    unused
```

You should now be able to login with the default `fedora` username and password
`pass`.

## How it works

Internally, the `crun-qemu` runtime uses [crun] to run a different container
that in turn uses [libvirt] to run a [QEMU] guest based on the VM image included
in the user-specified container.

## License

This project is released under the GPL 2.0 (or later) license. See
[LICENSE](LICENSE).

[cloud-init]: https://cloud-init.io/
[crun]: https://github.com/containers/crun
[KubeVirt `containerDisk`s]: https://kubevirt.io/user-guide/virtual_machines/disks_and_volumes/#containerdisk
[libvirt]: https://libvirt.org/
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
[QEMU]: https://www.qemu.org/
[virtiofs]: https://virtio-fs.gitlab.io/
