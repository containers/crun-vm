// SPDX-License-Identifier: GPL-2.0-or-later

mod custom_opts;
mod domain;
mod first_boot;
mod runtime_env;

use std::error::Error;
use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn create(
    global_args: &liboci_cli::GlobalOpts,
    args: &liboci_cli::Create,
) -> Result<(), Box<dyn Error>> {
    let config_path = args.bundle.join("config.json");

    let mut spec = oci_spec::runtime::Spec::load(&config_path)?;
    let original_root_path = spec.root_path().clone();

    let runtime_env = RuntimeEnv::current(&spec, &original_root_path)?;
    let custom_options = CustomOptions::from_spec(&spec, runtime_env)?;

    set_up_container_root(&mut spec, &args.bundle)?;
    let base_vm_image_info =
        set_up_vm_image(&spec, &args.bundle, &original_root_path, &custom_options)?;

    let (virtiofs_mounts, tmpfs_mounts) = set_up_mounts(&mut spec)?;
    let block_device_mounts = set_up_block_devices(&mut spec)?;

    let mounts = Mounts {
        virtiofs: virtiofs_mounts,
        tmpfs: tmpfs_mounts,
        block_device: block_device_mounts,
    };

    set_up_char_devices(&mut spec)?;

    set_up_extra_container_mounts(&mut spec)?;
    set_up_security(&mut spec);

    spec.save(&config_path)?;
    spec.save(spec.root_path().join("crun-qemu/config.json"))?; // to aid debugging

    set_up_first_boot_config(&spec, &mounts, &custom_options)?;
    set_up_libvirt_domain_xml(&spec, &base_vm_image_info, &mounts, &custom_options)?;

    crun_create(global_args, args)?; // actually create container

    Ok(())
}

fn set_up_container_root(
    spec: &mut oci_spec::runtime::Spec,
    bundle_path: &Path,
) -> Result<(), Box<dyn Error>> {
    // create root directory

    spec.set_root(Some(
        oci_spec::runtime::RootBuilder::default()
            .path(bundle_path.join("crun-qemu-root"))
            .readonly(false)
            .build()
            .unwrap(),
    ));

    fs::create_dir(spec.root_path())?;

    if let Some(context) = spec.mount_label() {
        // the directory we're using as the root for the container is not the one that podman
        // prepared for us, so we need to set its context ourselves to prevent SELinux from getting
        // angry at us
        set_file_context(spec.root_path(), context)?;
    }

    // configure container entrypoint

    const ENTRYPOINT_BYTES: &[u8] = include_bytes!("entrypoint.sh");

    let entrypoint_path: PathBuf = spec.root_path().join("crun-qemu/entrypoint.sh");
    fs::create_dir_all(entrypoint_path.parent().unwrap())?;

    fs::write(&entrypoint_path, ENTRYPOINT_BYTES)?;
    fs::set_permissions(&entrypoint_path, Permissions::from_mode(0o555))?;

    spec.set_process({
        let mut process = spec.process().clone().unwrap();
        process.set_cwd(".".into());
        process.set_command_line(None);
        process.set_args(Some(vec!["/crun-qemu/entrypoint.sh".to_string()]));
        Some(process)
    });

    Ok(())
}

fn set_up_vm_image(
    spec: &oci_spec::runtime::Spec,
    bundle_path: &Path,
    original_root_path: &Path,
    custom_options: &CustomOptions,
) -> Result<VmImageInfo, Box<dyn Error>> {
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
        Path::new("crun-qemu/image").join(vm_image_path_in_host.file_name().unwrap());
    let mirror_vm_image_path_in_host = spec.root_path().join(&mirror_vm_image_path_in_container);
    let mirror_vm_image_path_in_container = Path::new("/").join(mirror_vm_image_path_in_container);

    let private_dir = if custom_options.persist_changes {
        let vm_image_dir_path = vm_image_path_in_host.parent().unwrap();
        let vm_image_dir_name = vm_image_dir_path.file_name().unwrap();

        let overlay_private_dir_name =
            format!(".crun-qemu.{}.tmp", vm_image_dir_name.to_str().unwrap());
        let overlay_private_dir_path = vm_image_dir_path
            .parent()
            .unwrap()
            .join(overlay_private_dir_name);

        overlay_private_dir_path
    } else {
        bundle_path.join("crun-qemu-vm-image-overlayfs")
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
        custom_options.persist_changes,
        private_dir,
    )?;

    let mut vm_image_info = VmImageInfo::of(&mirror_vm_image_path_in_host)?;

    if custom_options.persist_changes {
        // We want to propagate writes but not removal, so that the user's file isn't deleted by
        // Podman on cleanup, so we bind mount it on top of itself.

        bind_mount_file(&mirror_vm_image_path_in_host, &mirror_vm_image_path_in_host)?;

        vm_image_info.path = mirror_vm_image_path_in_container;
    } else {
        // The overlayfs mount already isolates the user's original image file from writes, but to
        // ensure that we get copy-on-write and page cache sharing even when the underlying file
        // system doesn't support reflinks, we create a qcow2 overlay and use that as the image.

        let overlay_vm_image_path_in_container = PathBuf::from("crun-qemu/image-overlay.qcow2");
        let overlay_vm_image_path_in_host =
            spec.root_path().join(&overlay_vm_image_path_in_container);
        let overlay_vm_image_path_in_container =
            Path::new("/").join(overlay_vm_image_path_in_container);

        vm_image_info.path = mirror_vm_image_path_in_container;
        create_overlay_vm_image(&overlay_vm_image_path_in_host, &vm_image_info)?;

        vm_image_info.path = overlay_vm_image_path_in_container;
    }

    Ok(vm_image_info)
}

