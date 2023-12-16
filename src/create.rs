// SPDX-License-Identifier: GPL-2.0-or-later

use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use xml::writer::XmlEvent;

use crate::util::{
    create_overlay_image, crun, extract_runner_root_into, find_single_file_in_directories,
    get_image_format,
};

pub fn create(
    global_args: &liboci_cli::GlobalOpts,
    args: &liboci_cli::Create,
) -> Result<(), Box<dyn Error>> {
    let config_path = args
        .bundle
        .join("config.json")
        .to_str()
        .expect("path is utf-8")
        .to_string();

    let mut spec = libocispec::runtime::Spec::load(&config_path)?;

    // find VM image

    let root = spec
        .root
        .as_ref()
        .expect("config.json includes configuration for the container's root filesystem");

    let vm_image_path = find_single_file_in_directories([
        args.bundle.join(&root.path),
        args.bundle.join(&root.path).join("disk"),
    ])?;
    let vm_image_format = get_image_format(&vm_image_path)?;

    // prepare root filesystem for runner container

    let runner_root_path = args.bundle.join("crun-qemu-runner-root");
    extract_runner_root_into(&runner_root_path)?;

    // create overlay image

    let overlay_image_path = runner_root_path.join("vm/image-overlay.qcow2");
    create_overlay_image(overlay_image_path, &vm_image_path)?;

    // adjust config for runner container

    spec.root = Some(libocispec::runtime::Root {
        path: runner_root_path
            .to_str()
            .expect("path is utf-8")
            .to_string(),
        readonly: None,
    });

    let process = spec.process.as_mut().expect("process config");
    process.command_line = None;
    process.args = Some(vec!["/vm/entrypoint.sh".to_string()]);

    let linux = spec.linux.as_mut().expect("linux config");
    let devices = linux.devices.get_or_insert_with(Vec::new);

    let kvm_major_minor = fs::metadata("/dev/kvm")?.rdev();
    devices.push(libocispec::runtime::LinuxDevice {
        file_mode: None,
        gid: None,
        major: Some((kvm_major_minor >> 8).try_into().unwrap()),
        minor: Some((kvm_major_minor & 0xff).try_into().unwrap()),
        path: "/dev/kvm".to_string(),
        device_type: "char".to_string(),
        uid: None,
    });

    let mounts = spec.mounts.get_or_insert_with(Vec::new);

    let ignore_mounts = [
        "/cloud-init",
        "/dev",
        "/etc/hostname",
        "/etc/hosts",
        "/etc/resolv.conf",
        "/proc",
        "/run/.containerenv",
        "/run/secrets",
        "/sys",
        "/sys/fs/cgroup",
    ];

    let virtiofs_mounts: Vec<_> = mounts
        .iter_mut()
        .filter(|m| {
            m.mount_type == Some("bind".to_string())
                && !m.destination.starts_with("/dev/")
                && !ignore_mounts.contains(&m.destination.as_str())
        })
        .enumerate()
        .map(|(i, m)| {
            let virtiofs_tag_in_guest = m.destination.clone();
            m.destination = format!("/vm/mounts/{}", i);
            VirtiofsMount {
                socket: format!("/vm/mounts/virtiofsd/{}", i),
                target: virtiofs_tag_in_guest,
            }
        })
        .collect();

    let mut cloudinit_mounts: Vec<&mut _> = mounts
        .iter_mut()
        .filter(|m| m.mount_type == Some("bind".to_string()) && m.destination == "/cloud-init")
        .collect();

    let has_cloudinit_config = if cloudinit_mounts.len() > 1 {
        return Err(Box::new(io::Error::other("more than one cloud-init mount")));
    } else if let Some(mount) = cloudinit_mounts.pop() {
        mount.destination = "/vm/cloud-init".to_string();
        true
    } else {
        false
    };

    mounts.push(libocispec::runtime::Mount {
        destination: "/vm/image".to_string(),
        gid_mappings: None,
        options: Some(vec!["bind".to_string(), "rprivate".to_string()]),
        source: Some(
            vm_image_path
                .canonicalize()?
                .to_str()
                .expect("path is utf-8")
                .to_string(),
        ),
        mount_type: Some("bind".to_string()),
        uid_mappings: None,
    });

    spec.save(&config_path)?;

    // create libvirt domain XML

    write_domain_xml(
        runner_root_path.join("vm/domain.xml"),
        &vm_image_format,
        &virtiofs_mounts,
        has_cloudinit_config,
    )?;

    // create runner container

    crun_create(global_args, args)?;

    Ok(())
}

