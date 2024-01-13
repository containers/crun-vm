// SPDX-License-Identifier: GPL-2.0-or-later

mod custom_opts;
mod domain;
mod first_boot;
mod runtime_env;

use std::fs::{self, Permissions};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use nix::sys::stat::{major, makedev, minor, mknod, Mode, SFlag};

use crate::commands::create::custom_opts::CustomOptions;
use crate::commands::create::domain::set_up_libvirt_domain_xml;
use crate::commands::create::first_boot::FirstBootConfig;
use crate::commands::create::runtime_env::RuntimeEnv;
use crate::crun::crun_create;
use crate::util::{
    bind_mount_dir_with_different_context, bind_mount_file, create_overlay_vm_image,
    find_single_file_in_dirs, set_file_context, SpecExt, VmImageInfo,
};

pub fn create(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Create) -> Result<()> {
    let bundle_path: &Utf8Path = args.bundle.as_path().try_into()?;
    let config_path = bundle_path.join("config.json");

    let mut spec = oci_spec::runtime::Spec::load(&config_path)?;
    let original_root_path = spec.root_path()?.to_path_buf();

    let runtime_env = RuntimeEnv::current(&spec, &original_root_path)?;
    let custom_options = CustomOptions::from_spec(&spec, runtime_env)?;

    set_up_container_root(&mut spec, bundle_path, &custom_options)?;
    let base_vm_image_info =
        set_up_vm_image(&spec, bundle_path, &original_root_path, &custom_options)?;

    let mut mounts = Mounts::default();
    set_up_mounts(&mut spec, &mut mounts)?;
    set_up_devices(&mut spec, &mut mounts)?;
    set_up_blockdevs(&mut spec, &mut mounts, &custom_options)?;

    set_up_extra_container_mounts_and_devices(&mut spec)?;
    set_up_security(&mut spec);

    set_up_first_boot_config(&spec, &mounts, &custom_options, runtime_env)?;
    set_up_libvirt_domain_xml(&spec, &base_vm_image_info, &mounts, &custom_options)?;

    adjust_container_resources(&mut spec);

    spec.save(&config_path)?;
    spec.save(spec.root_path()?.join("crun-vm/config.json"))?; // to aid debugging

    crun_create(global_args, args)?; // actually create container

    Ok(())
}

