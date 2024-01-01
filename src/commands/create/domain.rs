// SPDX-License-Identifier: GPL-2.0-or-later

use std::fs::File;
use std::io::Write;

use anyhow::Result;
use sysinfo::SystemExt;
use xml::writer::XmlEvent;

use crate::commands::create::custom_opts::{CustomOptions, VfioPciMdevUuid};
use crate::commands::create::Mounts;
use crate::util::{PathExt, SpecExt, VmImageInfo};

pub fn set_up_libvirt_domain_xml(
    spec: &oci_spec::runtime::Spec,
    vm_image_info: &VmImageInfo,
    mounts: &Mounts,
    custom_options: &CustomOptions,
) -> Result<()> {
    let path = spec.root_path().join("crun-qemu/domain.xml");

    let mut w = xml::EmitterConfig::new()
        .perform_indent(true)
        .create_writer(File::create(path)?);

    s(&mut w, "domain", &[("type", "kvm")], |w| {
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
                    ("file", "/crun-qemu/first-boot/ignition.ign"),
                ],
            )
        })?;

        if !mounts.virtiofs.is_empty() {
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
                se(
                    w,
                    "driver",
                    &[("name", "qemu"), ("type", &vm_image_info.format)],
                )?;
                se(w, "source", &[("file", vm_image_info.path.as_str())])?;
                Ok(())
            })?;

            for (i, dev) in mounts.block_device.iter().enumerate() {
                let typ = if dev.is_regular_file { "file" } else { "block" };
                let source_attr = if dev.is_regular_file { "file" } else { "dev" };

                s(w, "disk", &[("type", typ), ("device", "disk")], |w| {
                    se(w, "target", &[("dev", &next_dev_name()), ("bus", "virtio")])?;
                    se(
                        w,
                        "source",
                        &[(source_attr, dev.path_in_container.as_str())],
                    )?;
                    if dev.readonly {
                        se(w, "readonly", &[])?;
                    }
                    st(w, "serial", &[], &format!("crun-qemu-block-{i}"))?;
                    Ok(())
                })?;
            }

            s(w, "disk", &[("type", "file"), ("device", "disk")], |w| {
                se(
                    w,
                    "source",
                    &[("file", "/crun-qemu/first-boot/cloud-init.iso")],
                )?;
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

            for (i, mount) in mounts.virtiofs.iter().enumerate() {
                let path = mount.path_in_container.as_str();
                let tag = format!("virtiofs-{}", i);

                s(w, "filesystem", &[("type", "mount")], |w| {
                    se(w, "driver", &[("type", "virtiofs")])?;
                    s(w, "binary", &[("path", "/crun-qemu/virtiofsd")], |w| {
                        se(w, "sandbox", &[("mode", "chroot")])
                    })?;
                    se(w, "source", &[("dir", path)])?;
                    se(w, "target", &[("dir", &tag)])?;
                    Ok(())
                })?;
            }

            for address in &custom_options.vfio_pci {
                s(
                    w,
                    "hostdev",
                    &[("mode", "subsystem"), ("type", "pci")],
                    |w| {
                        s(w, "source", &[], |w| {
                            se(
                                w,
                                "address",
                                &[
                                    ("domain", &format!("0x{:04x}", address.domain)),
                                    ("bus", &format!("0x{:02x}", address.bus)),
                                    ("slot", &format!("0x{:02x}", address.slot)),
                                    ("function", &format!("0x{:01x}", address.function)),
                                ],
                            )
                        })
                    },
                )?;
            }

            for VfioPciMdevUuid(uuid) in &custom_options.vfio_pci_mdev {
                s(
                    w,
                    "hostdev",
                    &[
                        ("mode", "subsystem"),
                        ("type", "mdev"),
                        ("model", "vfio-pci"),
                    ],
                    |w| s(w, "source", &[], |w| se(w, "address", &[("uuid", uuid)])),
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

        if quota == 0 {
            return None;
        }

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
