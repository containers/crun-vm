# Example: Manage VMs with systemd using Podman Quadlet and crun-vm

[Podman Quadlet] is a feature that allows you to define systemd unit files for
containers, such that they can be managed like any other systemd service.

crun-vm is fully compatible with Podman Quadlet, meaning that you can also
launch and manage your VMs through systemd. Here's an example of how to do so:

1. Create file `$HOME/.config/containers/systemd/my-vm.container` with the
   following contents:

   ```toml
   [Unit]
   Description=My VM
   After=local-fs.target

   [Container]
   PodmanArgs=--runtime crun-vm            # make Podman use crun-vm as the runtime
   Image=quay.io/containerdisks/fedora:40  # the container image containing our VM image
   Exec=--password pass                    # optional crun-vm arguments

   [Install]
   WantedBy=multi-user.target default.target  # start on boot by default
   ```

   The options under `[Container]` in this unit file will effectively translate
   into the following podman invocation:

   ```console
   $ podman run --runtime crun-vm quay.io/containerdisks/fedora:40 --password pass
   ```

2. Inform systemd of the new unit file:

   ```console
   $ systemctl --user daemon-reload
   ```

   This will creates a my-vm.service file based on my-vm.container above.

3. Manage my-vm.service as you would any other service. For instance, we can
   start it up:

   ```console
   $ systemctl --user start my-vm.service
   ```

   And then check its status:

   ```console
   $ systemctl --user status my-vm.service
   ```

See [this article] for additional information on using Podman Quadlet, and
[podman-systemd.unit(5)] for the reference format for Quadlet systemd units.

[Podman Quadlet]: https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html
[podman-systemd.unit(5)]: https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html
[this article]: https://www.redhat.com/sysadmin/quadlet-podman