struct Mounts {
    virtiofs: Vec<VirtiofsMount>,
    tmpfs: Vec<TmpfsMount>,
    block_device: Vec<BlockDeviceMount>,
}

struct BlockDeviceMount {
    path_in_container: PathBuf,
    path_in_guest: PathBuf,
}

struct VirtiofsMount {
    path_in_container: PathBuf,
    path_in_guest: PathBuf,
}

struct TmpfsMount {
    path_in_guest: PathBuf,
}

fn set_up_mounts(
    spec: &mut oci_spec::runtime::Spec,
) -> Result<(Vec<VirtiofsMount>, Vec<TmpfsMount>), Box<dyn Error>> {
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

    const TARGET_PREFIXES_TO_IGNORE: &[&str] = &["/dev"];

    let mut new_mounts: Vec<oci_spec::runtime::Mount> = vec![];

    let mut virtiofs_mounts: Vec<VirtiofsMount> = vec![];
    let mut tmpfs_mounts: Vec<TmpfsMount> = vec![];

    for mount in spec.mounts().iter().flatten() {
        if TARGETS_TO_IGNORE
            .iter()
            .any(|path| mount.destination() == Path::new(path))
        {
            new_mounts.push(mount.clone());
            continue;
        }

        if TARGET_PREFIXES_TO_IGNORE
            .iter()
            .any(|prefix| mount.destination().starts_with(prefix))
        {
            new_mounts.push(mount.clone());
            continue;
        }

        match mount.typ().as_deref() {
            Some("bind") => {
                let meta = match mount.source() {
                    Some(source) => source.metadata()?,
                    None => continue,
                };

                if !meta.file_type().is_dir() {
                    continue;
                }

                let path_in_container =
                    PathBuf::from(format!("/crun-qemu/virtiofs/{}", virtiofs_mounts.len()));
                let path_in_guest = mount.destination().clone();

                // redirect the mount to a path in the container that we control
                let mut new_mount = mount.clone();
                new_mount.set_destination(path_in_container.clone());
                new_mounts.push(new_mount);

                virtiofs_mounts.push(VirtiofsMount {
                    path_in_container,
                    path_in_guest,
                });
            }
            Some("tmpfs") => {
                // don't actually mount it in the container

                let path_in_guest = mount.destination().clone();
                tmpfs_mounts.push(TmpfsMount { path_in_guest });
            }
            _ => {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only bind and tmpfs mounts are supported",
                )))
            }
        }
    }

    spec.set_mounts(Some(new_mounts));

    Ok((virtiofs_mounts, tmpfs_mounts))
}

