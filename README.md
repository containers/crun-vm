# The crun-vm OCI Runtime

**crun-vm** is an [OCI Runtime] that enables [Podman], [Docker], and
[Kubernetes] to run QEMU-compatible Virtual Machine (VM) images. This means you
can:

  - Run **VMs** as easily as you run **containers**.
  - Manage containers and VMs **together** using the **same** standard tooling.

---

<p align="center">
  <img src="docs/example.gif" width="680" />
</p>

---

<table>
<tr>
<td width="450" valign="top">

### Quick start

Install crun-vm:

```console
$ dnf install crun-vm
```

Launch a VM from a disk image packaged in a container:

```console
$ podman run --runtime crun-vm -it \
    quay.io/containerdisks/fedora:40
```

Launch a VM from a disk image under `my-image-dir/`:

```console
$ podman run --runtime crun-vm -it \
    --rootfs my-image-dir/
```

Launch a VM from a [bootable container]:

```console
$ podman run --runtime crun-vm -it \
    quay.io/crun-vm/example-fedora-bootc:40
```

Set the password for a VM's default user:

```console
$ podman run --runtime crun-vm -it \
    quay.io/containerdisks/fedora:40 \
    --password pass  # for user "fedora"
```

Exec (ssh) into a VM:

```console
$ podman exec -it --latest -- --as fedora
```

<p></p>
</td>
<td valign="top">

### Major features

  - Control VM **CPU** and **memory** allocation.
  - Pass **cloud-init** or **Ignition** configs to VMs.
  - Mount **directories** into VMs.
  - Pass **block devices** through to VMs.
  - Expose additional **disk images** to VMs.
  - **Forward ports** from the host to VMs.
  - **`podman|docker|kubectl exec`** into VMs.

---

### Documentation

  1. [Installing crun-vm](docs/1-installing.md)
  2. [Running VMs with **Podman** or **Docker**](docs/2-podman-docker.md)
  3. [Running VMs as **systemd** services](docs/3-systemd.md)
  4. [Running VMs in **Kubernetes**](docs/4-kubernetes.md)
  5. [**crun-vm(1)** man page](docs/5-crun-vm.1.ronn)

---

### License

This project is released under the GPL 2.0 (or later) license. See
[LICENSE](LICENSE).

<p></p>
</td>
</tr>
</table>

[bootable container]: https://containers.github.io/bootable
[Docker]: https://www.docker.com/
[Kubernetes]: https://kubernetes.io/
[KubeVirt]: https://kubevirt.io/
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
[Podman]: https://podman.io/
[systemd]: https://systemd.io/
