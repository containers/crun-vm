# 4. Internals

Internally, crun-vm uses [crun] to run an isolated [libvirt] instance that
boots a [QEMU] VM from the user-specified image.

[crun]: https://github.com/containers/crun
[libvirt]: https://libvirt.org/
[QEMU]: https://www.qemu.org/
