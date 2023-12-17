// SPDX-License-Identifier: GPL-2.0-or-later

use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

pub fn find_single_file_in_directories<I, P>(dir_paths: I) -> io::Result<PathBuf>
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
pub struct ImageInfo {
    #[serde(rename = "virtual-size")]
    pub size: u64,

    pub format: String,
}

pub fn get_image_info(image_path: impl AsRef<Path>) -> io::Result<ImageInfo> {
    let output = Command::new("qemu-img")
        .arg("info")
        .arg("--output=json")
        .arg(image_path.as_ref().as_os_str())
        .stdout(Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other("qemu-img failed"));
    }

    Ok(serde_json::from_slice(&output.stdout)?)
}

pub fn create_overlay_image(
    overlay_image_path: impl AsRef<Path>,
    backing_image_path: impl AsRef<Path>,
    backing_image_info: &ImageInfo,
) -> io::Result<()> {
    let status = Command::new("qemu-img")
        .arg("create")
        .arg("-q")
        .arg("-f")
        .arg("qcow2")
        .arg("-u")
        .arg("-F")
        .arg(&backing_image_info.format)
        .arg("-b")
        .arg(backing_image_path.as_ref())
        .arg(overlay_image_path.as_ref())
        .arg(backing_image_info.size.to_string())
        .spawn()?
        .wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("qemu-img failed"))
    }
}

/// Run `crun`.
///
/// `crun` will inherit this process' standard streams.
///
/// TODO: It may be better to use libcrun directly, although its public API purportedly isn't in
/// great shape: https://github.com/containers/crun/issues/1018
pub fn crun<I, S>(args: I) -> io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new("crun").args(args).spawn()?.wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("crun failed"))
    }
}

/// Returns `true` if a cloud-init config should be passed to the VM.
pub fn generate_cloud_init_iso(
    source_config_path: Option<impl AsRef<Path>>,
    runner_root: impl AsRef<Path>,
    virtiofs_mounts: impl IntoIterator<Item = impl AsRef<str>>,
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

    let mounts = match &mut user_data {
        serde_yaml::Value::Mapping(m) => {
            if !m.contains_key("mounts") {
                m.insert("mounts".into(), serde_yaml::Value::Sequence(vec![]));
            }

            match m.get_mut("mounts").unwrap() {
                serde_yaml::Value::Sequence(mounts) => mounts,
                _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
            }
        }
        _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
    };

    for mount in virtiofs_mounts {
        let mount = mount.as_ref();
        mounts.push(vec![mount, mount, "virtiofs", "defaults", "0", "0"].into());
    }

    {
        let mut f = File::create(user_data_path)?;
        f.write_all(b"#cloud-config\n")?;
        serde_yaml::to_writer(&mut f, &user_data)?;
    }

    // generate iso

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
