# 2. Running VMs with **Podman** or **Docker**

Here, we outline how to use crun-vm to run VMs using the Podman or Docker
container engines. See [crun-vm(1)] for a full reference and additional details
on crun-vm specific options.

We use `podman` in the example commands below. Unless otherwise noted, the same
commands also with `docker`.

<details open>
  <summary><b>Navigation</b></summary>

  1. [Installing crun-vm](1-installing.md)
  2. **Running VMs with **Podman** or **Docker****
     - [**Booting VMs**](#booting-vms)
       - [From containerdisks](#from-containerdisks)
       - [From VM image files](#from-vm-image-files)
       - [From bootable containers](#from-bootable-containers)
     - [**Configuring VMs on first boot**](#configuring-vms-on-first-boot)
       - [Default user password](#default-user-password)
       - [cloud-init](#cloud-init)
       - [Ignition](#ignition)
     - [**Interacting with VMs**](#interacting-with-vms)
       - [Exec'ing into VMs](#execing-into-vms)
       - [Port forwarding](#port-forwarding)
     - [**Sharing resources with VMs**](#sharing-resources-with-vms)
       - [Files](#files)
       - [Directories](#directories)
       - [Block devices](#block-devices)
  3. [Running VMs as **systemd** services](3-systemd.md)
  4. [Running VMs in **Kubernetes**](4-kubernetes.md)
  5. [**crun-vm(1)** man page](5-crun-vm.1.ronn)

</details>

## Booting VMs

### From containerdisks

A "containerdisk" is a container image that packages a sole VM image file under
`/` or `/disk/`. This is how you would boot a VM from the
quay.io/containerdisks/fedora:40 containerdisk using crun-vm:

```console
$ podman run --runtime crun-vm -it quay.io/containerdisks/fedora:40
Booting `Fedora Linux (6.8.5-301.fc40.x86_64) 40 (Cloud Edition)'
[...]
```

> Several regularly-updated containerdisks may be found at
> https://quay.io/organization/containerdisks. You can also easily build your
> own:
>
> ```dockerfile
> FROM scratch
> COPY my-vm-image.qcow2 /
> ENTRYPOINT ["no-entrypoint"]
> ```

The VM console should take over your terminal. This VM image has no users that
you may log in as using a password, so although you can interact with the VM's
login screen, you will be unable to access a command prompt for now. To abort
the VM, press `ctrl-]`.

You can also detach from the VM without terminating it by pressing `ctrl-p,
ctrl-q`. Afterwards, reattach by running:

```console
$ podman attach --latest
```

This command also works when you start the VM in detached mode using
podman-run's `-d`/`--detach` flag.

It is also possible to omit flags `-i`/`--interactive` and `-t`/`--tty` to
podman-run, in which case you won't be able to interact with the VM but can
still observe its console. Note that pressing `ctrl-]` will have no effect, but
you can always use the following command to terminate the VM:

```container
$ podman stop --latest
```

### From VM image files

> This feature is only supported with Podman.

It is also possible to boot VMs directly from disk image files by using Podman's
`--rootfs` option to point at a directory containing a sole image file. For
instance, these commands download and boot a Fedora 40 VM:

```console
$ mkdir my-vm-image/ && curl -LO --output-dir my-vm-image/ https://download.fedoraproject.org/pub/fedora/linux/releases/40/Cloud/x86_64/images/Fedora-Cloud-Base-Generic.x86_64-40-1.14.qcow2

$ podman run --runtime crun-vm -it --rootfs my-vm-image/
Booting `Fedora Linux (6.8.5-301.fc40.x86_64) 40 (Cloud Edition)'
[...]
```

### From bootable containers

crun-vm can also launch VMs from [bootc bootable container images], which are
containers that package a full operating system:

```console
$ podman run --runtime crun-vm -it quay.io/crun-vm/example-fedora-bootc:40
Converting quay.io/crun-vm/example-fedora-bootc:40 into a VM image...
[...]
Caching VM image as a containerdisk...
[...]
Booting VM...
[...]
```

crun-vm generates a VM image from the bootable container and then boots it. The
generated VM image is packaged as a containerdisk and cached in the host's
container storage, so that subsequent runs will boot faster:

```console
$ podman run --runtime crun-vm -it quay.io/crun-vm/example-fedora-bootc:40
Retrieving cached VM image...
[...]
Booting VM...
[...]
```

## Configuring VMs on first boot

### Default user password

In the examples above, you were able to boot a VM but not to log in. An easy way
to fix this when a VM has [cloud-init] installed is to use the [`--password`]
option, which sets the password for the VM's "default" user (as determined in
the image's cloud-init configuration). For quay.io/containerdisks/fedora:40,
that is the `fedora` user:

```console
$ podman run --runtime crun-vm -it quay.io/containerdisks/fedora:40 --password pass
Booting `Fedora Linux (6.8.5-301.fc40.x86_64) 40 (Cloud Edition)'
[...]
3b0232a04046 login: fedora
Password: pass
[fedora@3b0232a04046 ~]$
```

Like all crun-vm specific options, [`--password`] must be passed in *after* the
image specification.

### cloud-init

You can provide a full [cloud-init] NoCloud configuration to a VM by passing in
the crun-vm specific option [`--cloud-init`] *after* the image specification:

```console
$ ls my-cloud-init-config/
meta-data  user-data

$ cat my-cloud-init-config/meta-data  # empty

$ cat my-cloud-init-config/user-data
#cloud-config
write_files:
  - path: $home/file
    content: |
      hello

$ podman run --runtime crun-vm -it quay.io/containerdisks/fedora:40 \
    --cloud-init $PWD/my-cloud-init-config/  # path must be absolute
```

### Ignition

You can also provide an [Ignition] configuration to a VM using the crun-vm
specific [`--ignition`] option:

```console
$ cat my-ignition-config.ign
{
  "ignition": {
    "version": "3.0.0"
  },
  "passwd": {
    "users": [
      {
        "name": "core",
        "passwordHash": "$y$j9T$USdd8CBvFNVU1xKwQUnsU/$aE.3arHcRxD0ZT3vkvsSpEsteUj6vC4ZdRHY8eOj1f4"
      }
    ]
  }
}

$ podman run --runtime crun-vm -it quay.io/crun-vm/example-fedora-coreos:40 \
    --ignition $PWD/my-ignition-config.ign  # path must be absolute
```

## Interacting with VMs

### Exec'ing into VMs

Assuming a VM supports cloud-init or Ignition and exposes an SSH server on port
22, you can `ssh` into it as root using podman-exec:

```console
$ podman run --runtime crun-vm --detach quay.io/containerdisks/fedora:40
8068a2c180e0f4bf494f5e0baa37d9f13a9810f76b361c0771b73666e47ec383

$ podman exec --latest whoami
Please login as the user "fedora" rather than the user "root".
```

This particular VM image does not allow logging in as root. To `ssh` into the VM
as a different user, specify its username using the [`--as`] option immediately
before the command (if any). You may need to pass in `--` before this option to
prevent podman-exec from trying to interpret it:

```console
$ podman exec --latest -- --as fedora whoami
fedora
```

If you just want a login shell, pass in an empty string as the command. The
following would be the output if this VM image allowed logging in as root:

```console
$ podman exec -it --latest ""
[root@8068a2c180e0 ~]$
```

You may also log in as a specific user:

```console
$ podman exec -it --latest -- --as fedora
[fedora@8068a2c180e0 ~]$
```

When a VM supports cloud-init, `authorized_keys` is automatically set up to
allow SSH access by podman-exec for users `root` and the default user as set in
the image's cloud-init configuration. With Ignition, this is set up for users
`root` and `core`.

### Port forwarding

You can use podman-run's standard `-p`/`--publish` option to enable TCP and/or
UDP port forwarding:

```console
$ podman run --runtime crun-vm --detach -p 8000:80 quay.io/crun-vm/example-http-server:latest
36c8705482589cfc4336a03d3802e7699f5fb228123d18e693488ac7b80116d1

$ curl localhost:8000
<!DOCTYPE HTML>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Directory listing for /</title>
</head>
<body>
[...]
```

## Sharing resources with VMs

### Files

You can bind mount regular files into a VM:

```console
$ podman run --runtime crun-vm -it \
    -v ./README.md:/home/fedora/README.md:z \
    quay.io/containerdisks/fedora:40
```

Regular files currently appear as block devices in the VM, but this is subject
to change.

### Directories

It is also possible to bind mount directories into a VM:

```console
$ podman run --runtime crun-vm -it \
    -v ./util:/home/fedora/util:z \
    quay.io/containerdisks/fedora:40
```

If the VM supports cloud-init or Ignition, the volume will automatically be
mounted at the given destination path. Otherwise, you can mount it manually with
the following command, where `<index>` is the 0-based index of the volume
according to the order the `-v`/`--volume` or `--mount` flags where given in:

```console
$ mount -t virtiofs virtiofs-<index> /home/fedora/util
```

### Block devices

If cloud-init or Ignition are supported by a VM, it is possible to pass block
devices through to it and make them appear at a specific path using podman-run's
`--device` flag. For instance, assuming `/dev/ram0` exists on the host and is
accessible by the current user:

```console
$ podman run --runtime crun-vm -it \
    --device /dev/ram0:/home/fedora/my-disk \
    quay.io/containerdisks/fedora:40
```

You can also use the more powerful, crun-vm specific [`--blockdev`]
`source=<path>,target=<path>,format=<fmt>` option to this effect. This option
also allows you to specify a regular file as the source, and the source may be
in any disk format known to QEMU (*e.g.*, raw, qcow2; when using `--device`, raw
format is assumed):

```console
$ podman run --runtime crun-vm -it \
    quay.io/containerdisks/fedora:40 \
    --blockdev source=$PWD/my-disk.qcow2,target=/home/fedora/my-disk,format=qcow2  # paths must be absolute
```

[`--as`]: 5-crun-vm.1.ronn#exec-options
[`--blockdev`]: 5-crun-vm.1.ronn#createrun-options
[`--cloud-init`]: 5-crun-vm.1.ronn#createrun-options
[`--ignition`]: 5-crun-vm.1.ronn#createrun-options
[`--password`]: 5-crun-vm.1.ronn#createrun-options
[bootc bootable container images]: https://containers.github.io/bootable/
[cloud-init]: https://cloud-init.io/
[crun-vm(1)]: 5-crun-vm.1.ronn
[domain XML definition]: https://libvirt.org/formatdomain.html
[Ignition]: https://coreos.github.io/ignition/
[KubeVirt `containerDisk`s]: https://kubevirt.io/user-guide/virtual_machines/disks_and_volumes/#containerdisk
[libvirt]: https://libvirt.org/
