// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::{c_char, CString};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nix::mount::MsFlags;
use serde::Deserialize;
use tempfile::TempDir;

pub fn set_file_context(path: impl AsRef<Path>, context: &str) -> io::Result<()> {
    extern "C" {
        fn setfilecon(path: *const c_char, con: *const c_char) -> i32;
    }

    let path = CString::new(path.as_ref().as_os_str().as_bytes())?;
    let context = CString::new(context.as_bytes())?;

    if unsafe { setfilecon(path.as_ptr(), context.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

/// Expose directory `from` at `to` with the given SELinux `context` recursively applied.
///
/// This does *not* modify the SELinux context of `from` nor of files under `from`.
///
/// TODO: Is this a neat relabeling trick or simply a bad hack?
pub fn link_directory_with_separate_context(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    context: &str,
) -> io::Result<()> {
    let temp_dir = TempDir::new()?;

    let upper_dir = temp_dir.path().join("upper");
    let work_dir = temp_dir.path().join("work");

    fs::create_dir(&upper_dir)?;
    fs::create_dir(&work_dir)?;
    fs::create_dir_all(to.as_ref())?;

    // TODO: Harden quoting.
    // TODO: Podman probably won't umount this for us.
    nix::mount::mount(
        Some("overlay"),
        to.as_ref(),
        Some("overlay"),
        MsFlags::empty(),
        Some(
            format!(
                "lowerdir={},upperdir={},workdir={},context=\"{}\"",
                from.as_ref().to_str().unwrap(),
                upper_dir.to_str().unwrap(),
                work_dir.to_str().unwrap(),
                context,
            )
            .as_str(),
        ),
    )?;

    // TODO: Clean up the temporary directory.
    let _ = temp_dir.into_path();

    Ok(())
}

pub trait SpecExt {
    fn root_path(&self) -> &PathBuf;
    fn mount_label(&self) -> Option<&str>;
    fn linux_devices(&self) -> &[oci_spec::runtime::LinuxDevice];

    fn mounts_push(&mut self, mount: oci_spec::runtime::Mount);
    fn linux_resources_devices_push(
        &mut self,
        linux_device_cgroup: oci_spec::runtime::LinuxDeviceCgroup,
    );
    fn linux_seccomp_syscalls_push(&mut self, linux_syscall: oci_spec::runtime::LinuxSyscall);
}

impl SpecExt for oci_spec::runtime::Spec {
    fn root_path(&self) -> &PathBuf {
        self.root().as_ref().unwrap().path()
    }

    fn mount_label(&self) -> Option<&str> {
        self.linux().as_ref()?.mount_label().as_deref()
    }

    fn linux_devices(&self) -> &[oci_spec::runtime::LinuxDevice] {
        let linux = match self.linux().as_ref() {
            Some(linux) => linux,
            None => return &[],
        };

        let devices = match linux.devices() {
            Some(devices) => devices,
            None => return &[],
        };

        devices.as_slice()
    }

    fn mounts_push(&mut self, mount: oci_spec::runtime::Mount) {
        let mut mounts = self.mounts().clone().unwrap_or_default();
        mounts.push(mount);
        self.set_mounts(Some(mounts));
    }

    fn linux_resources_devices_push(
        &mut self,
        linux_device_cgroup: oci_spec::runtime::LinuxDeviceCgroup,
    ) {
        self.set_linux({
            let mut linux = self.linux().clone().expect("linux config");
            linux.set_resources({
                let mut resources = linux.resources().clone().unwrap_or_default();
                resources.set_devices({
                    let mut devices = resources.devices().clone().unwrap_or_default();
                    devices.push(linux_device_cgroup);
                    Some(devices)
                });
                Some(resources)
            });
            Some(linux)
        });
    }

    fn linux_seccomp_syscalls_push(&mut self, linux_syscall: oci_spec::runtime::LinuxSyscall) {
        self.set_linux({
            let mut linux = self.linux().clone().expect("linux config");
            linux.set_seccomp({
                let mut seccomp = linux.seccomp().clone();
                if let Some(seccomp) = &mut seccomp {
                    seccomp.set_syscalls({
                        let mut syscalls = seccomp.syscalls().clone().unwrap_or_default();
                        syscalls.push(linux_syscall);
                        Some(syscalls)
                    });
                }
                seccomp
            });
            Some(linux)
        });
    }
}

pub fn find_single_file_in_dirs(
    dir_paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ignore_files: &[impl AsRef<Path>],
) -> io::Result<PathBuf> {
    let mut candidate: Option<PathBuf> = None;

    for dir_path in dir_paths {
        let dir_path = dir_path.as_ref();

        if dir_path.is_dir() {
            for entry in dir_path.read_dir()? {
                let e = entry?;

                if !e.file_type()?.is_file() {
                    continue; // we only care about regular files
                }

                let path = e.path();

                if ignore_files.iter().any(|f| path == f.as_ref()) {
                    continue; // file is in `ignore_files`
                }

                if candidate.is_some() {
                    return Err(io::Error::other("more than one file found"));
                } else {
                    candidate = Some(path);
                }
            }
        }
    }

    if let Some(path) = candidate {
        Ok(path)
    } else {
        Err(io::Error::other("no files found"))
    }
}

#[derive(Deserialize)]
pub struct VmImageInfo {
    #[serde(skip)]
    pub path: PathBuf,

    #[serde(rename = "virtual-size")]
    pub size: u64,

    pub format: String,
}

impl VmImageInfo {
    pub fn of(vm_image_path: impl AsRef<Path>) -> io::Result<VmImageInfo> {
        let vm_image_path = vm_image_path.as_ref().to_path_buf();

        let output = Command::new("qemu-img")
            .arg("info")
            .arg("--output=json")
            .arg(vm_image_path.as_os_str())
            .stdout(Stdio::piped())
            .output()?;

        if !output.status.success() {
            return Err(io::Error::other("qemu-img failed"));
        }

        let mut info: VmImageInfo = serde_json::from_slice(&output.stdout)?;
        info.path = vm_image_path;

        Ok(info)
    }
}

pub fn create_overlay_vm_image(
    overlay_vm_image_path: &Path,
    base_vm_image_info: &VmImageInfo,
) -> io::Result<()> {
    let status = Command::new("qemu-img")
        .arg("create")
        .arg("-q")
        .arg("-f")
        .arg("qcow2")
        .arg("-u")
        .arg("-F")
        .arg(&base_vm_image_info.format)
        .arg("-b")
        .arg(&base_vm_image_info.path)
        .arg(overlay_vm_image_path)
        .arg(base_vm_image_info.size.to_string())
        .spawn()?
        .wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("qemu-img failed"))
    }
}
