// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::{c_char, CString};
use std::fs::{self, OpenOptions, Permissions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Result};
use nix::mount::MsFlags;
use serde::Deserialize;

pub trait PathExt {
    fn as_str(&self) -> &str;

    fn as_string(&self) -> String {
        self.as_str().to_string()
    }
}

impl<P: AsRef<Path>> PathExt for P {
    fn as_str(&self) -> &str {
        self.as_ref().to_str().expect("path is utf-8")
    }
}

pub fn set_file_context(path: impl AsRef<Path>, context: &str) -> Result<()> {
    extern "C" {
        fn setfilecon(path: *const c_char, con: *const c_char) -> i32;
    }

    let path = CString::new(path.as_ref().as_os_str().as_bytes())?;
    let context = CString::new(context.as_bytes())?;

    if unsafe { setfilecon(path.as_ptr(), context.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }

    Ok(())
}

pub fn bind_mount_file(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    // ensure target exists

    if let Some(parent) = to.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }

    OpenOptions::new()
        .create(true)
        .append(true)
        .open(to.as_ref())?;

    // bind mount file

    if let Err(e) = nix::mount::mount(
        Some(from.as_ref()),
        to.as_ref(),
        Option::<&str>::None,
        MsFlags::MS_BIND,
        Option::<&str>::None,
    ) {
        bail!(
            "mount({:?}, {:?}, NULL, MS_BIND, NULL) failed: {}",
            from.as_str(),
            to.as_str(),
            e
        );
    }

    Ok(())
}

/// Expose directory `from` at `to` with the given SELinux `context`, if any, recursively applied.
///
/// This does *not* modify the SELinux context of `from` nor of files under `from`.
///
/// If `propagate_changes` is true, `private_dir` must belong to the same file system as `from` and
/// be a separate subtree.
///
/// TODO: Is this a neat relabeling trick or simply a bad hack?
pub fn bind_mount_dir_with_different_context(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    context: Option<&str>,
    propagate_changes: bool,
    private_dir: impl AsRef<Path>,
) -> Result<()> {
    let layer_dir = private_dir.as_ref().join("layer");
    let work_dir = private_dir.as_ref().join("work");

    fs::create_dir_all(&layer_dir)?;
    fs::create_dir_all(&work_dir)?;
    fs::create_dir_all(to.as_ref())?;

    fn escape_path(mount_option: &str) -> String {
        mount_option.replace('\\', "\\\\").replace(',', "\\,")
    }

    fn escape_context(mount_option: &str) -> String {
        assert!(!mount_option.contains('"'));
        format!("\"{}\"", mount_option)
    }

    let (lower_dir, upper_dir) = match propagate_changes {
        true => (layer_dir.as_path(), from.as_ref()),
        false => (from.as_ref(), layer_dir.as_path()),
    };

    let mut options = format!(
        "lowerdir={},upperdir={},workdir={}",
        escape_path(lower_dir.as_str()),
        escape_path(upper_dir.as_str()),
        escape_path(work_dir.as_str()),
    );

    if let Some(context) = context {
        options = format!("{},context={}", options, escape_context(context));
    }

    if let Err(e) = nix::mount::mount(
        Some("overlay"),
        to.as_ref(),
        Some("overlay"),
        MsFlags::empty(),
        Some(options.as_str()),
    ) {
        bail!(
            "mount(\"overlay\", {:?}, \"overlay\", 0, {:?}) failed: {}",
            to.as_str(),
            options,
            e,
        );
    }

    // Make any necessary manual cleanup a bit easier by ensuring the workdir is accessible to the
    // user that Podman is running under.
    fs::set_permissions(work_dir.join("work"), Permissions::from_mode(0o700))?;

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
    fn process_capabilities_insert_beip(&mut self, capability: oci_spec::runtime::Capability);
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

    fn process_capabilities_insert_beip(&mut self, capability: oci_spec::runtime::Capability) {
        self.set_process({
            let mut process = self.process().clone().expect("process config");
            process.set_capabilities({
                let mut capabilities = process.capabilities().clone().unwrap_or_default();

                fn insert(
                    cap: oci_spec::runtime::Capability,
                    to: &Option<oci_spec::runtime::Capabilities>,
                ) -> Option<oci_spec::runtime::Capabilities> {
                    let mut caps = to.clone().unwrap_or_default();
                    caps.insert(cap);
                    Some(caps)
                }

                capabilities.set_bounding(insert(capability, capabilities.bounding()));
                capabilities.set_effective(insert(capability, capabilities.effective()));
                capabilities.set_inheritable(insert(capability, capabilities.inheritable()));
                capabilities.set_permitted(insert(capability, capabilities.permitted()));

                Some(capabilities)
            });
            Some(process)
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
) -> Result<PathBuf> {
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
                    bail!("more than one file found");
                } else {
                    candidate = Some(path);
                }
            }
        }
    }

    if let Some(path) = candidate {
        Ok(path)
    } else {
        bail!("no files found");
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
    pub fn of(vm_image_path: impl AsRef<Path>) -> Result<VmImageInfo> {
        let vm_image_path = vm_image_path.as_ref().to_path_buf();

        let output = Command::new("qemu-img")
            .arg("info")
            .arg("--output=json")
            .arg(vm_image_path.as_os_str())
            .stdout(Stdio::piped())
            .output()?;

        if !output.status.success() {
            bail!("qemu-img failed");
        }

        let mut info: VmImageInfo = serde_json::from_slice(&output.stdout)?;
        info.path = vm_image_path;

        Ok(info)
    }
}

pub fn create_overlay_vm_image(
    overlay_vm_image_path: &Path,
    base_vm_image_info: &VmImageInfo,
) -> Result<()> {
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
        bail!("qemu-img failed");
    }
}
