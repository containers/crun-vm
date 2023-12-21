// SPDX-License-Identifier: GPL-2.0-or-later

use std::error::Error;
use std::fs::File;
use std::io::{self, Write};

use sysinfo::SystemExt;
use xml::writer::XmlEvent;

use crate::commands::create::{BlockDevice, CustomOptions, VirtiofsMount};
use crate::util::{SpecExt, VmImageInfo};

pub fn set_up_libvirt_domain_xml(
    spec: &oci_spec::runtime::Spec,
    base_vm_image_info: &VmImageInfo,
    block_devices: &[BlockDevice],
    virtiofs_mounts: &[VirtiofsMount],
    custom_options: &CustomOptions,
) -> Result<(), Box<dyn Error>> {
    let path = spec.root_path().join("crun-qemu/domain.xml");

    let w = &mut xml::EmitterConfig::new()
        .perform_indent(true)
        .create_writer(File::create(path)?);

    s(w, "domain", &[("type", "kvm")], |w| {
        st(w, "name", &[], "domain")?;

        se(w, "cpu", &[("mode", "host-model")])?;
        let vcpus = get_vcpu_count(spec).to_string();
        if let Some(cpu_set) = get_cpu_set(spec) {
            st(w, "vcpu", &[("cpuset", cpu_set.as_str())], vcpus.as_str())?;
        } else {
            st(w, "vcpu", &[], vcpus.as_str())?;
        }

        let memory = get_memory_size(spec).to_string();
        st(w, "memory", &[("unit", "b")], memory.as_str())?;

        s(w, "os", &[], |w| {
            st(w, "type", &[("arch", "x86_64"), ("machine", "q35")], "hvm")
        })?;

        // fw_cfg requires ACPI
        s(w, "features", &[], |w| se(w, "acpi", &[]))?;

        s(w, "sysinfo", &[("type", "fwcfg")], |w| {
            se(
                w,
                "entry",
                &[
                    ("name", "opt/com.coreos/config"),
                    ("file", "/crun-qemu/ignition.ign"),
                ],
            )
        })?;

        if !virtiofs_mounts.is_empty() {
            s(w, "memoryBacking", &[], |w| {
                se(w, "source", &[("type", "memfd")])?;
                se(w, "access", &[("mode", "shared")])?;
                Ok(())
            })?;
        }

        s(w, "devices", &[], |w| {
            st(w, "emulator", &[], "/usr/bin/qemu-system-x86_64")?;

            s(w, "serial", &[("type", "pty")], |w| {
                se(w, "target", &[("port", "0")])
            })?;
            s(w, "console", &[("type", "pty")], |w| {
                se(w, "target", &[("type", "serial"), ("port", "0")])
            })?;

            let mut next_dev_index = 0;
            let mut next_dev_name = || {
                let i = next_dev_index;
                next_dev_index += 1;
                format!("vd{}", ('a'..='z').cycle().nth(i).unwrap())
            };

            s(w, "disk", &[("type", "file"), ("device", "disk")], |w| {
                se(w, "target", &[("dev", &next_dev_name()), ("bus", "virtio")])?;
                se(w, "driver", &[("name", "qemu"), ("type", "qcow2")])?;
                se(w, "source", &[("file", "/crun-qemu/image-overlay.qcow2")])?;
                s(w, "backingStore", &[("type", "file")], |w| {
                    se(w, "format", &[("type", &base_vm_image_info.format)])?;
                    se(
                        w,
                        "source",
                        &[("file", base_vm_image_info.path.to_str().unwrap())],
                    )?;
                    se(w, "backingStore", &[])?;
                    Ok(())
                })?;
                Ok(())
            })?;

            for (i, dev) in block_devices.iter().enumerate() {
                s(w, "disk", &[("type", "block"), ("device", "disk")], |w| {
                    se(w, "target", &[("dev", &next_dev_name()), ("bus", "virtio")])?;
                    se(
                        w,
                        "source",
                        &[("dev", dev.path_in_container.to_str().unwrap())],
                    )?;
                    st(w, "serial", &[], &format!("crun-qemu-bdev-{i}"))?;
                    Ok(())
                })?;
            }

            s(w, "disk", &[("type", "file"), ("device", "disk")], |w| {
                se(
                    w,
                    "source",
                    &[("file", "/crun-qemu/cloud-init/cloud-init.iso")],
                )?;
                se(w, "driver", &[("name", "qemu"), ("type", "raw")])?;
                se(w, "target", &[("dev", &next_dev_name()), ("bus", "virtio")])?;
                Ok(())
            })?;

            s(w, "interface", &[("type", "user")], |w| {
                se(w, "backend", &[("type", "passt")])?;
                se(w, "model", &[("type", "virtio")])?;
                se(w, "portForward", &[("proto", "tcp")])?;
                se(w, "portForward", &[("proto", "udp")])?;
                Ok(())
            })?;

            for mount in virtiofs_mounts {
                s(w, "filesystem", &[("type", "mount")], |w| {
                    se(w, "driver", &[("type", "virtiofs")])?;
                    s(w, "binary", &[("path", "/crun-qemu/virtiofsd")], |w| {
                        se(w, "sandbox", &[("mode", "chroot")])
                    })?;
                    se(
                        w,
                        "source",
                        &[("dir", mount.path_in_container.to_str().unwrap())],
                    )?;
                    se(
                        w,
                        "target",
                        &[("dir", mount.path_in_guest.to_str().unwrap())],
                    )?;
                    Ok(())
                })?;
            }

            // TODO: Check if these are reasonably paths to the sysfs dir for a PCI mdev device.
            // TODO: Avoid all the unwrap()s.
            let vfio_pci_mdev_uuids: Vec<_> = custom_options
                .vfio_pci_mdev
                .iter()
                .map(|path| {
                    Ok(path
                        .canonicalize()?
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string())
                })
                .collect::<io::Result<_>>()?;

            for uuid in vfio_pci_mdev_uuids {
                s(
                    w,
                    "hostdev",
                    &[
                        ("mode", "subsystem"),
                        ("type", "mdev"),
                        ("model", "vfio-pci"),
                    ],
                    |w| {
                        s(w, "source", &[], |w| {
                            se(w, "address", &[("uuid", uuid.as_ref())])
                        })
                    },
                )?;
            }

            Ok(())
        })?;

        Ok(())
    })?;

    w.inner_mut().write_all(&[b'\n'])?;

    Ok(())
}

