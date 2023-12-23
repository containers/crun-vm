// SPDX-License-Identifier: GPL-2.0-or-later

mod domain;
mod first_boot;

use std::error::Error;
use std::fs::{self, File, Permissions};
use std::io;
use std::iter;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use nix::sys::stat::{major, makedev, minor, mknod, Mode, SFlag};

use crate::commands::create::domain::set_up_libvirt_domain_xml;
use crate::commands::create::first_boot::FirstBootConfig;
use crate::crun::crun_create;
use crate::util::{
    create_overlay_vm_image, find_single_file_in_dirs, link_directory_with_separate_context,
    set_file_context, SpecExt, VmImageInfo,
};

pub fn create(
    global_args: &liboci_cli::GlobalOpts,
    args: &liboci_cli::Create,
) -> Result<(), Box<dyn Error>> {
    let config_path = args
        .bundle
        .join("config.json")
        .to_str()
        .unwrap()
        .to_string();

    let mut spec = oci_spec::runtime::Spec::load(&config_path)?;
    let original_root_path = spec.root_path().clone();

    let is_docker = original_root_path.join(".dockerenv").exists();
    let custom_options = CustomOptions::from_spec(&spec, is_docker)?;

    set_up_container_root(&mut spec, &args.bundle)?;
    let base_vm_image_info = set_up_vm_image(&spec, &args.bundle, &original_root_path)?;

    let virtiofs_mounts = set_up_directory_bind_mounts(&mut spec)?;
    let block_devices = set_up_block_devices(&mut spec)?;
    set_up_char_devices(&mut spec)?;

    set_up_directories_and_files_from_host(&mut spec)?;

    set_up_seccomp_profile(&mut spec, is_docker);
    set_up_passt_wrapper(&mut spec)?;

    spec.save(&config_path)?;
    spec.save(spec.root_path().join("crun-qemu/config.json"))?; // to aid debugging

    set_up_first_boot_config(
        &spec,
        &custom_options,
        spec.hostname().as_deref(),
        &block_devices,
        &virtiofs_mounts,
    )?;

    set_up_libvirt_domain_xml(
        &spec,
        &base_vm_image_info,
        &block_devices,
        &virtiofs_mounts,
        &custom_options,
    )?;

    crun_create(global_args, args)?; // actually create container

    Ok(())
}

#[derive(clap::Parser, Debug)]
struct CustomOptions {
    #[clap(long)]
    cloud_init: Option<PathBuf>,

    #[clap(long)]
    ignition: Option<PathBuf>,

    #[clap(long)]
    vfio_pci_mdev: Vec<PathBuf>,
}

impl CustomOptions {
    pub fn from_spec(spec: &oci_spec::runtime::Spec, is_docker: bool) -> io::Result<Self> {
        let args = spec
            .process()
            .as_ref()
            .unwrap()
            .args()
            .iter()
            .flatten()
            .filter(|arg| !arg.trim().is_empty());

        // TODO: We currently assume that no entrypoint is given (either by being set by in the
        // container image or through --entrypoint). Must somehow find whether the first arg is the
        // entrypoint and ignore it in that case.
        let options =
            Self::parse_from(iter::once(&"podman run ... <image>".to_string()).chain(args));

        if is_docker {
            // Unlike Podman, Docker doesn't run the runtime with the same working directory as the
            // process that ran `docker`, so we require these paths to be absolute.
            //
            // TODO: There must be a better way...

            fn any_is_relative(iter: impl IntoIterator<Item = impl AsRef<Path>>) -> bool {
                iter.into_iter().any(|p| p.as_ref().is_relative())
            }

            if any_is_relative(&options.cloud_init)
                || any_is_relative(&options.ignition)
                || any_is_relative(&options.vfio_pci_mdev)
            {
                return Err(io::Error::other(concat!(
                    "paths specified using --cloud-init, --ignition, or --vfio-pci-mdev must be",
                    " absolute when using Docker",
                )));
            }
        }

        Ok(options)
    }
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
) -> Result<VmImageInfo, Box<dyn Error>> {
    // where inside the container to look for the VM image
    const VM_IMAGE_SEARCH_PATHS: [&str; 2] = ["./", "disk/"];

    // docker may add these files to the root of the container
    const FILES_TO_IGNORE: [&str; 2] = [".dockerinit", ".dockerenv"];

    let base_vm_image_path_in_host = find_single_file_in_dirs(
        VM_IMAGE_SEARCH_PATHS.map(|p| original_root_path.join(p)),
        &FILES_TO_IGNORE.map(|f| original_root_path.join(f)),
    )?;

    // mount user-provided VM image file into container with the appropriate SELinux context

    if let Some(context) = spec.mount_label() {
        link_directory_with_separate_context(
            base_vm_image_path_in_host.parent().unwrap(),
            spec.root_path().join("crun-qemu/image"),
            context,
            bundle_path.join("crun-qemu-vm-image-overlayfs"),
        )?;
    } else {
        todo!("TODO: probably just add a mount to the spec")
    }

    // create overlay image

    let overlay_vm_image_path_in_host = spec.root_path().join("crun-qemu/image-overlay.qcow2");

    let base_vm_image_path_in_container =
        Path::new("/crun-qemu/image").join(base_vm_image_path_in_host.file_name().unwrap());

    let mut base_vm_image_info = VmImageInfo::of(&base_vm_image_path_in_host)?;
    base_vm_image_info.path = base_vm_image_path_in_container;

    create_overlay_vm_image(&overlay_vm_image_path_in_host, &base_vm_image_info)?;

    Ok(base_vm_image_info)
}

