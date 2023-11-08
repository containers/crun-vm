# The `crun-qemu` OCI runtime

This is an **experimental** [OCI Runtime] implementation that enables `podman
run` (and maybe also `docker run`) to work with VM images packaged in container
images. The objective is to make running VMs (in simple configurations) as easy
as running containers, while leveraging the existing container image
distribution infrastructure.

The runtime expects container images to contain a VM image file under a `/disk`
directory. The image file can have any name and must be in *raw* format. No
other files may exist under `/disk`. (This is the convention followed by
KubeVirt `containerDisk`s, so you can use those containers with this runtime.)

Eventually we could make the runtime work with VM images distributed as [OCI
Artifacts]. This would make the work of packaging the VM image into a container
unnecessary.

## Trying it out

```console
$ cargo build
$ podman run --security-opt label=disable --rm -it --runtime="$PWD"/target/debug/crun-qemu quay.io/kubevirt/alpine-container-disk-demo unused
```

This command is handy for development:

```console
$ cargo build && RUST_BACKTRACE=1 podman run --log-level=debug --security-opt label=disable --rm -it --runtime="$PWD"/target/debug/crun-qemu quay.io/kubevirt/alpine-container-disk-demo unused
```

## How it works

Internally, the `crun-qemu` runtime uses [crun] to run a different container
that in turn uses [libvirt] to run a [QEMU] guest based on the VM image included
in the user-specified container.

## License

This project is released under the GPL 3.0 license. See [LICENSE](LICENSE).

[crun]: https://github.com/containers/crun
[libvirt]: https://libvirt.org/
[OCI Artifacts]: https://github.com/opencontainers/image-spec/blob/v1.1.0-rc5/artifacts-guidance.md
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
[QEMU]: https://www.qemu.org/