// section
fn s(
    w: &mut xml::EventWriter<File>,
    name: &str,
    attrs: &[(&str, &str)],
    f: impl FnOnce(&mut xml::EventWriter<File>) -> xml::writer::Result<()>,
) -> xml::writer::Result<()> {
    let mut start = XmlEvent::start_element(name);
    for (key, value) in attrs {
        start = start.attr(*key, value);
    }

    w.write(start)?;
    f(w)?;
    w.write(XmlEvent::end_element())?;

    Ok(())
}

// section w/ text value
fn st(
    w: &mut xml::EventWriter<File>,
    name: &str,
    attrs: &[(&str, &str)],
    value: &str,
) -> xml::writer::Result<()> {
    s(w, name, attrs, |w| w.write(XmlEvent::Characters(value)))
}

// empty section
fn se(
    w: &mut xml::EventWriter<File>,
    name: &str,
    attrs: &[(&str, &str)],
) -> xml::writer::Result<()> {
    s(w, name, attrs, |_w| Ok(()))
}

fn get_vcpu_count(spec: &oci_spec::runtime::Spec) -> u64 {
    let vcpu_count = (|| {
        let linux_cpu = spec
            .linux()
            .as_ref()?
            .resources()
            .as_ref()?
            .cpu()
            .as_ref()?;

        let quota: u64 = linux_cpu.quota()?.try_into().ok()?;
        let period: u64 = linux_cpu.period()?;

        // return "quota / period" rounded up
        quota
            .checked_add(period)?
            .checked_sub(1)?
            .checked_div(period)
    })();

    vcpu_count.unwrap_or_else(|| num_cpus::get().try_into().unwrap())
}

fn get_memory_size(spec: &oci_spec::runtime::Spec) -> u64 {
    let memory_size: Option<u64> = (|| {
        spec.linux()
            .as_ref()?
            .resources()
            .as_ref()?
            .memory()
            .as_ref()?
            .limit()?
            .try_into()
            .ok()
    })();

    memory_size.unwrap_or_else(|| {
        let mut system =
            sysinfo::System::new_with_specifics(sysinfo::RefreshKind::new().with_memory());
        system.refresh_memory();
        system.total_memory()
    })
}

fn get_cpu_set(spec: &oci_spec::runtime::Spec) -> Option<String> {
    spec.linux()
        .as_ref()?
        .resources()
        .as_ref()?
        .cpu()
        .as_ref()?
        .cpus()
        .clone()
}
