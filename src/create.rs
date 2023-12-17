// SPDX-License-Identifier: GPL-2.0-or-later

use std::error::Error;
use std::fs::{self, File, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use xml::writer::XmlEvent;

use crate::util::{
    create_overlay_image, crun, find_single_file_in_directories, generate_cloud_init_iso,
    get_image_info,
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

    let mut spec = oci_spec::runtime::Spec::load(&config_path)?;

    // find VM image

    let root = spec
        .root()
        .as_ref()
        .expect("config.json includes configuration for the container's root filesystem");

    let vm_image_path = find_single_file_in_directories([root.path(), &root.path().join("disk")])?;
    let vm_image_info = get_image_info(&vm_image_path)?;

    // prepare root filesystem for runner container

    let runner_root_path = args.bundle.join("crun-qemu-runner-root");
    fs::create_dir_all(runner_root_path.join("crun-qemu"))?;

    const ENTRYPOINT_BYTES: &[u8] = include_bytes!("runner.sh");
    let entrypoint_path = runner_root_path.join("crun-qemu/runner.sh");
    fs::write(&entrypoint_path, ENTRYPOINT_BYTES)?;
    fs::set_permissions(&entrypoint_path, Permissions::from_mode(0o555))?;

    // create overlay image

    let overlay_image_path = runner_root_path.join("crun-qemu/image-overlay.qcow2");
    create_overlay_image(overlay_image_path, "/crun-qemu/image", &vm_image_info)?;

    // adjust config for runner container

    spec.set_root(Some(
        oci_spec::runtime::RootBuilder::default()
            .path(&runner_root_path)
            .readonly(false)
            .build()?,
    ));

    spec.set_process({
        let mut process = spec.process().clone().expect("process config");
        process.set_cwd(".".into());
        process.set_command_line(None);
        process.set_args(Some(vec!["/crun-qemu/runner.sh".to_string()]));
        Some(process)
    });

    spec.set_linux({
        let mut linux = spec.linux().clone().expect("linux config");

        linux.set_devices({
            let mut devices = linux.devices().clone().unwrap_or_default();

            let kvm_major_minor = fs::metadata("/dev/kvm")?.rdev();
            devices.push(
                oci_spec::runtime::LinuxDeviceBuilder::default()
                    .typ(oci_spec::runtime::LinuxDeviceType::C)
                    .path("/dev/kvm")
                    .major(i64::try_from(kvm_major_minor >> 8).unwrap())
                    .minor(i64::try_from(kvm_major_minor & 0xff).unwrap())
                    .build()?,
            );

            Some(devices)
        });

        Some(linux)
    });

    let virtiofs_mounts: Vec<VirtiofsMount>;
    let cloudinit_config: Option<PathBuf>;

    spec.set_mounts({
        let mut mounts = spec.mounts().clone().unwrap_or_default();

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

        virtiofs_mounts = mounts
            .iter_mut()
            .filter(|m| {
                m.typ() == &Some("bind".to_string())
                    && !m.destination().starts_with("/dev/")
                    && !ignore_mounts.contains(&m.destination().to_string_lossy().as_ref())
            })
            .enumerate()
            .map(|(i, m)| {
                let socket = format!("/crun-qemu/mounts/virtiofsd/{}", i);
                let target = m.destination().to_str().unwrap().to_string();
                m.set_destination(Path::new("/crun-qemu/mounts").join(i.to_string()));
                VirtiofsMount { socket, target }
            })
            .collect();

        let mut cloudinit_mounts: Vec<(usize, _)> = mounts
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.typ() == &Some("bind".to_string())
                    && m.destination().to_str() == Some("/cloud-init")
            })
            .map(|(i, m)| (i, m.source().clone()))
            .collect();

        cloudinit_config = if cloudinit_mounts.len() > 1 {
            return Err(Box::new(io::Error::other("more than one cloud-init mount")));
        } else if let Some((i, source)) = cloudinit_mounts.pop() {
            mounts.remove(i);
            source
        } else {
            None
        };

        for path in ["/bin", "/dev/log", "/etc/pam.d", "/lib", "/lib64", "/usr"] {
            mounts.push(
                oci_spec::runtime::MountBuilder::default()
                    .typ("bind")
                    .source(path)
                    .destination(path)
                    .options(["bind".to_string(), "rprivate".to_string(), "ro".to_string()])
                    .build()?,
            );
        }

        fs::create_dir_all(runner_root_path.join("etc"))?;
        fs::copy("/etc/passwd", runner_root_path.join("etc/passwd"))?;
        fs::copy("/etc/group", runner_root_path.join("etc/group"))?;

        mounts.push(
            oci_spec::runtime::MountBuilder::default()
                .typ("bind")
                .source(vm_image_path.canonicalize()?)
                .destination("/crun-qemu/image")
                .options(["bind".to_string(), "rprivate".to_string()])
                .build()?,
        );

        Some(mounts)
    });

    spec.save(&config_path)?;

    // create cloud-init config

    let needs_cloud_init = generate_cloud_init_iso(
        cloudinit_config,
        &runner_root_path,
        virtiofs_mounts.iter().map(|m| &m.target),
    )?;

    // create libvirt domain XML

    write_domain_xml(
        runner_root_path.join("crun-qemu/domain.xml"),
        &vm_image_info.format,
        &virtiofs_mounts,
        needs_cloud_init,
        &spec,
    )?;

    // create runner container

    crun_create(global_args, args)?;

    Ok(())
}

struct VirtiofsMount {
    socket: String,
    target: String,
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

fn write_domain_xml(
    path: impl AsRef<Path>,
    image_format: &str,
    virtiofs_mounts: &[VirtiofsMount],
    needs_cloud_init: bool,
    spec: &oci_spec::runtime::Spec,
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
        let vcpus = get_vcpu_count(spec).to_string();
        if let Some(cpu_set) = get_cpu_set(spec) {
            st(w, "vcpu", &[("cpuset", cpu_set.as_str())], vcpus.as_str())?;
        } else {
            st(w, "vcpu", &[], vcpus.as_str())?;
        }

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
                se(w, "source", &[("file", "/crun-qemu/image-overlay.qcow2")])?;
                s(w, "backingStore", &[("type", "file")], |w| {
                    se(w, "format", &[("type", image_format)])?;
                    se(w, "source", &[("file", "/crun-qemu/image")])?;
                    se(w, "backingStore", &[])?;
                    Ok(())
                })?;
                Ok(())
            })?;

            if needs_cloud_init {
                s(w, "disk", &[("type", "file"), ("device", "disk")], |w| {
                    se(
                        w,
                        "source",
                        &[("file", "/crun-qemu/cloud-init/cloud-init.iso")],
                    )?;
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
