# 1. Installing `crun-qemu`

## Build and install from source (on Fedora)

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

4. Copy the `target/debug/crun-qemu` binary to wherever you prefer:

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

With Podman, it is possible to use `crun-qemu` without installing it, *i.e.*,
performing only steps 1–3 above. In this case, instead of setting the runtime
with `--runtime crun-qemu`, specify an absolute path to the runtime binary:
`--runtime "$PWD"/target/debug/crun-qemu`.
