# The `crun-qemu` OCI runtime

This is an **experimental** [OCI Runtime] that enables [Podman] and [Docker] to
run Virtual Machine (VM) images. The objective is to make running VMs (in simple
configurations) as easy as running containers, using standard container tooling
and without the need for in-depth knowledge of virtualization technologies like
libvirt.

## Installing

### Build and install from source (on Fedora)

1. Install `crun-qemu`'s runtime dependencies:

   ```console
   $ dnf install bash coreutils crun genisoimage libselinux libvirt-client libvirt-daemon-driver-qemu libvirt-daemon-log qemu-img qemu-system-x86-core shadow-utils util-linux virtiofsd
   ```

2. Install Rust and Cargo if you do not already have Rust tooling available:

   ```console
   $ dnf install cargo
   ```

3. Build `crun-qemu`:

   ```console
   $ cargo build
   ```

4. Copy the `target/debug/crun-qemu` binary to wherever you prefer, for instance
   `/usr/local/bin/`:

   ```console
   $ cp target/debug/crun-qemu /usr/local/bin/
   ```

5. If you're using Podman:

     - Merge the following configuration into `/etc/containers/containers.conf`:

       ```toml
       [engine.runtimes]
       crun-qemu = ["/usr/local/bin/crun-qemu"]
       ```

   If you're using Docker:

     - Merge the following configuration into `/etc/docker/daemon.json`:

       ```json
       {
         "runtimes": {
           "crun-qemu": {
             "path": "/usr/local/bin/crun-qemu"
           }
         }
       }
       ```

     - In `/etc/sysconfig/docker`, replace the line:

       ```
       --default-ulimit nofile=1024:1024 \
       ```

       With:

       ```
       --default-ulimit nofile=262144:262144 \
       ```

     - Reload the `docker` service for the new configuration to take effect:

       ```console
       $ service docker reload
       ```

When using Podman, it is possible to run the examples below and elsewhere in
this repo without actually installing `crun-qemu`, *i.e.*, performing only steps
1–3. In this case, you must replace `--runtime crun-qemu` with `--runtime
"$PWD"/target/debug/crun-qemu` when running the examples.

## Overview

Here we overview some of the major features provided by `crun-qemu`.

To run the examples below using Docker instead of Podman, you must additionally
pass `--security-opt label=disable` to docker-run. Other than that, and unless
otherwise stated, you can simply replace `podman` with `docker` in the commands
below.

### Booting VMs

#### From regular VM image files

First, obtain a QEMU-compatible VM image and place it in a directory by itself:

```console
$ mkdir my-vm-image
$ curl -LO --output-dir my-vm-image https://download.fedoraproject.org/pub/fedora/linux/releases/39/Cloud/x86_64/images/Fedora-Cloud-Base-39-1.5.x86_64.qcow2
```

Then run:

> This example does not work with Docker, as docker-run does not support the
> `--rootfs` flag; see the next section for a Docker-compatible way of running
> VM images.

```console
$ podman run \
    --runtime crun-qemu \
    -it --rm \
    --rootfs my-vm-image \
    ""
```

The VM console should take over your terminal. To abort the VM, press `ctrl-]`.

You can also detach from the VM without terminating it by pressing `ctrl-p,
ctrl-q`. Afterwards, reattach by running:

> For this command to work with Docker, you must replace the `--latest` flag
> with the container's name or ID.

```console
$ podman attach --latest
```

This command also works when you start the VM in detached mode using
podman-run's `-d`/`--detach` flag.

It is also possible to omit flags `-i`/`--interactive` and `-t`/`--tty` to
podman-run, in which case you won't be able to interact with the VM but can
still observe its console. Note that pressing `ctrl-]` will have no effect, but
you can always use the following command to terminate the VM:

> For this command to work with Docker, you must omit the `--time=0` option and
> replace the `--latest` flag with the container's name or ID.

```container
$ podman rm --force --time=0 --latest
```

#### From VM image files packaged into container images

`crun-qemu` also works with container images that contain a VM image file with
any name under `/` or under `/disk/`. No other files may exist in those
directories. Containers built for use as [KubeVirt `containerDisk`s] follow this
convention, so you can use those here:

```console
$ podman run \
    --runtime crun-qemu \
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

> For this command to work with Docker, you must provide an absolute path to
> `--cloud-init`.

```console
$ ls examples/cloud-init/config/
meta-data  user-data  vendor-data

$ podman run \
    --runtime crun-qemu \
    -it --rm \
    quay.io/containerdisks/fedora:39 \
    --cloud-init examples/cloud-init/config
```

You should now be able to login with the default `fedora` username and password
`pass`.

#### Ignition

Similarly, you can provide an [Ignition] configuration to the VM by passing in
the `--ignition` option:

> For this command to work with Docker, you must provide an absolute path to
> `--ignition`.

```console
$ podman run \
    --runtime crun-qemu \
    -it --rm \
    quay.io/crun-qemu/example-fedora-coreos:39 \
    --ignition examples/ignition/config.ign