fn set_up_block_devices(
    spec: &mut oci_spec::runtime::Spec,
) -> Result<Vec<BlockDeviceMount>, Box<dyn Error>> {
    let mut block_devices: Vec<BlockDeviceMount> = vec![];

    // set up block devices passed in using --device (note that rootless podman will turn those into
    // --mount/--volume instead)

    for device in spec.linux_devices() {
        if device.typ() != oci_spec::runtime::LinuxDeviceType::B {
            continue;
        }

        let major: u64 = device.major().try_into().unwrap();
        let minor: u64 = device.minor().try_into().unwrap();

        let path_in_container = PathBuf::from(format!("crun-qemu/bdevs/{}:{}", major, minor));
        let path_in_guest = device.path().clone();

        fs::create_dir_all(spec.root_path().join(&path_in_container).parent().unwrap())?;

        mknod(
            &spec.root_path().join(&path_in_container),
            SFlag::S_IFBLK,
            Mode::from_bits_retain(device.file_mode().unwrap()),
            makedev(major, minor),
        )?;

        block_devices.push(BlockDeviceMount {
            path_in_container,
            path_in_guest,
        });
    }

    // set up block devices passed in using --mount/--volume

    let mut mounts = spec.mounts().clone().unwrap_or_default();

    for mount in &mut mounts {
        if mount.typ().as_deref() != Some("bind") {
            continue;
        }

        let source = match mount.source() {
            Some(source) => source,
            None => continue,
        };

        let meta = source.metadata()?;

        if !meta.file_type().is_block_device() {
            continue;
        }

        let major = major(meta.rdev());
        let minor = minor(meta.rdev());

        let path_in_container = PathBuf::from(format!("crun-qemu/bdevs/{}:{}", major, minor));
        let path_in_guest = mount.destination().clone();

        // redirect the mount to a path that we control in container
        mount.set_destination(path_in_container.clone());

        // with Docker and rootful Podman, we must add devices that are passed in as bind mounts
        // to .linux.resources.devices for the container to actually be able to access them
        spec.linux_resources_devices_push(
            oci_spec::runtime::LinuxDeviceCgroupBuilder::default()
                .allow(true)
                .typ(oci_spec::runtime::LinuxDeviceType::B)
                .major(i64::try_from(major).unwrap())
                .minor(i64::try_from(minor).unwrap())
                .access("rwm")
                .build()
                .unwrap(),
        );

        block_devices.push(BlockDeviceMount {
            path_in_container,
            path_in_guest,
        });
    }

    spec.set_mounts(Some(mounts));

    Ok(block_devices)
}

fn set_up_char_devices(spec: &mut oci_spec::runtime::Spec) -> Result<(), Box<dyn Error>> {
    fn set_up(
        spec: &mut oci_spec::runtime::Spec,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn Error>> {
        let rdev = fs::metadata(path.as_ref())?.rdev();

        spec.mounts_push(
            oci_spec::runtime::MountBuilder::default()
                .typ("bind")
                .source(path.as_ref())
                .destination(path.as_ref())
                .options(["bind".to_string(), "rprivate".to_string(), "ro".to_string()])
                .build()
                .unwrap(),
        );

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

    set_up(spec, "/dev/kvm")?;

    for entry in fs::read_dir("/dev/vfio")? {
        let entry = entry?;
        if entry.metadata()?.file_type().is_char_device() {
            set_up(spec, entry.path())?;
        }
    }

    Ok(())
}

fn set_up_extra_container_mounts(spec: &mut oci_spec::runtime::Spec) -> Result<(), Box<dyn Error>> {
    const PATHS_TO_BIND_MOUNT: &[&str] =
        &["/bin", "/dev/log", "/etc/pam.d", "/lib", "/lib64", "/usr"];

    for path in PATHS_TO_BIND_MOUNT {
        spec.mounts_push(
            oci_spec::runtime::MountBuilder::default()
                .typ("bind")
                .source(path)
                .destination(path)
                .options(["bind".to_string(), "rprivate".to_string(), "ro".to_string()])
                .build()
                .unwrap(),
        );
    }

    fs::create_dir_all(spec.root_path().join("etc"))?;
    fs::copy("/etc/passwd", spec.root_path().join("etc/passwd"))?;
    fs::copy("/etc/group", spec.root_path().join("etc/group"))?;

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
) -> Result<(), Box<dyn Error>> {
    let container_public_key = generate_container_ssh_key_pair(spec)?;

    let config = FirstBootConfig {
        hostname: spec.hostname().as_deref(),
        container_public_key: &container_public_key,
        mounts,
    };

    config.apply_to_cloud_init_config(
        custom_options.cloud_init.as_ref(),
        spec.root_path().join("crun-qemu/cloud-init"),
    )?;

    config.apply_to_ignition_config(
        custom_options.ignition.as_ref(),
        spec.root_path().join("crun-qemu/ignition.ign"),
    )?;

    Ok(())
}

/// Returns the public key.
fn generate_container_ssh_key_pair(
    spec: &oci_spec::runtime::Spec,
) -> Result<String, Box<dyn Error>> {
    fs::create_dir_all(spec.root_path().join("root/.ssh"))?;

    let status = Command::new("ssh-keygen")
        .arg("-q")
        .arg("-f")
        .arg(spec.root_path().join("root/.ssh/id_rsa"))
        .arg("-N")
        .arg("")
        .spawn()?
        .wait()?;

    if !status.success() {
        return Err(Box::new(io::Error::other("ssh-keygen failed")));
    }

    let public_key = fs::read_to_string(spec.root_path().join("root/.ssh/id_rsa.pub"))?;

    Ok(public_key)
}