fn set_up_container_root(
    spec: &mut oci_spec::runtime::Spec,
    bundle_path: &Utf8Path,
    custom_options: &CustomOptions,
) -> Result<()> {
    // create root directory

    spec.set_root(Some(
        oci_spec::runtime::RootBuilder::default()
            .path(bundle_path.join("crun-vm-root"))
            .readonly(false)
            .build()
            .unwrap(),
    ));

    fs::create_dir_all(spec.root_path()?)?;

    if let Some(context) = spec.mount_label() {
        // the directory we're using as the root for the container is not the one that podman
        // prepared for us, so we need to set its context ourselves to prevent SELinux from getting
        // angry at us
        set_file_context(spec.root_path()?, context)?;
    }

    // configure container entrypoint

    const ENTRYPOINT_BYTES: &[u8] = include_bytes!("entrypoint.sh");

    let entrypoint_path: Utf8PathBuf = spec.root_path()?.join("crun-vm/entrypoint.sh");
    fs::create_dir_all(entrypoint_path.parent().unwrap())?;

    fs::write(&entrypoint_path, ENTRYPOINT_BYTES)?;
    fs::set_permissions(&entrypoint_path, Permissions::from_mode(0o555))?;

    let command = match custom_options.print_libvirt_xml {
        true => vec!["cat", "/crun-vm/domain.xml"],
        false => vec!["/crun-vm/entrypoint.sh"],
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
    bundle_path: &Utf8Path,
    original_root_path: &Utf8Path,
    custom_options: &CustomOptions,
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

    let mirror_vm_image_path_in_container =
        Utf8Path::new("crun-vm/image").join(vm_image_path_in_host.file_name().unwrap());
    let mirror_vm_image_path_in_host = spec.root_path()?.join(&mirror_vm_image_path_in_container);
    let mirror_vm_image_path_in_container =
        Utf8Path::new("/").join(mirror_vm_image_path_in_container);

    let private_dir = if custom_options.persistent {
        let vm_image_dir_path = vm_image_path_in_host.parent().unwrap();
        let vm_image_dir_name = vm_image_dir_path.file_name().unwrap();

        let overlay_private_dir_name = format!(".crun-vm.{}.tmp", vm_image_dir_name);
        let overlay_private_dir_path = vm_image_dir_path
            .parent()
            .unwrap()
            .join(overlay_private_dir_name);

        overlay_private_dir_path
    } else {
        bundle_path.join("crun-vm-vm-image-overlayfs")
    };

    // We may need to change the VM image context to actually be able to access it, but we don't
    // want to change the user's original image file and also don't want to do a full data copy, so
    // we use an overlayfs mount, which allows us to expose the same file with a different context.
    //
    // TODO: Clean up `private_dir` when VM is terminated (would be best-effort, but better than
    // nothing).
    bind_mount_dir_with_different_context(
        vm_image_path_in_host.parent().unwrap(),
        mirror_vm_image_path_in_host.parent().unwrap(),
        spec.mount_label(),
        custom_options.persistent,
        private_dir,
    )?;

    let mut vm_image_info = VmImageInfo::of(&mirror_vm_image_path_in_host)?;

    if custom_options.persistent {
        // We want to propagate writes but not removal, so that the user's file isn't deleted by
        // Podman on cleanup, so we bind mount it on top of itself.

        bind_mount_file(&mirror_vm_image_path_in_host, &mirror_vm_image_path_in_host)?;

        vm_image_info.path = mirror_vm_image_path_in_container;
    } else {
        // The overlayfs mount already isolates the user's original image file from writes, but to
        // ensure that we get copy-on-write and page cache sharing even when the underlying file
        // system doesn't support reflinks, we create a qcow2 overlay and use that as the image.

        let overlay_vm_image_path_in_container = Utf8PathBuf::from("crun-vm/image-overlay.qcow2");
        let overlay_vm_image_path_in_host =
            spec.root_path()?.join(&overlay_vm_image_path_in_container);
        let overlay_vm_image_path_in_container =
            Utf8Path::new("/").join(overlay_vm_image_path_in_container);

        vm_image_info.path = mirror_vm_image_path_in_container;
        create_overlay_vm_image(&overlay_vm_image_path_in_host, &vm_image_info)?;

        vm_image_info.path = overlay_vm_image_path_in_container;
    }

    Ok(vm_image_info)
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

    add_bind_mount(spec, "/dev/kvm");
    add_char_dev(spec, "/dev/kvm")?;

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
    env: RuntimeEnv,
) -> Result<()> {
    let container_public_key = get_container_ssh_key_pair(spec, env)?;

    let config = FirstBootConfig {
        hostname: spec.hostname().as_deref(),
        container_public_key: &container_public_key,
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
/// This first attempts to use the current user's key pair, in case the VM does not support
/// cloud-init but the user injected their public key into it themselves.
fn get_container_ssh_key_pair(spec: &oci_spec::runtime::Spec, env: RuntimeEnv) -> Result<String> {
    let ssh_path = spec.root_path()?.join("root/.ssh");
    fs::create_dir_all(&ssh_path)?;

    let try_copy_user_key_pair = || -> Result<bool> {
        if env != RuntimeEnv::Other {
            // definitely not Podman, we're probably not running as the user that invoked the engine
            return Ok(false);
        }

        if let Some(user_home_path) = home::home_dir() {
            let user_ssh = user_home_path.join(".ssh");

            if user_ssh.join("id_rsa.pub").is_file() && user_ssh.join("id_rsa").is_file() {
                fs::copy(user_ssh.join("id_rsa.pub"), ssh_path.join("id_rsa.pub"))?;
                fs::copy(user_ssh.join("id_rsa"), ssh_path.join("id_rsa"))?;
                return Ok(true);
            }
        }

        Ok(false)
    };

    if !try_copy_user_key_pair()? {
        let status = Command::new("ssh-keygen")
            .arg("-q")
            .arg("-f")
            .arg(ssh_path.join("id_rsa"))
            .arg("-N")
            .arg("")
            .spawn()?
            .wait()?;

        ensure!(status.success(), "ssh-keygen failed");
    }

    Ok(fs::read_to_string(ssh_path.join("id_rsa.pub"))?)
}

fn adjust_container_resources(spec: &mut oci_spec::runtime::Spec) {
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