struct VirtiofsMount {
    socket: String,
    target: String,
}

fn write_domain_xml(
    path: impl AsRef<Path>,
    image_format: &str,
    virtiofs_mounts: &[VirtiofsMount],
    has_cloudinit_config: bool,
) -> Result<(), Box<dyn Error>> {
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

    // section w/ text
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

    let w = &mut xml::EmitterConfig::new()
        .perform_indent(true)
        .create_writer(File::create(path)?);

    s(w, "domain", &[("type", "kvm")], |w| {
        st(w, "name", &[], "domain")?;

        se(w, "cpu", &[("mode", "host-model")])?;
        st(w, "vcpu", &[], "2")?;

        st(w, "memory", &[("unit", "GiB")], "2")?;

        s(w, "os", &[], |w| {
            st(w, "type", &[("arch", "x86_64"), ("machine", "q35")], "hvm")
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

            s(w, "disk", &[("type", "file"), ("device", "disk")], |w| {
                se(w, "target", &[("dev", "vda"), ("bus", "virtio")])?;
                se(w, "driver", &[("name", "qemu"), ("type", "qcow2")])?;
                se(w, "source", &[("file", "/vm/image-overlay.qcow2")])?;
                s(w, "backingStore", &[("type", "file")], |w| {
                    se(w, "format", &[("type", image_format)])?;
                    se(w, "source", &[("file", "/vm/image")])?;
                    se(w, "backingStore", &[])?;
                    Ok(())
                })?;
                Ok(())
            })?;

            if has_cloudinit_config {
                s(w, "disk", &[("type", "file"), ("device", "disk")], |w| {
                    se(w, "source", &[("file", "/vm/cloud-init.iso")])?;
                    se(w, "driver", &[("name", "qemu"), ("type", "raw")])?;
                    se(w, "target", &[("dev", "vdb"), ("bus", "virtio")])?;
                    Ok(())
                })?;
            }

            s(w, "interface", &[("type", "user")], |w| {
                se(w, "backend", &[("type", "passt")])?;
                se(w, "model", &[("type", "virtio")])?;
                Ok(())
            })?;

            for mount in virtiofs_mounts {
                s(w, "filesystem", &[("type", "mount")], |w| {
                    se(w, "driver", &[("type", "virtiofs"), ("queue", "1024")])?;
                    se(w, "source", &[("socket", &mount.socket)])?;
                    se(w, "target", &[("dir", &mount.target)])?;
                    Ok(())
                })?;
            }

            Ok(())
        })?;

        Ok(())
    })?;

    w.inner_mut().write_all(&[b'\n'])?;

    Ok(())
}

fn crun_create(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Create) -> io::Result<()> {
    // build crun argument list

    let mut arg_list = Vec::new();

    if global_args.debug {
        arg_list.push("--debug");
    }

    if let Some(path) = &global_args.log {
        arg_list.push("--log");
        arg_list.push(path.to_str().expect("path is utf-8"));
    }

    if let Some(format) = &global_args.log_format {
        arg_list.push("--log-format");
        arg_list.push(format);
    }

    if args.no_pivot {
        arg_list.push("--no-pivot");
    }

    if let Some(path) = &global_args.root {
        arg_list.push("--root");
        arg_list.push(path.to_str().expect("path is utf-8"));
    }

    if global_args.systemd_cgroup {
        arg_list.push("--systemd-cgroup");
    }

    arg_list.push("create");

    arg_list.push("--bundle");
    arg_list.push(args.bundle.to_str().expect("path is utf-8"));

    if let Some(path) = &args.console_socket {
        arg_list.push("--console-socket");
        arg_list.push(path.to_str().expect("path is utf-8"));
    }

    if args.no_new_keyring {
        arg_list.push("--no-new-keyring");
    }

    arg_list.push("--preserve-fds");
    let preserve_fds = args.preserve_fds.to_string();
    arg_list.push(&preserve_fds);

    if let Some(path) = &args.pid_file {
        arg_list.push("--pid-file");
        arg_list.push(path.to_str().expect("path is utf-8"));
    }

    arg_list.push(&args.container_id);

    // run crun

    crun(arg_list)
}
