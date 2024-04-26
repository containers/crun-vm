// SPDX-License-Identifier: GPL-2.0-or-later

use std::iter;
use std::str::FromStr;

use anyhow::{anyhow, ensure, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use lazy_static::lazy_static;
use regex::Regex;

use crate::commands::create::engine::Engine;

#[derive(Clone, Debug)]
pub struct Blockdev {
    pub source: Utf8PathBuf,
    pub target: Utf8PathBuf,
    pub format: String,
}

impl FromStr for Blockdev {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Blockdev> {
        lazy_static! {
            static ref PATTERN: Regex =
                Regex::new(r"^source=([^,]+),target=([^,]+),format=([^,]+)$").unwrap();
        }

        let captures = PATTERN
            .captures(s)
            .ok_or_else(|| anyhow!("invalid --blockdev option"))?;

        let blockdev = Blockdev {
            source: Utf8PathBuf::from(&captures[1]),
            target: Utf8PathBuf::from(&captures[2]),
            format: captures[3].to_string(),
        };

        Ok(blockdev)
    }
}

#[derive(clap::Parser, Debug)]
pub struct CustomOptions {
    #[clap(long)]
    pub blockdev: Vec<Blockdev>,

    #[clap(long)]
    pub persistent: bool,

    #[clap(long)]
    pub random_ssh_key_pair: bool,

    #[clap(long, help = "Use system emulation rather than KVM")]
    pub emulated: bool,

    #[clap(long)]
    pub cloud_init: Option<Utf8PathBuf>,

    #[clap(long)]
    pub ignition: Option<Utf8PathBuf>,

    #[clap(long)]
    pub password: Option<String>,

    #[clap(long)]
    pub merge_libvirt_xml: Vec<Utf8PathBuf>,

    #[clap(long)]
    pub print_libvirt_xml: bool,

    #[clap(long, conflicts_with = "print_libvirt_xml")]
    pub print_config_json: bool,
}

impl CustomOptions {
    pub fn from_spec(spec: &oci_spec::runtime::Spec, engine: Engine) -> Result<Self> {
        let mut args: Vec<&String> = spec
            .process()
            .as_ref()
            .unwrap()
            .args()
            .iter()
            .flatten()
            .filter(|a| !a.trim().is_empty())
            .collect();

        if let Some(&first_arg) = args.first() {
            let ignore = [
                "no-entrypoint",
                "/sbin/init",
                "/usr/sbin/init",
                "/usr/local/sbin/init",
            ];

            if ignore.contains(&first_arg.as_str()) {
                args.remove(0);
            }
        }

        if let Some(&first_arg) = args.first() {
            ensure!(
                first_arg.starts_with('-'),
                "unexpected entrypoint '{first_arg}' found; use an image without an entrypoint or with entrypoint \"no-entrypoint\", and/or pass in an empty \"\" entrypoint on the command line"
            );
        }

        let mut options = CustomOptions::parse_from(
            iter::once(&"podman run [<podman-opts>] <image>".to_string()).chain(args),
        );

        ensure!(
            !spec.root().as_ref().unwrap().readonly().unwrap_or(false) || !options.persistent,
            "--persistent was set but the container's root file system was mounted as read-only"
        );

        fn all_are_absolute(iter: impl IntoIterator<Item = impl AsRef<Utf8Path>>) -> bool {
            iter.into_iter().all(|p| p.as_ref().is_absolute())
        }

        fn path_in_container_into_path_in_host(
            spec: &oci_spec::runtime::Spec,
            path: impl AsRef<Utf8Path>,
        ) -> Result<Utf8PathBuf> {
            let mount = spec
                .mounts()
                .iter()
                .flatten()
                .filter(|m| m.source().is_some())
                .filter(|m| path.as_ref().starts_with(m.destination()))
                .last()
                .ok_or_else(|| anyhow!("can't find {}", path.as_ref()))?;

            let mount_source: &Utf8Path = mount.source().as_deref().unwrap().try_into()?;

            let relative_path = path.as_ref().strip_prefix(mount.destination()).unwrap();
            let path_in_host = mount_source.join(relative_path);

            ensure!(path_in_host.try_exists()?, "can't find {}", path.as_ref());

            Ok(path_in_host)
        }

        // Docker doesn't run the runtime with the same working directory as the process
        // that launched `docker-run`. Similarly, custom option paths in Kubernetes refer to
        // paths in the container/VM, and there isn't a reasonable notion of what the
        // current directory is. We thus simply always require custom option paths to be
        // absolute.
        ensure!(
            all_are_absolute(options.blockdev.iter().flat_map(|b| [&b.source, &b.target]))
                && all_are_absolute(&options.cloud_init)
                && all_are_absolute(&options.ignition)
                && all_are_absolute(&options.merge_libvirt_xml),
            concat!(
                "paths specified using --blockdev, --cloud-init, --ignition, or",
                " --merge-libvirt-xml must be absolute",
            ),
        );

        if engine == Engine::Kubernetes {
            for blockdev in &mut options.blockdev {
                blockdev.source = path_in_container_into_path_in_host(spec, &blockdev.source)?;
                blockdev.target = path_in_container_into_path_in_host(spec, &blockdev.target)?;
            }

            if let Some(path) = &mut options.cloud_init {
                *path = path_in_container_into_path_in_host(spec, &path)?;
            }

            if let Some(path) = &mut options.ignition {
                *path = path_in_container_into_path_in_host(spec, &path)?;
            }

            for path in &mut options.merge_libvirt_xml {
                *path = path_in_container_into_path_in_host(spec, &path)?;
            }
        }

        Ok(options)
    }
}
