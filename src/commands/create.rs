// SPDX-License-Identifier: GPL-2.0-or-later

use std::error::Error;
use std::fs::{self, File, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use sysinfo::SystemExt;
use xml::writer::XmlEvent;

use crate::crun::crun_create;
use crate::util::{create_overlay_vm_image, find_single_file_in_dirs, VmImageInfo};

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

    let vm_image_path = find_single_file_in_dirs([root.path(), &root.path().join("disk")])?;
    let vm_image_info = VmImageInfo::of(&vm_image_path)?;

    // prepare root filesystem for runner container

    let runner_root_path = args.bundle.join("crun-qemu-runner-root");
    fs::create_dir_all(runner_root_path.join("crun-qemu"))?;

    const ENTRYPOINT_BYTES: &[u8] = include_bytes!("entrypoint.sh");
    let entrypoint_path = runner_root_path.join("crun-qemu/entrypoint.sh");
    fs::write(&entrypoint_path, ENTRYPOINT_BYTES)?;
    fs::set_permissions(&entrypoint_path, Permissions::from_mode(0o555))?;

    // create overlay image

    let overlay_image_path = runner_root_path.join("crun-qemu/image-overlay.qcow2");
    create_overlay_vm_image(overlay_image_path, "/crun-qemu/image", &vm_image_info)?;

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
        process.set_args(Some(vec!["/crun-qemu/entrypoint.sh".to_string()]));
        Some(process)
    });

    let mut block_devices: Vec<BlockDevice>;

    spec.set_linux({
        let mut linux = spec.linux().clone().expect("linux config");

        linux.set_devices({
            let mut devices = linux.devices().clone().unwrap_or_default();

            block_devices = devices
                .iter()
                .filter(|d| d.typ() == oci_spec::runtime::LinuxDeviceType::B)
                .map(|d| BlockDevice {
                    source: format!("/dev/block/{}:{}", d.major(), d.minor()),
                    target: d.path().to_path_buf(),
                })
                .collect();

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

    let mut virtiofs_mounts: Vec<VirtiofsMount> = Vec::new();
    let cloudinit_config: Option<PathBuf>;
    let has_ignition_config: bool;
    let pub_key: String;

    spec.set_mounts({
        let mut mounts = spec.mounts().clone().unwrap_or_default();

        let ignore_mounts = [
            "/cloud-init",
            "/dev",
            "/etc/hostname",
            "/etc/hosts",
            "/etc/resolv.conf",
            "/ignition",
            "/proc",
            "/run/.containerenv",
            "/run/secrets",
            "/sys",
            "/sys/fs/cgroup",
        ];

        for (i, m) in mounts
            .iter_mut()
            .filter(|m| m.typ() == &Some("bind".to_string()))
            .enumerate()
        {
            if let Some(path) = m.source() {
                let meta = path.metadata()?;

                if meta.file_type().is_block_device() {
                    let new_dest = format!("/crun-qemu/bdevs/{}", i);
                    block_devices.push(BlockDevice {
                        source: new_dest.clone(),
                        target: m.destination().to_path_buf(),
                    });
                    m.set_destination(PathBuf::from(new_dest));
                } else if meta.file_type().is_dir()
                    && !m.destination().starts_with("/dev/")
                    && !ignore_mounts.contains(&m.destination().to_string_lossy().as_ref())
                {
                    let source = format!("/crun-qemu/mounts/{}", i);
                    let target = m.destination().to_str().unwrap().to_string();
                    m.set_destination(PathBuf::from(&source));
                    virtiofs_mounts.push(VirtiofsMount { source, target });
                }
            }
        }

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

        let mut ignition_mounts: Vec<(usize, _)> = mounts
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.typ() == &Some("bind".to_string())
                    && m.destination().to_str() == Some("/ignition")
            })
            .map(|(i, m)| (i, m.source().clone()))
            .collect();

        has_ignition_config = if ignition_mounts.len() > 1 {
            return Err(Box::new(io::Error::other("more than one Ignition mount")));
        } else if let Some((i, source)) = ignition_mounts.pop() {
            mounts.remove(i);
            match source {
                Some(source) => {
                    fs::copy(source, runner_root_path.join("crun-qemu/ignition.ign"))?;
                    true
                }
                None => false,
            }
        } else {
            false
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

        fs::create_dir_all(runner_root_path.join("root/.ssh"))?;
        let status = Command::new("ssh-keygen")
            .arg("-q")
            .arg("-f")
            .arg(runner_root_path.join("root/.ssh/id_rsa"))
            .arg("-N")
            .arg("")
            .spawn()?
            .wait()?;
        if !status.success() {
            return Err(Box::new(io::Error::other("ssh-keygen failed")));
        }
        pub_key = fs::read_to_string(runner_root_path.join("root/.ssh/id_rsa.pub"))?;

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

    let needs_cloud_init = gen_cloud_init_iso(
        cloudinit_config,
        &runner_root_path,
        block_devices.iter().map(|d| &d.target),
        virtiofs_mounts.iter().map(|m| &m.target),
        pub_key.trim(),
    )?;

    // create libvirt domain XML

    write_domain_xml(
        runner_root_path.join("crun-qemu/domain.xml"),
        &vm_image_info.format,
        &block_devices,
        &virtiofs_mounts,
        needs_cloud_init,
        has_ignition_config,
        &spec,
    )?;

    // create runner container

    crun_create(global_args, args)?;

    Ok(())
}

/// Returns `true` if a cloud-init config should be passed to the VM.
fn gen_cloud_init_iso(
    source_config_path: Option<impl AsRef<Path>>,
    runner_root: impl AsRef<Path>,
    block_device_targets: impl IntoIterator<Item = impl AsRef<Path>>,
    virtiofs_mounts: impl IntoIterator<Item = impl AsRef<str>>,
    container_pub_key: &str,
) -> Result<bool, Box<dyn Error>> {
    let virtiofs_mounts: Vec<_> = virtiofs_mounts.into_iter().collect();

    if source_config_path.is_none() && virtiofs_mounts.is_empty() {
        // user didn't specify a cloud-init config and we have nothing to add
        return Ok(false);
    }

    let config_path = runner_root.as_ref().join("crun-qemu/cloud-init");
    fs::create_dir_all(&config_path)?;

    // create copy of config

    for file in ["meta-data", "user-data", "vendor-data"] {
        let path = config_path.join(file);

        if let Some(source_config_path) = &source_config_path {
            let source_path = source_config_path.as_ref().join(file);
            if source_path.exists() {
                if !source_path.symlink_metadata()?.is_file() {
                    return Err(io::Error::other(format!(
                        "cloud-init: expected {file} to be a regular file"
                    ))
                    .into());
                }
                fs::copy(source_path, &path)?;
                continue;
            }
        }

        let mut f = File::create(path)?;
        if file == "user-data" {
            f.write_all(b"#cloud-config\n")?;
        }
    }

    // adjust user-data config

    let user_data_path = config_path.join("user-data");
    let user_data = fs::read_to_string(&user_data_path)?;

    if let Some(line) = user_data.lines().next() {
        if line.trim() != "#cloud-config" {
            return Err(io::Error::other(
                "cloud-init: expected shebang '#cloud-config' in user-data file",
            )
            .into());
        }
    }

    let mut user_data: serde_yaml::Value = serde_yaml::from_str(&user_data)
        .map_err(|e| io::Error::other(format!("cloud-init: invalid user-data file: {e}")))?;

    if let serde_yaml::Value::Null = &user_data {
        user_data = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }

    let user_data_mapping = match &mut user_data {
        serde_yaml::Value::Mapping(m) => m,
        _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
    };

    // adjust mounts

    if !user_data_mapping.contains_key("mounts") {
        user_data_mapping.insert("mounts".into(), serde_yaml::Value::Sequence(vec![]));
    }

    let mounts = match user_data_mapping.get_mut("mounts").unwrap() {
        serde_yaml::Value::Sequence(mounts) => mounts,
        _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
    };

    for mount in virtiofs_mounts {
        let mount = mount.as_ref();
        mounts.push(vec![mount, mount, "virtiofs", "defaults", "0", "0"].into());
    }

    // adjust authorized keys

    if !user_data_mapping.contains_key("ssh_authorized_keys") {
        user_data_mapping.insert(
            "ssh_authorized_keys".into(),
            serde_yaml::Value::Sequence(vec![]),
        );
    }

    let ssh_authorized_keys = match user_data_mapping.get_mut("ssh_authorized_keys").unwrap() {
        serde_yaml::Value::Sequence(keys) => keys,
        _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
    };

    ssh_authorized_keys.push(container_pub_key.into());

    // create block device symlinks

    if !user_data_mapping.contains_key("runcmd") {
        user_data_mapping.insert("runcmd".into(), serde_yaml::Value::Sequence(vec![]));
    }

    let runcmd = match user_data_mapping.get_mut("runcmd").unwrap() {
        serde_yaml::Value::Sequence(cmds) => cmds,
        _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
    };

    for (i, target) in block_device_targets.into_iter().enumerate() {
        let target: &Path = target.as_ref();
        let parent = match target.parent() {
            Some(path) if path.to_str() != Some("") => Some(path),
            _ => None,
        };

        if let Some(parent) = parent {
            runcmd.push(serde_yaml::Value::Sequence(vec![
                "mkdir".into(),
                "-p".into(),
                parent.to_str().expect("path is utf-8").into(),
            ]));
        }

        runcmd.push(serde_yaml::Value::Sequence(vec![
            "ln".into(),
            "--symbolic".into(),
            format!("/dev/disk/by-id/virtio-crun-qemu-bdev-{i}").into(),
            target.to_str().expect("path is utf-8").into(),
        ]));
    }

    // generate iso

    {
        let mut f = File::create(user_data_path)?;
        f.write_all(b"#cloud-config\n")?;
        serde_yaml::to_writer(&mut f, &user_data)?;
    }

    let status = Command::new("genisoimage")
        .arg("-output")
        .arg(
            runner_root
                .as_ref()
                .join("crun-qemu/cloud-init/cloud-init.iso"),
        )
        .arg("-volid")
        .arg("cidata")
        .arg("-joliet")
        .arg("-rock")
        .arg("-quiet")
        .arg(config_path.join("meta-data"))
        .arg(config_path.join("user-data"))
        .arg(config_path.join("vendor-data"))
        .spawn()?
        .wait()?;

    if !status.success() {
        return Err(io::Error::other("genisoimage failed").into());
    }

    Ok(true)
}

struct BlockDevice {
    source: String,
    target: PathBuf,
}

struct VirtiofsMount {
    source: String,
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

fn write_domain_xml(
    path: impl AsRef<Path>,
    image_format: &str,
    block_devices: &[BlockDevice],
    virtiofs_mounts: &[VirtiofsMount],
    needs_cloud_init: bool,
    has_ignition_config: bool,
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

        if has_ignition_config {
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
        }

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
                    se(w, "format", &[("type", image_format)])?;
                    se(w, "source", &[("file", "/crun-qemu/image")])?;
                    se(w, "backingStore", &[])?;
                    Ok(())
                })?;
                Ok(())
            })?;

            for (i, dev) in block_devices.iter().enumerate() {
                s(w, "disk", &[("type", "block"), ("device", "disk")], |w| {
                    se(w, "target", &[("dev", &next_dev_name()), ("bus", "virtio")])?;
                    se(w, "source", &[("dev", &dev.source)])?;
                    st(w, "serial", &[], &format!("crun-qemu-bdev-{i}"))?;
                    Ok(())
                })?;
            }

            if needs_cloud_init {
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
            }

            s(w, "interface", &[("type", "user")], |w| {
                se(w, "backend", &[("type", "passt")])?;
                se(w, "model", &[("type", "virtio")])?;
                s(w, "portForward", &[("proto", "tcp")], |w| {
                    se(w, "range", &[("start", "22"), ("to", "22")])
                })?;
                Ok(())
            })?;

            for mount in virtiofs_mounts {
                s(w, "filesystem", &[("type", "mount")], |w| {
                    se(w, "driver", &[("type", "virtiofs")])?;
                    s(w, "binary", &[("path", "/crun-qemu/virtiofsd")], |w| {
                        se(w, "sandbox", &[("mode", "chroot")])
                    })?;
                    se(w, "source", &[("dir", &mount.source)])?;
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
