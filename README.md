# The `crun-qemu` OCI runtime

This is an **experimental** [OCI Runtime] implementation that enables `podman
run` to work with VM images packaged in container images. The objective is to
make running VMs (in simple configurations) as easy as running containers, while
leveraging the existing container image distribution infrastructure.

The runtime expects container images to contain a VM image file with any name
under a `/disk` directory. No other files may exist under `/disk`. (This is the
convention followed by KubeVirt `containerDisk`s, so you can use those
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

The VM console should take over your terminal. To abort the VM, press `ctrl-a,
x`.

You can also detach from the VM without terminating it by pressing `ctrl-p,
ctrl-q`. Afterwards, reattach by running:

```console
$ podman attach --latest
```

## How it works

Internally, the `crun-qemu` runtime uses [crun] to run a different container
that in turn uses [libvirt] to run a [QEMU] guest based on the VM image included
in the user-specified container.

## Development

This command is handy for development:

```console
$ cargo build && RUST_BACKTRACE=1 podman run --log-level=debug --security-opt label=disable --rm -it --runtime="$PWD"/target/debug/crun-qemu quay.io/containerdisks/fedora:39 unused
```

## License

This project is released under the GPL 3.0 license. See [LICENSE](LICENSE).

[crun]: https://github.com/containers/crun
[libvirt]: https://libvirt.org/
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
[QEMU]: https://www.qemu.org/
