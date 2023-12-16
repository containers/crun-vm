# The `crun-qemu` OCI runtime

This is an **experimental** [OCI Runtime] implementation that enables `podman
run` to work with VM images packaged in container images. The objective is to
make running VMs (in simple configurations) as easy as running containers, while
leveraging the existing container image distribution infrastructure.

The runtime expects container images to contain a VM image file with any name
under `/` or under `/disk/`. No other files may exist in those directories.
(This convention is followed by KubeVirt `containerDisk`s, so you can use those
containers with this runtime.)

## Trying it out

First build the runtime:

```console
$ cargo build
```

Then try it out with an example image:

```console
$ podman run \
    --runtime="$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    quay.io/containerdisks/fedora:39 \
    unused
```

The VM console should take over your terminal. To abort the VM, press `ctrl-]`.

You can also detach from the VM without terminating it by pressing `ctrl-p,
ctrl-q`. Afterwards, reattach by running:

```console
$ podman attach --latest
```

## Using your own VM images

Use the `package-vm-image.sh` script to package a VM image file into a container
image:

```console
$ ./package-vm-image.sh
Usage: ./package-vm-image.sh <vm_image_file> <container_image_tag>
Package a given VM image file into a container image and tag it.

$ ./package-vm-image.sh my-vm-image.qcow2 my-vm-container-image:v1
STEP 1/2: FROM scratch
STEP 2/2: COPY 'my-vm-image.qcow2' /image
COMMIT my-vm-container-image:v1
--> 0b6a775fdc37
Successfully tagged localhost/my-vm-container-image:v1
0b6a775fdc377c0ec65fb67b54c524c707718f50193fa513a2e309aa08424635
```

## Bind mounts

Bind mounts are passed through to the VM as [virtiofs] file systems:

```console
$ podman run \
    --runtime="$PWD"/target/debug/crun-qemu \
    --security-opt label=disable \
    -it --rm \
    -v ./util:/my-tag \
    quay.io/containerdisks/fedora:39 \
    unused
```

What would normally be the destination path becomes the virtiofs tag (podman
requires it to begin with a `/`). For the example above, you would then run
`mount -t virtiofs /my-tag my-mount-point` in the VM to mount the virtiofs file
system.

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
[libvirt]: https://libvirt.org/
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
[QEMU]: https://www.qemu.org/
[virtiofs]: https://virtio-fs.gitlab.io/
