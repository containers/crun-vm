# 1. Installing crun-vm

## Build and install from source (on Fedora)

1. Install crun-vm's runtime dependencies:

   ```console
   $ dnf install bash coreutils crun genisoimage libselinux-devel libvirt-client libvirt-daemon-driver-qemu libvirt-daemon-log qemu-img qemu-system-x86-core shadow-utils util-linux virtiofsd
   ```

2. Install Rust and Cargo if you do not already have Rust tooling available:

   ```console
   $ dnf install cargo
   ```

3. Build crun-vm:

   ```console
   $ cargo build
   ```

4. Copy the `target/debug/crun-vm` binary to wherever you prefer:

   ```console
   $ cp target/debug/crun-vm /usr/local/bin/
   ```

5. If you're using Podman:

     - Merge the following configuration into
       `/etc/containers/containers.conf`:

       > For rootless Podman, you can instead use
       > `${XDG_CONFIG_PATH}/containers/containers.conf`, where
       > `$XDG_CONFIG_PATH` defaults to `${HOME}/.config`.

       ```toml
       [engine.runtimes]
       crun-vm = ["/usr/local/bin/crun-vm"]
       ```

   If you're using Docker:

     - Merge the following configuration into `/etc/docker/daemon.json`:

       ```json
       {
         "runtimes": {
           "crun-vm": {
             "path": "/usr/local/bin/crun-vm"
           }
         }
       }
       ```

     - Reload the `docker` service for the new configuration to take effect:

       ```console
       $ service docker reload
       ```

With Podman, it is possible to use crun-vm without installing it, *i.e.*,
performing only steps 1–3 above. In this case, instead of setting the runtime
with `--runtime crun-vm`, specify an absolute path to the runtime binary:
`--runtime "$PWD"/target/debug/crun-vm`.
