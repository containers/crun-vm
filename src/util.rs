// SPDX-License-Identifier: GPL-2.0-or-later

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

pub fn find_single_file_in_dirs<I, P>(dir_paths: I) -> io::Result<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut candidate = None;

    for dir_path in dir_paths {
        let dir_path = dir_path.as_ref();

        if dir_path.is_dir() {
            for entry in dir_path.read_dir()? {
                let e = entry?;

                if e.file_type()?.is_file() {
                    if candidate.is_some() {
                        return Err(io::Error::other("more than one file found"));
                    } else {
                        candidate = Some(e.path());
                    }
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
    #[serde(rename = "virtual-size")]
    pub size: u64,
    pub format: String,
}

impl VmImageInfo {
    pub fn of(vm_image_path: impl AsRef<Path>) -> io::Result<VmImageInfo> {
        let output = Command::new("qemu-img")
            .arg("info")
            .arg("--output=json")
            .arg(vm_image_path.as_ref().as_os_str())
            .stdout(Stdio::piped())
            .output()?;

        if !output.status.success() {
            return Err(io::Error::other("qemu-img failed"));
        }

        Ok(serde_json::from_slice(&output.stdout)?)
    }
}

pub fn create_overlay_vm_image(
    overlay_vm_image_path: impl AsRef<Path>,
    backing_vm_image_path: impl AsRef<Path>,
    backing_vm_image_info: &VmImageInfo,
) -> io::Result<()> {
    let status = Command::new("qemu-img")
        .arg("create")
        .arg("-q")
        .arg("-f")
        .arg("qcow2")
        .arg("-u")
        .arg("-F")
        .arg(&backing_vm_image_info.format)
        .arg("-b")
        .arg(backing_vm_image_path.as_ref())
        .arg(overlay_vm_image_path.as_ref())
        .arg(backing_vm_image_info.size.to_string())
        .spawn()?
        .wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("qemu-img failed"))
    }
}
