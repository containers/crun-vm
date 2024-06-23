# 1. Installing crun-vm

There are two steps to setting up crun-vm on a system:

  - Installing the actual `crun-vm` binary;
  - Configuring Podman, Docker, and/or Kubernetes (whichever you intend to use
    crun-vm with) to recognize crun-vm as a runtime.

These steps are detailed in the sections below.

<details open>
  <summary><b>Navigation</b></summary>

  1. **Installing crun-vm**
     - [**Installing the `crun-vm` binary**](#installing-the-crun-vm-binary)
       - [On Fedora](#on-fedora)
       - [From source](#from-source)
     - [**Making crun-vm available as a runtime**](#making-crun-vm-available-as-a-runtime)
       - [To Podman](#to-podman)
       - [To Docker](#to-docker)
       - [To Kubernetes](#to-kubernetes)
  2. [Running VMs with **Podman** or **Docker**](2-podman-docker.md)
  3. [Running VMs as **systemd** services](3-systemd.md)
  4. [Running VMs in **Kubernetes**](4-kubernetes.md)
  5. [**crun-vm(1)** man page](5-crun-vm.1.ronn)

</details>

## Installing the `crun-vm` binary

### On Fedora

```console
$ dnf install crun-vm
```

### From source

1. Install crun-vm's build dependencies:

   - [cargo](https://doc.rust-lang.org/stable/cargo/getting-started/installation.html)
   - [gzip](https://www.gzip.org/)
   - [libselinux](https://github.com/SELinuxProject/selinux/tree/main/libselinux),
     including development headers
   - [ronn-ng](https://github.com/apjanke/ronn-ng)

2. Install crun-vm's runtime dependencies:

   - bash
   - [coreutils](https://www.gnu.org/software/coreutils/)
   - [crun](https://github.com/containers/crun)
   - [crun-krun](https://github.com/containers/crun/blob/main/krun.1.md)
   - [genisoimage](https://github.com/Distrotech/cdrkit)
   - grep
   - [libselinux](https://github.com/SELinuxProject/selinux/tree/main/libselinux)
   - [libvirtd](https://gitlab.com/libvirt/libvirt) or
     [virtqemud](https://gitlab.com/libvirt/libvirt)
   - [passt](https://passt.top/)
   - [qemu-img](https://gitlab.com/qemu-project/qemu)
   - qemu-system-x86_64-core, qemu-system-aarch64-core, and/or other [QEMU
     system emulators](https://gitlab.com/qemu-project/qemu) for the VM
     architectures you want to support
   - ssh
   - [util-linux](https://github.com/util-linux/util-linux)
   - [virsh](https://gitlab.com/libvirt/libvirt)
   - [virtiofsd](https://gitlab.com/virtio-fs/virtiofsd)
   - [virtlogd](https://gitlab.com/libvirt/libvirt)

3. Install crun-vm's binary and man page:

   ```console
   $ make install
   ```

## Making crun-vm available as a runtime

### To Podman

Nothing to do here, since Podman automatically recognizes crun-vm as a runtime.
Commands like `podman create` and `podman run` can be made to use the crun-vm
runtime by passing them the `--runtime crun-vm` option.

<!--
Paths search by Podman:
  - `/usr/bin/crun-vm`
  - `/usr/local/bin/crun-vm`
  - `/usr/local/sbin/crun-vm`
  - `/sbin/crun-vm`
  - `/bin/crun-vm`
  - `/run/current-system/sw/bin/crun-vm`
 -->

See [2. Using crun-vm and **Podman** or **Docker** to run a
VM](2-podman-docker.md) to get started.

### To Docker

1. Merge the following configuration into `/etc/docker/daemon.json` (creating
   that directory and file if necessary):

   ```json
   {
     "runtimes": {
       "crun-vm": {
         "path": "/usr/bin/crun-vm"
       }
     }
   }
   ```

2. Reload the `docker` service for the new configuration to take effect:

   ```console
   $ service docker reload
   ```

Commands like `docker create` and `docker run` can then be made to use the
crun-vm runtime by passing them the `--runtime crun-vm` option.

See [2. Using crun-vm and **Podman** or **Docker** to run a
VM](2-podman-docker.md) to get started.

### To Kubernetes

For crun-vm to be usable as a runtime in a Kubernetes cluster, the latter must
be use the [CRI-O] runtime. See the Kubernetes docs on [runtimes] for more
information.

1. Install crun-vm on all cluster nodes where pods may be scheduled, using any
   of the methods [described above](#installing-the-crun-vm-binary).

2. Append the following to `/etc/crio/crio.conf`:

   ```toml
   [crio.runtime.runtimes.crun-vm]
   runtime_path = "/usr/bin/crun-vm"
   ```

3. Create a `RuntimeClass` object in the cluster that references crun-vm:

   ```yaml
   apiVersion: node.k8s.io/v1
   kind: RuntimeClass
   metadata:
     name: crun-vm  # a name of your choice
   handler: crun-vm
   ```

Pods can then be configured to use this `RuntimeClass` by specifying its name
under `Pod.spec.runtimeClassName`.

See [4. Using crun-vm and **Pod YAML** to run a VM with **Podman**, **systemd**,
or **Kubernetes**](4-pod-yaml.md) to get started.

[runtimes]: https://kubernetes.io/docs/setup/production-environment/container-runtimes/#cri-o
[CRI-O]: https://cri-o.io/
