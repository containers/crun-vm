// SPDX-License-Identifier: GPL-2.0-or-later

use std::iter;
use std::str::FromStr;

use anyhow::{anyhow, ensure, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use lazy_static::lazy_static;
use regex::Regex;

use crate::commands::create::runtime_env::RuntimeEnv;

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
    pub cloud_init: Option<Utf8PathBuf>,

    #[clap(long)]
    pub ignition: Option<Utf8PathBuf>,

    #[clap(long)]
    pub password: Option<String>,

    #[clap(long)]
    pub merge_libvirt_xml: Vec<Utf8PathBuf>,

    #[clap(long)]
    pub print_libvirt_xml: bool,
}

impl CustomOptions {
    pub fn from_spec(spec: &oci_spec::runtime::Spec, env: RuntimeEnv) -> Result<Self> {
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
        let mut options = CustomOptions::parse_from(
            iter::once(&"podman run [<podman-opts>] <image>".to_string()).chain(args),
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

        match env {
            RuntimeEnv::Docker => {
                // Docker doesn't run the runtime with the same working directory as the process
                // that launched `docker-run`, so we require custom option paths to be absolute.
                //
                // TODO: There must be a better way...
                ensure!(
                    all_are_absolute(options.blockdev.iter().flat_map(|b| [&b.source, &b.target]))
                        && all_are_absolute(&options.cloud_init)
                        && all_are_absolute(&options.ignition)
                        && all_are_absolute(&options.merge_libvirt_xml),
                    concat!(
                        "paths specified using --blockdev, --cloud-init, --ignition, or",
                        " --merge-libvirt-xml must be absolute when using crun-vm as a Docker",
                        " runtime",
                    ),
                );
            }
            RuntimeEnv::Kubernetes => {
                // Custom option paths in Kubernetes refer to paths in the container/VM, and there
                // isn't a reasonable notion of what the current directory is.
                ensure!(
                    all_are_absolute(options.blockdev.iter().flat_map(|b| [&b.source, &b.target]))
                        && all_are_absolute(&options.cloud_init)
                        && all_are_absolute(&options.ignition)
                        && all_are_absolute(&options.merge_libvirt_xml),
                    concat!(
                        "paths specified using --blockdev, --cloud-init, --ignition, or",
                        " --merge-libvirt-xml must be absolute when using crun-vm as a",
                        " Kubernetes runtime",
                    ),
                );

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
            RuntimeEnv::Other => {}
        }

        Ok(options)
    }
}
