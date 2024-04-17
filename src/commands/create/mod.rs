// SPDX-License-Identifier: GPL-2.0-or-later

mod custom_opts;
mod domain;
mod first_boot;
mod runtime_env;

use std::ffi::OsStr;
use std::fs::{self, File, Permissions};
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use nix::sys::stat::{major, makedev, minor, mknod, Mode, SFlag};
use rust_embed::RustEmbed;

use crate::commands::create::custom_opts::CustomOptions;
use crate::commands::create::domain::set_up_libvirt_domain_xml;
use crate::commands::create::first_boot::FirstBootConfig;
use crate::commands::create::runtime_env::RuntimeEnv;
use crate::util::{
    bind_mount_dir_with_different_context, bind_mount_file, create_overlay_vm_image, crun,
    find_single_file_in_dirs, is_mountpoint, set_file_context, SpecExt, VmImageInfo,
};

pub fn create(args: &liboci_cli::Create, raw_args: &[impl AsRef<OsStr>]) -> Result<()> {
    let bundle_path: &Utf8Path = args.bundle.as_path().try_into()?;
    let config_path = bundle_path.join("config.json");

    let mut spec = oci_spec::runtime::Spec::load(&config_path)?;
    let original_root_path: Utf8PathBuf = spec.root_path()?.canonicalize()?.try_into()?; // ensure absolute

    if let Some(process) = spec.process().as_ref() {
        if let Some(capabilities) = process.capabilities().as_ref() {
            fn any_is_cap_sys_admin(caps: &Option<oci_spec::runtime::Capabilities>) -> bool {
                caps.as_ref()
                    .is_some_and(|set| set.contains(&oci_spec::runtime::Capability::SysAdmin))
            }

            ensure!(
                !any_is_cap_sys_admin(capabilities.bounding())
                    && !any_is_cap_sys_admin(capabilities.effective())
                    && !any_is_cap_sys_admin(capabilities.inheritable())
                    && !any_is_cap_sys_admin(capabilities.permitted())
                    && !any_is_cap_sys_admin(capabilities.ambient()),
                "crun-vm is incompatible with privileged containers"
            );
        }
    }

    let runtime_env = RuntimeEnv::current(&spec, &original_root_path)?;
    let custom_options = CustomOptions::from_spec(&spec, runtime_env)?;

    // We include container_id in our paths to ensure no overlap with the user container's contents.
    let priv_dir_path = original_root_path.join(format!("crun-vm-{}", args.container_id));
    fs::create_dir_all(&priv_dir_path)?;

    if let Some(context) = spec.mount_label() {
        // the directory we're using as the root for the container is not the one that podman
        // prepared for us, so we need to set its context ourselves to prevent SELinux from getting
        // angry at us
        set_file_context(&priv_dir_path, context)?;
    }

    set_up_container_root(&mut spec, &priv_dir_path, &custom_options)?;
    let is_first_create = is_first_create(&spec)?;

    let base_vm_image_info = set_up_vm_image(
        &spec,
        &original_root_path,
        &priv_dir_path,
        &custom_options,
        is_first_create,
    )?;

    let mut mounts = Mounts::default();
    set_up_mounts(&mut spec, &mut mounts)?;
    set_up_devices(&mut spec, &mut mounts)?;
    set_up_blockdevs(&mut spec, &mut mounts, &custom_options)?;

    set_up_extra_container_mounts_and_devices(&mut spec)?;
    set_up_security(&mut spec);

    if is_first_create {
        let ssh_pub_key =
            set_up_ssh_key_pair(&mut spec, &custom_options, runtime_env, &priv_dir_path)?;

        set_up_first_boot_config(&spec, &mounts, &custom_options, &ssh_pub_key)?;
        set_up_libvirt_domain_xml(&spec, &base_vm_image_info, &mounts, &custom_options)?;
    }

    adjust_container_rlimits_and_resources(&mut spec);

    spec.save(&config_path)?;
    spec.save(spec.root_path()?.join("crun-vm/config.json"))?; // to aid debugging

    crun(raw_args)?; // actually create container

    Ok(())
}

