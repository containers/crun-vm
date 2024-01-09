// SPDX-License-Identifier: GPL-2.0-or-later

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, ensure, Context, Result};

use crate::commands::create::Mounts;
use crate::util::PathExt;

pub struct FirstBootConfig<'a> {
    pub hostname: Option<&'a str>,
    pub container_public_key: &'a str,
    pub password: Option<&'a str>,
    pub mounts: &'a Mounts,
}

impl FirstBootConfig<'_> {
    /// Returns `true` if a cloud-init config should be passed to the VM.
    pub fn apply_to_cloud_init_config(
        &self,
        in_config_dir_path: Option<impl AsRef<Path>>,
        out_config_dir_path: impl AsRef<Path>,
        out_config_iso_file_path: impl AsRef<Path>,
    ) -> Result<()> {
        fs::create_dir_all(&out_config_dir_path)?;

        // create copy of config

        let mut user_data: serde_yaml::Value;

        if let Some(in_config_dir_path) = &in_config_dir_path {
            for file in ["meta-data", "user-data"] {
                let path = in_config_dir_path.as_ref().join(file);
                ensure!(
                    path.is_file(),
                    "missing mandatory config file {}",
                    path.as_str()
                );
            }

            for file in ["meta-data", "user-data", "vendor-data"] {
                if in_config_dir_path.as_ref().join(file).try_exists()? {
                    // TODO: Security vulnerability, symlink may point somewhere on host that user
                    // shouldn't be able to access, especially when running as a Kubernetes runtime.
                    fs::copy(
                        in_config_dir_path.as_ref().join(file),
                        out_config_dir_path.as_ref().join(file),
                    )?;
                }
            }

            let user_data_str = fs::read_to_string(in_config_dir_path.as_ref().join("user-data"))?;

            if let Some(line) = user_data_str.lines().next() {
                ensure!(
                    line.trim() == "#cloud-config",
                    "expected shebang '#cloud-config' in user-data file"
                );
            }

            user_data = serde_yaml::from_str(&user_data_str).context("invalid user-data file")?;
        } else {
            user_data = serde_yaml::Value::Null;
        }

        // adjust user-data config

        if let serde_yaml::Value::Null = &user_data {
            user_data = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        }

        let user_data_mapping = match &mut user_data {
            serde_yaml::Value::Mapping(m) => m,
            _ => bail!("invalid user-data file"),
        };

        // set user passwords

        if let Some(password) = self.password {
            user_data_mapping.insert("password".into(), password.into());

            let chpasswd = match user_data_mapping
                .entry("chpasswd".into())
                .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            {
                serde_yaml::Value::Mapping(m) => m,
                _ => bail!("invalid user-data file"),
            };

            chpasswd.insert("expire".into(), false.into());
        }

        // adjust mounts

        if !self.mounts.virtiofs.is_empty() || !self.mounts.tmpfs.is_empty() {
            let mounts: &mut Vec<serde_yaml::Value> = match user_data_mapping
                .entry("mounts".into())
                .or_insert_with(|| serde_yaml::Value::Sequence(vec![]))
            {
                serde_yaml::Value::Sequence(mounts) => mounts,
                _ => bail!("invalid user-data file"),
            };

            let mut add_mount = |typ: &str, tag: &str, path_in_guest: &Path| {
                let path_in_guest = path_in_guest.as_str();
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
            _ => bail!("invalid user-data file"),
        };

        ssh_authorized_keys.push(self.container_public_key.into());

        // create block device symlinks and udev rules

        let block_device_symlinks = self.get_block_device_symlinks();
        let block_device_udev_rules = self.get_block_device_udev_rules();

        if !block_device_symlinks.is_empty() || block_device_udev_rules.is_some() {
            let runcmd = match user_data_mapping
                .entry("runcmd".into())
                .or_insert_with(|| serde_yaml::Value::Sequence(vec![]))
            {
                serde_yaml::Value::Sequence(v) => v,
                _ => bail!("invalid user-data file"),
            };

            for (path, target) in block_device_symlinks {
                runcmd.push(serde_yaml::Value::Sequence(vec![
                    "mkdir".into(),
                    "-p".into(),
                    path.parent().unwrap().as_str().into(),
                ]));

                runcmd.push(serde_yaml::Value::Sequence(vec![
                    "ln".into(),
                    "--symbolic".into(),
                    target.as_str().into(),
                    path.as_str().into(),
                ]));
            }

            if block_device_udev_rules.is_some() {
                runcmd.push("udevadm trigger".into());
            }
        }

        if let Some(rules) = block_device_udev_rules {
            let write_files = match user_data_mapping
                .entry("write_files".into())
                .or_insert_with(|| serde_yaml::Value::Sequence(vec![]))
            {
                serde_yaml::Value::Sequence(v) => v,
                _ => bail!("invalid user-data file"),
            };

            let mut mapping = serde_yaml::Mapping::new();
            mapping.insert("path".into(), "/etc/udev/rules.d/99-crun-qemu.rules".into());
            mapping.insert("content".into(), rules.into());

            write_files.push(mapping.into());
        }

        // generate iso

        {
            let mut f = File::create(out_config_dir_path.as_ref().join("user-data"))?;
            f.write_all(b"#cloud-config\n")?;
            serde_yaml::to_writer(&mut f, &user_data)?;
        }

        for file in ["meta-data", "vendor-data"] {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(out_config_dir_path.as_ref().join(file))?;
        }

        let status = Command::new("genisoimage")
            .arg("-output")
            .arg(out_config_iso_file_path.as_ref())
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

        ensure!(status.success(), "genisoimage failed");

        Ok(())
    }

    pub fn apply_to_ignition_config(
        &self,
        in_config_file_path: Option<impl AsRef<Path>>,
        out_config_file_path: impl AsRef<Path>,
    ) -> Result<()> {
        // load user config, if any

        let mut user_data: serde_json::Value = if let Some(user_path) = &in_config_file_path {
            fs::copy(user_path, &out_config_file_path)?;
            serde_json::from_reader(File::open(user_path).map(BufReader::new)?)
                .context("invalid config file")?
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
            _ => bail!("invalid config file"),
        };

        // adjust authorized keys

        let passwd = match user_data_mapping
            .entry("passwd")
            .or_insert_with(|| serde_json::json!({}))
        {
            serde_json::Value::Object(map) => map,
            _ => bail!("invalid config file"),
        };

        let users = match passwd
            .entry("users")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(users) => users,
            _ => bail!("invalid config file"),
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
                _ => bail!("invalid config file"),
            };

            if map.get("name") == Some(&"core".into()) {
                let keys = match map
                    .entry("sshAuthorizedKeys")
                    .or_insert_with(|| serde_json::json!([]))
                {
                    serde_json::Value::Array(keys) => keys,
                    _ => bail!("invalid config file"),
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
            _ => bail!("invalid config file"),
        };

        let files = match storage
            .entry("files")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(files) => files,
            _ => bail!("invalid config file"),
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
                "mode": 0o644,
                "overwrite": true,
                "contents": {
                    "source": format!("data:,{}", hostname)
                }
            }));
        }

        // create block device symlinks and udev rules

        if let Some(rules) = self.get_block_device_udev_rules() {
            files.push(serde_json::json!({
                "path": "/etc/udev/rules.d/99-crun-qemu.rules",
                "mode": 0o644,
                "overwrite": true,
                "contents": {
                    "source": format!("data:,{}", urlencoding::encode(&rules))
                }
            }));
        }

        let links = match storage
            .entry("links")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(links) => links,
            _ => bail!("invalid config file"),
        };

        for (path, target) in self.get_block_device_symlinks() {
            links.push(serde_json::json!({
                "path": path.as_str(),
                "overwrite": true,
                "target": target.as_str(),
                "hard": false,
            }));
        }

        // adjust mounts

        let systemd = match user_data_mapping
            .entry("systemd")
            .or_insert_with(|| serde_json::json!({}))
        {
            serde_json::Value::Object(map) => map,
            _ => bail!("invalid config file"),
        };

        let units = match systemd
            .entry("units")
            .or_insert_with(|| serde_json::json!([]))
        {
            serde_json::Value::Array(units) => units,
            _ => bail!("invalid config file"),
        };

        let mut add_mount = |typ: &str, tag: &str, path_in_guest: &Path| {
            let path_in_guest = path_in_guest.as_str();

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

        serde_json::to_writer(
            File::create(&out_config_file_path).map(BufWriter::new)?,
            &user_data,
        )?;

        Ok(())
    }

    fn get_block_device_symlinks(&self) -> Vec<(&Path, PathBuf)> {
        let mut symlinks = Vec::new();

        for (i, dev) in self.mounts.block_device.iter().enumerate() {
            if dev.path_in_guest.parent() != Some(Path::new("/dev")) {
                let target = PathBuf::from(format!("/dev/disk/by-id/virtio-crun-qemu-block-{i}"));
                symlinks.push((dev.path_in_guest.as_path(), target));
            }
        }

        symlinks
    }

    fn get_block_device_udev_rules(&self) -> Option<String> {
        let mut rules = String::new();

        for (i, dev) in self.mounts.block_device.iter().enumerate() {
            if dev.path_in_guest.parent() == Some(Path::new("/dev")) {
                rules.push_str(&format!(
                    "ENV{{ID_SERIAL}}==\"crun-qemu-block-{}\", SYMLINK+=\"{}\"\n",
                    i,
                    dev.path_in_guest.file_name().unwrap().as_str(),
                ));
            }
        }

        if rules.is_empty() {
            None
        } else {
            Some(rules)
        }
    }
}
