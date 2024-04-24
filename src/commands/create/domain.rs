// SPDX-License-Identifier: GPL-2.0-or-later

use std::fs::File;
use std::io::{BufReader, Write};

use anyhow::{ensure, Result};
use camino::Utf8Path;
use xml::writer::XmlEvent;

use crate::commands::create::custom_opts::CustomOptions;
use crate::commands::create::Mounts;
use crate::util::{SpecExt, VmImageInfo};

pub fn set_up_libvirt_domain_xml(
    spec: &oci_spec::runtime::Spec,
    vm_image_info: &VmImageInfo,
    mounts: &Mounts,
    custom_options: &CustomOptions,
) -> Result<()> {
    let path = spec.root_path()?.join("crun-vm/domain.xml");

    generate(&path, spec, vm_image_info, mounts, custom_options)?;
    merge_overlays(&path, &custom_options.merge_libvirt_xml)?;

    Ok(())
}

fn generate(
    path: impl AsRef<Utf8Path>,
    spec: &oci_spec::runtime::Spec,
    vm_image_info: &VmImageInfo,
    mounts: &Mounts,
    custom_options: &CustomOptions,
) -> Result<()> {
    let mut w = xml::EmitterConfig::new()
        .perform_indent(true)
        .create_writer(File::create(path.as_ref())?);

    let domain_type = match custom_options.emulated {
        true => "qemu",
        false => "kvm",
    };

    s(&mut w, "domain", &[("type", domain_type)], |w| {
        st(w, "name", &[], "domain")?;

        let cpu_mode = match custom_options.emulated {
            true => "host-model",
            false => "host-passthrough",
        };
        se(w, "cpu", &[("mode", cpu_mode)])?;

        let vcpus = get_vcpu_count(spec).to_string();
        if let Some(cpu_set) = get_cpu_set(spec) {
            st(w, "vcpu", &[("cpuset", cpu_set.as_str())], vcpus.as_str())?;
        } else {
            st(w, "vcpu", &[], vcpus.as_str())?;
        }

        let memory = get_memory_size(spec).to_string();
        st(w, "memory", &[("unit", "b")], memory.as_str())?;

        s(w, "os", &[], |w| {
            st(w, "type", &[("machine", "q35")], "hvm")
        })?;

        // fw_cfg requires ACPI
        s(w, "features", &[], |w| se(w, "acpi", &[]))?;

        s(w, "sysinfo", &[("type", "fwcfg")], |w| {
            se(
                w,
                "entry",
                &[
                    ("name", "opt/com.coreos/config"),
                    ("file", "/crun-vm/first-boot/ignition.ign"),
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
                    se(w, "driver", &[("name", "qemu"), ("type", &dev.format)])?;
                    se(
                        w,
                        "source",
                        &[(source_attr, dev.path_in_container.as_str())],
                    )?;
                    if dev.readonly {
                        se(w, "readonly", &[])?;
                    }
                    st(w, "serial", &[], &format!("crun-vm-block-{i}"))?;
                    Ok(())
                })?;
            }

            s(w, "disk", &[("type", "file"), ("device", "disk")], |w| {
                se(
                    w,
                    "source",
                    &[("file", "/crun-vm/first-boot/cloud-init.iso")],
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
                    s(
                        w,
                        "binary",
                        &[("path", "/crun-vm/virtiofsd.sh"), ("xattr", "on")],
                        |w| se(w, "sandbox", &[("mode", "chroot")]),
                    )?;
                    se(w, "source", &[("dir", path)])?;
                    se(w, "target", &[("dir", &tag)])?;
                    Ok(())
                })?;
            }

            Ok(())
        })?;

        Ok(())
    })?;

    w.inner_mut().flush()?;

    Ok(())
}

fn merge_overlays(
    base_path: impl AsRef<Utf8Path>,
    overlay_paths: &[impl AsRef<Utf8Path>],
) -> Result<()> {
    fn load(path: impl AsRef<Utf8Path>) -> Result<minidom::Element> {
        let reader = BufReader::new(File::open(path.as_ref())?);
        Ok(minidom::Element::from_reader_with_prefixes(
            reader,
            "".to_string(),
        )?)
    }

    fn save(path: impl AsRef<Utf8Path>, root: &minidom::Element) -> Result<()> {
        let mut writer = File::create(path.as_ref())?;
        root.write_to_decl(&mut writer)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(())
    }

    #[must_use]
    fn merge(base: &minidom::Element, overlay: &minidom::Element) -> minidom::Element {
        let mut builder = minidom::Element::builder(base.name(), base.ns());

        for (name, val) in base.attrs().chain(overlay.attrs()) {
            builder = builder.attr(name, val);
        }

        let overlay_has_text = !overlay.text().trim().is_empty();

        for base_node in base.nodes() {
            match base_node {
                minidom::Node::Element(base_child) => {
                    let new_child =
                        match overlay.get_child(base_child.name(), base_child.ns().as_str()) {
                            Some(overlay_child) => merge(base_child, overlay_child),
                            None => base_child.clone(),
                        };

                    builder = builder.append(new_child);
                }
                minidom::Node::Text(base_text) => {
                    if !overlay_has_text {
                        builder = builder.append(base_text.as_str());
                    }
                }
            };
        }

        for overlay_node in overlay.nodes() {
            match overlay_node {
                minidom::Node::Element(overlay_child) => {
                    if !base.has_child(overlay_child.name(), overlay_child.ns().as_str()) {
                        builder = builder.append(overlay_child.clone());
                    }
                }
                minidom::Node::Text(overlay_text) => {
                    if overlay_has_text {
                        builder = builder.append(overlay_text.as_str());
                    }
                }
            }
        }

        builder.build()
    }

    let mut base_root = load(&base_path)?;

    for overlay_path in overlay_paths {
        let overlay_root = load(overlay_path)?;
        ensure!(
            overlay_root.name() == "domain" && overlay_root.ns() == "",
            "libvirt XML root node must be named 'domain' and have no namespace"
        );
        base_root = merge(&base_root, &overlay_root);
    }

    save(&base_path, &base_root)
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

    memory_size.unwrap_or_else(|| 2u64.pow(31)) // default to 2 GiB
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