struct VirtiofsMount {
    path_in_container: PathBuf,
    path_in_guest: PathBuf,
}

fn set_up_directory_bind_mounts(
    spec: &mut oci_spec::runtime::Spec,
) -> Result<Vec<VirtiofsMount>, Box<dyn Error>> {
    const TARGETS_TO_IGNORE: &[&str] = &[
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

    let mut directory_bind_mounts: Vec<VirtiofsMount> = vec![];
    let mut mounts = spec.mounts().clone().unwrap_or_default();

    for (i, mount) in mounts.iter_mut().enumerate() {
        if mount.typ().as_deref() != Some("bind")
            || TARGETS_TO_IGNORE.contains(&mount.destination().to_str().unwrap_or_default())
            || mount.destination().starts_with("/dev/")
        {
            continue;
        }

        let meta = match mount.source() {
            Some(source) => source.metadata()?,
            None => continue,
        };

        if !meta.file_type().is_dir() {
            continue;
        }

        let path_in_container = PathBuf::from(format!("/crun-qemu/dir-bind-mounts/{}", i));
        let path_in_guest = mount.destination().clone();

        // redirect the mount to a path that we control in container
        mount.set_destination(path_in_container.clone());

        directory_bind_mounts.push(VirtiofsMount {
            path_in_container,
            path_in_guest,
        });
    }

    spec.set_mounts(Some(mounts));

    Ok(directory_bind_mounts)
}

struct BlockDevice {
    path_in_container: PathBuf,
    path_in_guest: PathBuf,
}

fn set_up_block_devices(
    spec: &mut oci_spec::runtime::Spec,
) -> Result<Vec<BlockDevice>, Box<dyn Error>> {
    let mut block_devices: Vec<BlockDevice> = vec![];

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

        block_devices.push(BlockDevice {
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

        block_devices.push(BlockDevice {
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

fn set_up_directories_and_files_from_host(
    spec: &mut oci_spec::runtime::Spec,
) -> Result<(), Box<dyn Error>> {
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

fn set_up_seccomp_profile(spec: &mut oci_spec::runtime::Spec, is_docker: bool) {
    if is_docker {
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
}

fn set_up_passt_wrapper(spec: &mut oci_spec::runtime::Spec) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(spec.root_path().join("crun-qemu/passt"))?;

    fs::copy(
        "/usr/bin/passt",
        spec.root_path().join("crun-qemu/passt/passt"),
    )?;

    File::create(spec.root_path().join("crun-qemu/passt/wrapper"))?;

    spec.mounts_push(
        oci_spec::runtime::MountBuilder::default()
            .typ("bind")
            .source(spec.root_path().join("crun-qemu/passt/wrapper"))
            .destination("usr/bin/passt")
            .options(["bind".to_string(), "rprivate".to_string()])
            .build()
            .unwrap(),
    );

    Ok(())
}

/// Configure cloud-init and Ignition for first-boot customization.
fn set_up_first_boot_config(
    spec: &oci_spec::runtime::Spec,
    custom_options: &CustomOptions,
    hostname: Option<&str>,
    block_devices: &[BlockDevice],
    virtiofs_mounts: &[VirtiofsMount],
) -> Result<(), Box<dyn Error>> {
    let container_public_key = generate_container_ssh_key_pair(spec)?;

    let config = FirstBootConfig {
        hostname,
        container_public_key: &container_public_key,
        block_devices,
        virtiofs_mounts,
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
