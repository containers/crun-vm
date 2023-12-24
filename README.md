# The `crun-qemu` runtime

This is an **experimental** [OCI Runtime] that enables [Podman], [Docker], and
[Kubernetes] to run Virtual Machine (VM) images. The objective is to make
running VMs (in simple configurations) as easy as running containers, using
standard container tooling and without the need for in-depth knowledge of
virtualization technologies like libvirt.

### Major features

  - Works as a runtime for **rootless Podman**, **rootful Podman**, **Docker**,
    and **Kubernetes**.
  - Launching VMs with **`podman run`**/**`docker run`** from VM image **files**
    or VM image files packaged into **container images**.
  - Controlling VM resource limits with `--cpus`, `--cpuset-cpus`, `--memory`,
    etc.
  - Passing **cloud-init** and **Ignition** configurations to VMs.
  - **Mounting directories** into VMs with `-v`/`--volume` or `--mount`.
  - Passing **block devices** through to VMs with `--device`.
  - Passing **vfio-pci** and **mediated vfio-pci** devices through to VMs.
  - **Forwarding ports** from the host to VMs with `-p`/`--publish`.
  - **`podman exec`**/**`docker exec`'ing** into VMs.

### Documentation

  1. [Installing `crun-qemu`](docs/1-installing.md)
  2. [Using `crun-qemu` as a Podman or Docker runtime](docs/2-podman-docker.md)
  3. [Using `crun-qemu` as a Kubernetes runtime](docs/3-kubernetes.md)
  4. [Internals](docs/4-internals.md)

### License

This project is released under the GPL 2.0 (or later) license. See
[LICENSE](LICENSE).

[Docker]: https://www.docker.com/
[Kubernetes]: https://kubernetes.io/
[Podman]: https://podman.io/
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