fn is_first_create(spec: &oci_spec::runtime::Spec) -> Result<bool> {
    let path = spec.root_path()?.join("crun-vm/create-ran");

    let error = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .err();

    match error {
        Some(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        Some(e) => Err(e.into()),
        None => Ok(true),
    }
}

fn set_up_container_root(
    spec: &mut oci_spec::runtime::Spec,
    priv_dir_path: &Utf8Path,
    custom_options: &CustomOptions,
) -> Result<()> {
    let new_root_path = priv_dir_path.join("root");
    fs::create_dir_all(&new_root_path)?;

    // create root directory

    spec.set_root(Some(
        oci_spec::runtime::RootBuilder::default()
            .path(&new_root_path)
            .readonly(false)
            .build()
            .unwrap(),
    ));

    // set up container scripts

    #[derive(RustEmbed)]
    #[folder = "scripts/"]
    struct Scripts;

    for path in Scripts::iter() {
        let path_in_host = new_root_path.join("crun-vm").join(path.as_ref());
        fs::create_dir_all(path_in_host.parent().unwrap())?;

        let file = Scripts::get(&path).unwrap();
        fs::write(&path_in_host, file.data)?;
        fs::set_permissions(&path_in_host, Permissions::from_mode(0o755))?;
    }

    // configure container entrypoint

    let command = if custom_options.print_libvirt_xml {
        vec!["cat", "/crun-vm/domain.xml"]
    } else if custom_options.print_config_json {
        vec!["cat", "/crun-vm/config.json"]
    } else {
        vec!["/crun-vm/entrypoint.sh"]
    };

    spec.set_process({
        let mut process = spec.process().clone().unwrap();
        process.set_cwd(".".into());
        process.set_command_line(None);
        process.set_args(Some(command.into_iter().map(String::from).collect()));
        Some(process)
    });

    Ok(())
}

fn set_up_vm_image(
    spec: &oci_spec::runtime::Spec,
    original_root_path: &Utf8Path,
    priv_dir_path: &Utf8Path,
    custom_options: &CustomOptions,
    is_first_create: bool,
) -> Result<VmImageInfo> {
    // where inside the container to look for the VM image
    const VM_IMAGE_SEARCH_PATHS: [&str; 2] = ["./", "disk/"];

    // docker may add these files to the root of the container
    const FILES_TO_IGNORE: [&str; 2] = [".dockerinit", ".dockerenv"];

    let vm_image_path_in_host = find_single_file_in_dirs(
        VM_IMAGE_SEARCH_PATHS.map(|p| original_root_path.join(p)),
        &FILES_TO_IGNORE.map(|f| original_root_path.join(f)),
    )?;

    // mount user-provided VM image file into container

    // Make VM image file available in a subtree that doesn't overlap our internal container root so
    // overlayfs works.

    let image_dir_path = priv_dir_path.join("image");
    fs::create_dir_all(&image_dir_path)?;

    if !image_dir_path.join("image").exists() {
        fs::hard_link(vm_image_path_in_host, image_dir_path.join("image"))?;
    }

    let mirror_vm_image_path_in_container = Utf8PathBuf::from("/crun-vm/image/image");
    let mirror_vm_image_path_in_host = spec.root_path()?.join("crun-vm/image/image");

    if custom_options.persistent {
        // Mount overlayfs to expose the user's VM image file with a different SELinux context so we
        // can always access it, using the file's parent as the upperdir so that writes still
        // propagate to it.

        if !is_mountpoint(mirror_vm_image_path_in_host.parent().unwrap())? {
            bind_mount_dir_with_different_context(
                image_dir_path,
                mirror_vm_image_path_in_host.parent().unwrap(),
                priv_dir_path.join("scratch"),
                spec.mount_label(),
                false,
            )?;
        }

        // Prevent the container engine from deleting the user's actual VM image file by mounting it
        // on top of itself under our overlayfs mount.

        bind_mount_file(&mirror_vm_image_path_in_host, &mirror_vm_image_path_in_host)?;

        let mut vm_image_info = VmImageInfo::of(&mirror_vm_image_path_in_host)?;
        vm_image_info.path = mirror_vm_image_path_in_container;

        Ok(vm_image_info)
    } else {
        // Mount overlayfs to expose the user's VM image file with a different SELinux context so we
        // can always access it.

        if !is_mountpoint(mirror_vm_image_path_in_host.parent().unwrap())? {
            bind_mount_dir_with_different_context(
                image_dir_path,
                mirror_vm_image_path_in_host.parent().unwrap(),
                priv_dir_path.join("scratch"),
                spec.mount_label(),
                true,
            )?;
        }

        // The overlayfs mount forbids writes to the VM image file, and also we want to get
        // copy-on-write and page cache sharing even when the underlying file system doesn't support
        // reflinks, so we create a qcow2 overlay image.

        let overlay_vm_image_path_in_container = Utf8PathBuf::from("crun-vm/image-overlay.qcow2");
        let overlay_vm_image_path_in_host =
            spec.root_path()?.join(&overlay_vm_image_path_in_container);
        let overlay_vm_image_path_in_container =
            Utf8Path::new("/").join(overlay_vm_image_path_in_container);

        let mut base_vm_image_info = VmImageInfo::of(&mirror_vm_image_path_in_host)?;
        base_vm_image_info.path = mirror_vm_image_path_in_container;

        if is_first_create {
            create_overlay_vm_image(&overlay_vm_image_path_in_host, &base_vm_image_info)?;
        }

        Ok(VmImageInfo {
            path: Utf8Path::new("/").join(overlay_vm_image_path_in_container),
            size: base_vm_image_info.size,
            format: "qcow2".to_string(),
        })
    }
}

#[derive(Default)]
struct Mounts {
    virtiofs: Vec<VirtiofsMount>,
    tmpfs: Vec<TmpfsMount>,
    block_device: Vec<BlockDeviceMount>,
}

struct BlockDeviceMount {
    format: String,
    is_regular_file: bool,
    path_in_container: Utf8PathBuf,
    path_in_guest: Utf8PathBuf,
    readonly: bool,
}

struct VirtiofsMount {
    path_in_container: Utf8PathBuf,
    path_in_guest: Utf8PathBuf,
}

struct TmpfsMount {
    path_in_guest: Utf8PathBuf,
}

fn set_up_mounts(spec: &mut oci_spec::runtime::Spec, mounts: &mut Mounts) -> Result<()> {
    const TARGETS_TO_IGNORE: &[&str] = &[
        "/etc/hostname",
        "/etc/hosts",
        "/etc/resolv.conf",
        "/proc",
        "/run/.containerenv",
        "/run/secrets",
        "/sys",
        "/sys/fs/cgroup",
    ];

    let mut new_oci_mounts: Vec<oci_spec::runtime::Mount> = vec![];

    for oci_mount in spec.mounts().iter().flatten() {
        if TARGETS_TO_IGNORE
            .iter()
            .any(|path| oci_mount.destination() == Utf8Path::new(path))
        {
            new_oci_mounts.push(oci_mount.clone());
            continue;
        }

        match oci_mount.typ().as_deref() {
            Some("bind") => {
                let meta = oci_mount.source().as_ref().unwrap().metadata()?;

                let path_in_container;

                if meta.file_type().is_dir() {
                    if oci_mount.destination().starts_with("/dev") {
                        new_oci_mounts.push(oci_mount.clone());
                        continue;
                    }

                    path_in_container = Utf8PathBuf::from(format!(
                        "/crun-vm/mounts/virtiofs/{}",
                        mounts.virtiofs.len()
                    ));
                    let path_in_guest = oci_mount.destination().clone().try_into()?;

                    mounts.virtiofs.push(VirtiofsMount {
                        path_in_container: path_in_container.clone(),
                        path_in_guest,
                    });
                } else if meta.file_type().is_block_device() || meta.file_type().is_file() {
                    let readonly = oci_mount
                        .options()
                        .iter()
                        .flatten()
                        .any(|o| o == "ro" || o == "readonly");

                    path_in_container = Utf8PathBuf::from(format!(
                        "crun-vm/mounts/block/{}",
                        mounts.block_device.len()
                    ));
                    let path_in_guest = oci_mount.destination().clone().try_into()?;

                    mounts.block_device.push(BlockDeviceMount {
                        format: "raw".to_string(),
                        is_regular_file: meta.file_type().is_file(),
                        path_in_container: path_in_container.clone(),
                        path_in_guest,
                        readonly,
                    });
                } else {
                    bail!("can only bind mount regular files, directories, and block devices");
                }

                // redirect the mount to a path in the container that we control
                let mut new_mount = oci_mount.clone();
                new_mount.set_destination(path_in_container.as_std_path().to_path_buf());
                new_oci_mounts.push(new_mount);
            }
            Some("tmpfs") => {
                if oci_mount.destination().starts_with("/dev") {
                    new_oci_mounts.push(oci_mount.clone());
                    continue;
                }

                // don't actually mount it in the container

                let path_in_guest = oci_mount.destination().clone().try_into()?;
                mounts.tmpfs.push(TmpfsMount { path_in_guest });
            }
            _ => {
                new_oci_mounts.push(oci_mount.clone());
            }
        }
    }

    spec.set_mounts(Some(new_oci_mounts));

    Ok(())
}

fn set_up_devices(spec: &mut oci_spec::runtime::Spec, mounts: &mut Mounts) -> Result<()> {
    // set up block devices passed in using --device (note that rootless podman will turn those into
    // --mount/--volume instead)

    for device in spec.linux_devices() {
        if device.typ() != oci_spec::runtime::LinuxDeviceType::B {
            continue;
        }

        let major: u64 = device.major().try_into().unwrap();
        let minor: u64 = device.minor().try_into().unwrap();
        let mode = device.file_mode().unwrap();

        let path_in_container = Utf8PathBuf::from(format!(
            "crun-vm/mounts/block/{}",
            mounts.block_device.len()
        ));
        let path_in_guest = device.path().clone().try_into()?;

        fs::create_dir_all(spec.root_path()?.join(&path_in_container).parent().unwrap())?;

        mknod(
            spec.root_path()?.join(&path_in_container).as_std_path(),
            SFlag::S_IFBLK,
            Mode::from_bits_retain(mode),
            makedev(major, minor),
        )?;

        mounts.block_device.push(BlockDeviceMount {
            format: "raw".to_string(),
            is_regular_file: false,
            path_in_container,
            path_in_guest,
            readonly: mode & 0o222 == 0,
        });
    }

    Ok(())
}

fn set_up_blockdevs(
    spec: &mut oci_spec::runtime::Spec,
    mounts: &mut Mounts,
    custom_options: &CustomOptions,
) -> Result<()> {
    // set up devices specified using --blockdev

    for blockdev in &custom_options.blockdev {
        let meta = blockdev.source.metadata()?;
        ensure!(
            meta.file_type().is_file() || meta.file_type().is_block_device(),
            "blockdev source must be a regular file or a block device"
        );

        let path_in_container = Utf8PathBuf::from(format!(
            "crun-vm/mounts/block/{}",
            mounts.block_device.len()
        ));
        let path_in_guest = blockdev.target.clone();

        fs::create_dir_all(spec.root_path()?.join(&path_in_container).parent().unwrap())?;

        // mount from the host to the container
        spec.mounts_push(
            oci_spec::runtime::MountBuilder::default()
                .typ("bind")
                .source(blockdev.source.canonicalize()?)
                .destination(&path_in_container)
                .options(["bind".to_string(), "rprivate".to_string()])
                .build()
                .unwrap(),
        );

        // and mount from the container to the guest
        mounts.block_device.push(BlockDeviceMount {
            format: blockdev.format.clone(),
            is_regular_file: meta.is_file(),
            path_in_container,
            path_in_guest,
            readonly: false,
        });
    }

    Ok(())
}

fn set_up_extra_container_mounts_and_devices(spec: &mut oci_spec::runtime::Spec) -> Result<()> {
    fn add_bind_mount(spec: &mut oci_spec::runtime::Spec, path: impl AsRef<Path>) {
        spec.mounts_push(
            oci_spec::runtime::MountBuilder::default()
                .typ("bind")
                .source(path.as_ref())
                .destination(path.as_ref())
                .options(["bind".to_string(), "rprivate".to_string(), "ro".to_string()])
                .build()
                .unwrap(),
        );
    }

    fn add_char_dev(spec: &mut oci_spec::runtime::Spec, path: impl AsRef<Path>) -> Result<()> {
        let rdev = fs::metadata(path.as_ref())?.rdev();

        spec.linux_resources_devices_push(
            oci_spec::runtime::LinuxDeviceCgroupBuilder::default()
                .allow(true)
                .typ(oci_spec::runtime::LinuxDeviceType::C)
                .major(i64::try_from(major(rdev))?)
                .minor(i64::try_from(minor(rdev))?)
                .access("rwm")
                .build()
                .unwrap(),
        );

        Ok(())
    }

    fs::create_dir_all(spec.root_path()?.join("etc"))?;
    fs::copy("/etc/passwd", spec.root_path()?.join("etc/passwd"))?;
    fs::copy("/etc/group", spec.root_path()?.join("etc/group"))?;

    for path in ["/bin", "/dev/log", "/etc/pam.d", "/lib", "/lib64", "/usr"] {
        add_bind_mount(spec, path);
    }

    // If virsh runs with a tty and finds an executable at /usr/bin/pkttyagent, it will attempt to
    // run it even if polkit auth is disabled, resulting in "Authorization not available. Check if
    // polkit service is running or see debug message for more information." messages.
    spec.mounts_push(
        oci_spec::runtime::MountBuilder::default()
            .typ("bind")
            .source("/dev/null")
            .destination("/usr/bin/pkttyagent")
            .options(["bind".to_string(), "rprivate".to_string(), "ro".to_string()])
            .build()
            .unwrap(),
    );

    add_bind_mount(spec, "/dev/kvm");
    add_char_dev(spec, "/dev/kvm")?;

    // in case user sets up VFIO passthrough by overriding the libvirt XML
    for entry in fs::read_dir("/dev/vfio")? {
        let entry = entry?;
        if entry.metadata()?.file_type().is_char_device() {
            add_bind_mount(spec, entry.path());
            add_char_dev(spec, entry.path())?;
        }
    }

    Ok(())
}

fn set_up_security(spec: &mut oci_spec::runtime::Spec) {
    // Some environments, notably CRI-O, launch the container without CAP_CHROOT by default, which
    // we need for passt's --sandbox=chroot.
    //
    // TODO: This doesn't seem reasonable. Should we just force users to configure the additional
    // capability? Should we just launch passt with --sanbox=none?
    spec.process_capabilities_insert_beip(oci_spec::runtime::Capability::SysChroot);

    // Docker's default seccomp profile blocks some systems calls that passt requires, so we
    // just adjust the profile to allow them.
    //
    // TODO: This doesn't seem reasonable at all. Should we just force users to use a different
    // seccomp profile? Should passt provide the option to bypass a lot of the isolation that it
    // does, given we're already in a container *and* under a seccomp profile?
    spec.linux_seccomp_syscalls_push(
        oci_spec::runtime::LinuxSyscallBuilder::default()
            .names(["mount", "pivot_root", "umount2", "unshare"].map(String::from))
            .action(oci_spec::runtime::LinuxSeccompAction::ScmpActAllow)
            .build()
            .unwrap(),
    );
}

/// Configure cloud-init and Ignition for first-boot customization.
fn set_up_first_boot_config(
    spec: &oci_spec::runtime::Spec,
    mounts: &Mounts,
    custom_options: &CustomOptions,
    container_public_key: &str,
) -> Result<()> {
    let config = FirstBootConfig {
        hostname: spec.hostname().as_deref(),
        container_public_key,
        password: custom_options.password.as_deref(),
        mounts,
    };

    config
        .apply_to_cloud_init_config(
            custom_options.cloud_init.as_ref(),
            spec.root_path()?.join("crun-vm/first-boot/cloud-init"),
            spec.root_path()?.join("crun-vm/first-boot/cloud-init.iso"),
        )
        .context("failed to load cloud-init config")?;

    config
        .apply_to_ignition_config(
            custom_options.ignition.as_ref(),
            spec.root_path()?.join("crun-vm/first-boot/ignition.ign"),
        )
        .context("failed to load ignition config")?;

    Ok(())
}

/// Returns the public key for the container.
///
/// This first attempts to use the current user's key pair, just in case the VM does not support
/// cloud-init but the user injected their public key into it themselves.
fn set_up_ssh_key_pair(
    spec: &mut oci_spec::runtime::Spec,
    custom_options: &CustomOptions,
    env: RuntimeEnv,
    priv_dir_path: &Utf8Path,
) -> Result<String> {
    let user_home: Utf8PathBuf = home::home_dir()
        .ok_or_else(|| anyhow!("could not determine user home"))?
        .try_into()?;

    let user_ssh_dir = user_home.join(".ssh");
    let container_ssh_dir = spec.root_path()?.join("root/.ssh");

    // Use the host user's key pair if:
    //   - The user didn't set the --random-ssh-key-pair flag; and
    //   - We're not running under Docker (otherwise we'd probably not be running as the user that
    //     invoked the engine); and
    //   - We're not running under Kubernetes (where there isn't a "host user"); and
    //   - They have a key pair.
    let use_user_key_pair = !custom_options.random_ssh_key_pair
        && env == RuntimeEnv::Other
        && user_ssh_dir.join("id_rsa.pub").is_file()
        && user_ssh_dir.join("id_rsa").is_file();

    if use_user_key_pair {
        // use host user's key pair

        bind_mount_dir_with_different_context(
            &user_ssh_dir,
            &container_ssh_dir,
            priv_dir_path.join("scratch-ssh"),
            spec.mount_label(),
            true,
        )?;
    } else {
        // use new key pair

        fs::create_dir_all(&container_ssh_dir)?;

        let status = Command::new("ssh-keygen")
            .arg("-q")
            .arg("-f")
            .arg(container_ssh_dir.join("id_rsa"))
            .arg("-N")
            .arg("")
            .arg("-C")
            .arg("")
            .spawn()?
            .wait()?;

        ensure!(status.success(), "ssh-keygen failed");
    }

    Ok(fs::read_to_string(container_ssh_dir.join("id_rsa.pub"))?)
}

fn adjust_container_rlimits_and_resources(spec: &mut oci_spec::runtime::Spec) {
    if let Some(process) = spec.process() {
        if let Some(rlimits) = process.rlimits() {
            let mut process = process.clone();
            let mut rlimits = rlimits.clone();

            // Forwarding all UDP and TCP traffic requires passt to open many sockets. Ensure that
            // the container's RLIMIT_NOFILE is large enough.
            rlimits.retain(|rl| rl.typ() != oci_spec::runtime::LinuxRlimitType::RlimitNofile);
            rlimits.push(
                oci_spec::runtime::LinuxRlimitBuilder::default()
                    .typ(oci_spec::runtime::LinuxRlimitType::RlimitNofile)
                    .hard(262144u64)
                    .soft(262144u64)
                    .build()
                    .unwrap(),
            );

            process.set_rlimits(Some(rlimits));
            spec.set_process(Some(process));
        }
    }

    if let Some(linux) = spec.linux() {
        if let Some(resources) = linux.resources() {
            let mut linux = linux.clone();
            let mut resources = resources.clone();

            resources.set_cpu(None);
            resources.set_memory(None);

            linux.set_resources(Some(resources));
            spec.set_linux(Some(linux));
        }
    }
}
