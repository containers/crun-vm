// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::{c_char, CString, OsStr};
use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, ensure, Result};
use camino::{Utf8Path, Utf8PathBuf};
use nix::mount::{MntFlags, MsFlags};
use serde::Deserialize;

pub fn set_file_context(path: impl AsRef<Utf8Path>, context: &str) -> Result<()> {
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

pub fn is_mountpoint(path: impl AsRef<Utf8Path>) -> Result<bool> {
    let parent = path
        .as_ref()
        .parent()
        .ok_or_else(|| anyhow!("path does not have a parent"))?;

    let path_dev = match fs::symlink_metadata(path.as_ref()) {
        Ok(meta) => meta.dev(),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };

    let parent_dev = fs::symlink_metadata(parent)?.dev();

    Ok(path_dev != parent_dev)
}

pub fn bind_mount_file(from: impl AsRef<Utf8Path>, to: impl AsRef<Utf8Path>) -> Result<()> {
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
        Some(from.as_ref().as_std_path()),
        to.as_ref().as_std_path(),
        Option::<&str>::None,
        MsFlags::MS_BIND,
        Option::<&str>::None,
    ) {
        bail!(
            "mount({:?}, {:?}, NULL, MS_BIND, NULL) failed: {}",
            from.as_ref(),
            to.as_ref(),
            e
        );
    }

    Ok(())
}

fn escape_path(mount_option: impl AsRef<Utf8Path>) -> String {
    mount_option
        .as_ref()
        .as_str()
        .replace('\\', "\\\\")
        .replace(',', "\\,")
}

fn escape_context(mount_option: &str) -> String {
    assert!(!mount_option.contains('"'));
    format!("\"{}\"", mount_option)
}

/// Expose directory `from` at `to` with the given SELinux `context`, if any, recursively applied.
///
/// This does *not* modify the SELinux context of `from` nor of files under `from`.
///
/// If `read_only` is false, `scratch_dir` must belong to the same file system as `from` and be a
/// separate subtree.
///
/// TODO: Is this a neat relabeling trick or simply a bad hack?
pub fn bind_mount_dir_with_different_context(
    from: impl AsRef<Utf8Path>,
    to: impl AsRef<Utf8Path>,
    scratch_dir: impl AsRef<Utf8Path>,
    context: Option<&str>,
    read_only: bool,
) -> Result<()> {
    fs::create_dir_all(to.as_ref())?;

    let mut options = if read_only {
        fs::create_dir_all(scratch_dir.as_ref())?;

        format!(
            "lowerdir={}:{}",
            escape_path(scratch_dir.as_ref()),
            escape_path(from)
        )
    } else {
        let layer_dir = scratch_dir.as_ref().join("layer");
        let work_dir = scratch_dir.as_ref().join("work");

        fs::create_dir_all(&layer_dir)?;
        fs::create_dir_all(&work_dir)?;

        format!(
            "lowerdir={},upperdir={},workdir={}",
            escape_path(layer_dir),
            escape_path(from),
            escape_path(&work_dir),
        )
    };

    if let Some(context) = context {
        options = format!("{},context={}", options, escape_context(context));
    }

    if let Err(e) = nix::mount::mount(
        Some("overlay"),
        to.as_ref().as_std_path(),
        Some("overlay"),
        MsFlags::empty(),
        Some(options.as_str()),
    ) {
        bail!(
            "mount(\"overlay\", {:?}, \"overlay\", 0, {:?}) failed: {}",
            to.as_ref(),
            options,
            e,
        );
    }

    if !read_only {
        // Make any necessary manual cleanup a bit easier by ensuring the workdir is accessible to
        // the user that Podman is running under.
        fs::set_permissions(
            scratch_dir.as_ref().join("work/work"),
            Permissions::from_mode(0o700),
        )?;
    }

    Ok(())
}

pub fn ensure_unmounted(path: impl AsRef<Utf8Path>) -> Result<()> {
    while is_mountpoint(&path)? {
        nix::mount::umount2(path.as_ref().as_std_path(), MntFlags::MNT_DETACH)?;
    }

    Ok(())
}

pub trait SpecExt {
    fn root_path(&self) -> Result<&Utf8Path>;
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
    fn root_path(&self) -> Result<&Utf8Path> {
        let path = self.root().as_ref().unwrap().path().as_path().try_into()?;
        Ok(path)
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
    dir_paths: impl IntoIterator<Item = impl AsRef<Utf8Path>>,
    ignore_files: &[impl AsRef<Utf8Path>],
) -> Result<Utf8PathBuf> {
    let mut candidate: Option<Utf8PathBuf> = None;

    for dir_path in dir_paths {
        let dir_path = dir_path.as_ref();

        if dir_path.is_dir() {
            for entry in dir_path.read_dir()? {
                let e = entry?;

                if !e.file_type()?.is_file() {
                    continue; // we only care about regular files
                }

                let path: Utf8PathBuf = e.path().try_into()?;

                if ignore_files.iter().any(|f| path == f.as_ref()) {
                    continue; // file is in `ignore_files`
                }

                ensure!(candidate.is_none(), "more than one file found");

                candidate = Some(path);
            }
        }
    }

    candidate.ok_or_else(|| anyhow!("no files found"))
}

#[derive(Deserialize)]
pub struct VmImageInfo {
    #[serde(skip)]
    pub path: Utf8PathBuf,

    #[serde(rename = "virtual-size")]
    pub size: u64,

    pub format: String,
}

impl VmImageInfo {
    pub fn of(vm_image_path: impl AsRef<Utf8Path>) -> Result<VmImageInfo> {
        let vm_image_path = vm_image_path.as_ref().to_path_buf();

        let output = Command::new("qemu-img")
            .arg("info")
            .arg("--output=json")
            .arg(vm_image_path.as_os_str())
            .stdout(Stdio::piped())
            .output()?;

        ensure!(
            output.status.success(),
            "`qemu-img info` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let mut info: VmImageInfo = serde_json::from_slice(&output.stdout)?;
        info.path = vm_image_path;

        Ok(info)
    }
}

pub fn create_overlay_vm_image(
    overlay_vm_image_path: &Utf8Path,
    base_vm_image_info: &VmImageInfo,
) -> Result<()> {
    let output = Command::new("qemu-img")
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
        .output()?;

    ensure!(
        output.status.success(),
        "`qemu-img create` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

/// Run `crun`.
///
/// `crun` will inherit this process' standard streams.
pub fn crun(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<()> {
    let status = Command::new("crun").args(args).spawn()?.wait()?;
    ensure!(status.success(), "crun failed");

    Ok(())
}
