// SPDX-License-Identifier: GPL-2.0-or-later

use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use crate::commands::create::Mounts;

pub struct FirstBootConfig<'a> {
    pub hostname: Option<&'a str>,
    pub container_public_key: &'a str,
    pub mounts: &'a Mounts,
}

impl FirstBootConfig<'_> {
    /// Returns `true` if a cloud-init config should be passed to the VM.
    pub fn apply_to_cloud_init_config(
        &self,
        in_config_dir_path: Option<impl AsRef<Path>>,
        out_config_dir_path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(&out_config_dir_path)?;

        // create copy of config

        for file in ["meta-data", "user-data", "vendor-data"] {
            let path = out_config_dir_path.as_ref().join(file);

            if let Some(user_config_path) = &in_config_dir_path {
                let user_path = user_config_path.as_ref().join(file);
                if user_path.exists() {
                    // TODO: Potential security vulnerability, symlink may point to somewhere on
                    // host that user isn't normally able to access, especially when running as a
                    // Kubernetes runtime.
                    if !user_path.metadata()?.is_file() {
                        return Err(io::Error::other(format!(
                            "cloud-init: expected {file} to be a regular file"
                        ))
                        .into());
                    }

                    fs::copy(user_path, &path)?;
                    continue;
                }
            }

            let mut f = File::create(path)?;
            if file == "user-data" {
                f.write_all(b"#cloud-config\n")?;
            }
        }

        // adjust user-data config

        let user_data_path = out_config_dir_path.as_ref().join("user-data");
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

        if !self.mounts.virtiofs.is_empty() || !self.mounts.tmpfs.is_empty() {
            let mounts: &mut Vec<serde_yaml::Value> = match user_data_mapping
                .entry("mounts".into())
                .or_insert_with(|| serde_yaml::Value::Sequence(vec![]))
            {
                serde_yaml::Value::Sequence(mounts) => mounts,
                _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
            };

            let mut add_mount = |typ: &str, tag: &str, path_in_guest: &Path| {
                let path_in_guest = path_in_guest.to_str().unwrap();
                mounts.push(vec![&tag, path_in_guest, typ, "defaults", "0", "0"].into());
            };

            for (i, mount) in self.mounts.virtiofs.iter().enumerate() {
                add_mount("virtiofs", &format!("virtiofs-{i}"), &mount.path_in_guest);
            }

            for mount in &self.mounts.tmpfs {
                add_mount("tmpfs", "tmpfs", &mount.path_in_guest);
            }
        }

        // adjust hostname

        if let Some(hostname) = self.hostname {
            user_data_mapping.insert("preserve_hostname".into(), false.into());
            user_data_mapping.insert("prefer_fqdn_over_hostname".into(), false.into());
            user_data_mapping.insert("hostname".into(), hostname.into());
        }

        // adjust authorized keys

        let ssh_authorized_keys = match user_data_mapping
            .entry("ssh_authorized_keys".into())
            .or_insert_with(|| serde_yaml::Value::Sequence(vec![]))
        {
            serde_yaml::Value::Sequence(keys) => keys,
            _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
        };

        ssh_authorized_keys.push(self.container_public_key.into());

        // create block device symlinks

        if !self.mounts.block_device.is_empty() {
            let runcmd = match user_data_mapping
                .entry("runcmd".into())
                .or_insert_with(|| serde_yaml::Value::Sequence(vec![]))
            {
                serde_yaml::Value::Sequence(cmds) => cmds,
                _ => return Err(io::Error::other("cloud-init: invalid user-data file").into()),
            };

            for (i, dev) in self.mounts.block_device.iter().enumerate() {
                let parent = match dev.path_in_guest.parent() {
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
                    dev.path_in_guest.to_str().expect("path is utf-8").into(),
                ]));
            }
        }

        // generate iso

        {
            let mut f = File::create(user_data_path)?;
            f.write_all(b"#cloud-config\n")?;
            serde_yaml::to_writer(&mut f, &user_data)?;
        }

        let status = Command::new("genisoimage")
            .arg("-output")
            .arg(out_config_dir_path.as_ref().join("cloud-init.iso"))
            .arg("-volid")
            .arg("cidata")
            .arg("-joliet")
            .arg("-rock")
            .arg("-quiet")
            .arg(out_config_dir_path.as_ref().join("meta-data"))
            .arg(out_config_dir_path.as_ref().join("user-data"))
            .arg(out_config_dir_path.as_ref().join("vendor-data"))
            .spawn()?
            .wait()?;

        if !status.success() {
            return Err(io::Error::other("genisoimage failed").into());
        }

        Ok(())
    }

    pub fn apply_to_ignition_config(
        &self,
        in_config_file_path: Option<impl AsRef<Path>>,
        out_config_file_path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn Error>> {
        // load user config, if any

        let mut user_data: serde_json::Value = if let Some(user_path) = &in_config_file_path {
            fs::copy(user_path, &out_config_file_path)?;
            serde_json::from_reader(File::open(user_path)?)
                .map_err(|e| io::Error::other(format!("ignition: invalid config file: {e}")))?
        } else {
            fs::write(
                &out_config_file_path,
                "{ \"ignition\": { \"version\": \"3.0.0\" } }\n",
            )?;
            serde_json::json!({
                "ignition": {
                    "version": "3.0.0"
                }
            })
        };

        let user_data_mapping = match &mut user_data {
            serde_json::Value::Object(m) => m,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        // adjust authorized keys

        let passwd = match user_data_mapping
            .entry("passwd")
            .or_insert_with(|| serde_json::json!({}))
        {
            serde_json::Value::Object(map) => map,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        let users = match passwd
            .entry("users")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(users) => users,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        let users_contains_core = users.iter().any(|u| match u {
            serde_json::Value::Object(m) => m.get("name") == Some(&"core".into()),
            _ => false,
        });

        if !users_contains_core {
            users.push(serde_json::json!({
                "name": "core",
            }));
        }

        for user in users {
            let map = match user {
                serde_json::Value::Object(m) => m,
                _ => return Err(io::Error::other("ignition: invalid config file").into()),
            };

            if map.get("name") == Some(&"core".into()) {
                let keys = match map
                    .entry("sshAuthorizedKeys")
                    .or_insert_with(|| serde_json::json!([]))
                {
                    serde_json::Value::Array(keys) => keys,
                    _ => return Err(io::Error::other("ignition: invalid config file").into()),
                };

                keys.push(self.container_public_key.into());

                break;
            }
        }

        // adjust hostname

        let storage = match user_data_mapping
            .entry("storage")
            .or_insert_with(|| serde_json::json!({}))
        {
            serde_json::Value::Object(map) => map,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        let files = match storage
            .entry("files")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(files) => files,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        if let Some(hostname) = self.hostname {
            files.retain(|f| {
                !matches!(
                    f,
                    serde_json::Value::Object(m) if m.get("path") == Some(&"/etc/hostname".into())
                )
            });

            files.push(serde_json::json!({
                "path": "/etc/hostname",
                "mode": 420,
                "overwrite": true,
                "contents": {
                    "source": format!("data:,{}", hostname)
                }
            }));
        }

        // create block device symlinks

        let links = match storage
            .entry("links")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(links) => links,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        for (i, dev) in self.mounts.block_device.iter().enumerate() {
            links.push(serde_json::json!({
                "path": dev.path_in_guest,
                "overwrite": true,
                "target": format!("/dev/disk/by-id/virtio-crun-qemu-bdev-{i}"),
                "hard": false,
            }));
        }

        // adjust mounts

        let systemd = match user_data_mapping
            .entry("systemd")
            .or_insert_with(|| serde_json::json!({}))
        {
            serde_json::Value::Object(map) => map,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        let units = match systemd
            .entry("units")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(units) => units,
            _ => return Err(io::Error::other("ignition: invalid config file").into()),
        };

        let mut add_mount = |typ: &str, tag: &str, path_in_guest: &Path| {
            let path_in_guest = path_in_guest.to_str().unwrap();

            // systemd insists on this unit file name format
            let systemd_unit_file_name = format!(
                "{}.mount",
                path_in_guest.trim_matches('/').replace('/', "-")
            );

            let systemd_unit = format!(
                "\
                [Mount]\n\
                What={tag}\n\
                Where={path_in_guest}\n\
                Type={typ}\n\
                \n\
                [Install]\n\
                WantedBy=local-fs.target\n\
                "
            );

            units.push(serde_json::json!({
                "name": systemd_unit_file_name,
                "enabled": true,
                "contents": systemd_unit
            }));
        };

        for (i, mount) in self.mounts.virtiofs.iter().enumerate() {
            add_mount("virtiofs", &format!("virtiofs-{i}"), &mount.path_in_guest);
        }

        for mount in &self.mounts.tmpfs {
            add_mount("tmpfs", "tmpfs", &mount.path_in_guest);
        }

        // generate file

        serde_json::to_writer(File::create(&out_config_file_path)?, &user_data)?;

        Ok(())
    }
}