```

You should now be able to login with the default `core` username and password
`pass`.

### SSH'ing into the VM

Assuming the VM supports cloud-init or Ignition and exposes an SSH server on
port 22, you can `ssh` into it using podman-exec as the VMs default user:

> For this command to work with Docker, you must replace the `--latest` flag
> with the container's name or ID.

```console
$ podman run \
    --runtime crun-qemu \
    --detach --rm \
    quay.io/containerdisks/fedora:39 \
    ""
8068a2c180e0f4bf494f5e0baa37d9f13a9810f76b361c0771b73666e47ec383

$ podman exec --latest fedora whoami
fedora

$ podman exec -it --latest fedora
[fedora@8068a2c180e0 ~]$
```

With cloud-init, the default user can vary between VM images. With Ignition,
`core` is considered to be the default user. In both cases, if the SSH server
allows password authentication, you should also be able to login as any other
user.

The `fedora` argument to podman-exec above, which would typically correspond to
the command to be executed, determines instead the name of the user to `ssh`
into the VM as. A command can optionally be specified with further arguments. If
no command is specified, a login shell is initiated. In this case, you probably
also want to pass flags `-it` to podman-exec.

If you actually just want to exec into the container in which the VM is running
(probably to debug some problem with `crun-qemu` itself), pass in `-` as the
username.

### Port forwarding

You can use podman-run's standard `-p`/`--publish` option to set up TCP and/or
UDP port forwarding:

```console
$ podman run \
    --runtime crun-qemu \
    --detach --rm \
    -p 8000:80 \
    quay.io/crun-qemu/example-http-server:latest \
    ""
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

### Passing things through to the VM

#### Directory bind mounts

Bind mounting directories into the VM is supported:

> :warning: This example recursively modifies the SELinux context of all files
> under the path being mounted, in this case `./util`, which in the worst case
> **may cause you to lose access to your files**. This is due to the `:z` volume
> mount modifier, which instructs Podman to relabel the volume so that the VM
> can access it.
>
> Alternatively, you may remove this modifier from the command below and add
> `--security-opt label=disable` instead to disable SELinux enforcement.

> For this command to work with Docker, you must provide an absolute path to
> `--cloud-init`.

```console
$ podman run \
    --runtime crun-qemu \
    -it --rm \
    -v ./util:/home/fedora/util:z \
    quay.io/containerdisks/fedora:39 \
    --cloud-init examples/cloud-init/config
```

If the VM supports cloud-init or Ignition, the volume will automatically be
mounted at the given destination path. Otherwise, you can mount it manually with
the following command, where `<index>` must be the 0-based index of the volume
according to the order the `-v`/`--volume` or `--mount` flags where given in:

```console
$ mount -t virtiofs virtiofs-<index> /home/fedora/util
```

#### Block devices

If cloud-init or Ignition are supported by the VM, it is possible to pass block
devices through to it at a specific path using podman-run's `--device` flag
(this example assumes `/dev/ram0` to exist and to be accessible by the current
user):

> For this command to work with Docker, you must provide an absolute path to
> `--cloud-init`.

```console
$ podman run \
    --runtime crun-qemu \
    -it --rm \
    --device /dev/ram0:/home/fedora/my-disk \
    quay.io/containerdisks/fedora:39 \
    --cloud-init examples/cloud-init/config
```

You can also pass them in as bind mounts using the `-v`/`--volume` or `--mount`
flags.

#### Mediated (mdev) vfio-pci devices

Mediated vfio-pci devices (such as vGPUs) can be passed through to the VM by
specifying the non-standard `--vfio-pci-mdev` option with a path to the mdev's
sysfs directory (this example assumes that the corresponding VFIO device under
`/dev/vfio/` is accessible to the current user):

```console
$ podman run \
    --runtime crun-qemu \
    -it --rm \
    quay.io/containerdisks/fedora:39 \
    --vfio-pci-mdev /sys/bus/pci/devices/0000:00:02.0/5fa530b9-9fdf-4cde-8eb7-af73fcdeeaae
```

## How it works

Internally, `crun-qemu` uses [crun] to run an isolated [libvirt] instance that
boots a [QEMU] VM from the user-specified image.

## License

This project is released under the GPL 2.0 (or later) license. See
[LICENSE](LICENSE).

[cloud-init]: https://cloud-init.io/
[crun]: https://github.com/containers/crun
[Docker]: https://www.docker.com/
[KubeVirt `containerDisk`s]: https://kubevirt.io/user-guide/virtual_machines/disks_and_volumes/#containerdisk
[libvirt]: https://libvirt.org/
[Ignition]: https://coreos.github.io/ignition/
[Podman]: https://podman.io/
[OCI Runtime]: https://github.com/opencontainers/runtime-spec/blob/v1.1.0/spec.md
[QEMU]: https://www.qemu.org/
