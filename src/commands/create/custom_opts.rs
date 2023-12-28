// SPDX-License-Identifier: GPL-2.0-or-later

use std::iter;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, bail, ensure, Result};
use clap::Parser;
use lazy_static::lazy_static;
use regex::Regex;

use crate::commands::create::runtime_env::RuntimeEnv;
use crate::util::PathExt;

#[derive(Clone, Debug)]
pub struct VfioPciAddress {
    pub domain: u16,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

impl FromStr for VfioPciAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        lazy_static! {
            static ref PATTERN: Regex = {
                let h = r"[0-9a-fA-F]".to_string();
                let db = format!(r"{h}{{4}}:{h}{{2}}");
                let dbsf = format!(r"{h}{{4}}:{h}{{2}}:{h}{{2}}.{h}{{1}}");

                let pattern = format!(
                    r"^/sys/devices/pci{db}(/{dbsf})*/({h}{{4}}):({h}{{2}}):({h}{{2}}).({h}{{1}})$"
                );

                Regex::new(&pattern).unwrap()
            };
        }

        let path = Path::new(s).canonicalize()?;

        let capture = PATTERN
            .captures(path.as_str())
            .ok_or_else(|| anyhow!("not a valid vfio-pci device sysfs path"))?;

        let address = VfioPciAddress {
            domain: u16::from_str_radix(&capture[2], 16).unwrap(),
            bus: u8::from_str_radix(&capture[3], 16).unwrap(),
            slot: u8::from_str_radix(&capture[4], 16).unwrap(),
            function: u8::from_str_radix(&capture[5], 16).unwrap(),
        };

        Ok(address)
    }
}

#[derive(Clone, Debug)]
pub struct VfioPciMdevUuid(pub String);

impl FromStr for VfioPciMdevUuid {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        lazy_static! {
            static ref PATTERN: Regex = {
                let h = r"[0-9a-zA-Z]".to_string();
                let db = format!(r"{h}{{4}}:{h}{{2}}");
                let dbsf = format!(r"{h}{{4}}:{h}{{2}}:{h}{{2}}.{h}{{1}}");
                let uuid = format!(r"{h}{{8}}-{h}{{4}}-{h}{{4}}-{h}{{4}}-{h}{{12}}");

                let pattern = format!(r"^/sys/devices/pci{db}(/{dbsf})+/({uuid})$");

                Regex::new(&pattern).unwrap()
            };
        }

        let path = Path::new(s).canonicalize()?;

        let capture = PATTERN
            .captures(path.as_str())
            .ok_or_else(|| anyhow!("not a valid vfio-pci mediated device sysfs path"))?;

        Ok(VfioPciMdevUuid(capture[2].to_string()))
    }
}

#[derive(clap::Parser, Debug)]
pub struct CustomOptions {
    #[clap(long)]
    pub persist_changes: bool,

    #[clap(long)]
    pub cloud_init: Option<PathBuf>,

    #[clap(long)]
    pub ignition: Option<PathBuf>,

    #[clap(long)]
    pub vfio_pci: Vec<VfioPciAddress>,

    #[clap(long)]
    pub vfio_pci_mdev: Vec<VfioPciMdevUuid>,

    #[clap(long)]
    pub password: Option<String>,
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

        if env.needs_absolute_custom_opt_paths() {
            fn any_is_relative(iter: impl IntoIterator<Item = impl AsRef<Path>>) -> bool {
                iter.into_iter().any(|p| p.as_ref().is_relative())
            }

            if any_is_relative(&options.cloud_init) || any_is_relative(&options.ignition) {
                bail!(
                    concat!(
                        "paths specified using --cloud-init or --ignition must be absolute when",
                        " using crun-qemu with {}",
                    ),
                    env.name().unwrap()
                );
            }
        }

        if env == RuntimeEnv::Kubernetes {
            fn path_in_container_into_path_in_host(
                spec: &oci_spec::runtime::Spec,
                path: Option<&mut PathBuf>,
            ) -> Result<()> {
                if let Some(path) = path {
                    let mount = spec
                        .mounts()
                        .iter()
                        .flatten()
                        .filter(|m| m.source().is_some())
                        .filter(|m| path.starts_with(m.destination()))
                        .last()
                        .ok_or_else(|| anyhow!("can't find {}", path.as_str()))?;

                    let relative_path = path.strip_prefix(mount.destination()).unwrap();
                    let path_in_host = mount.source().as_ref().unwrap().join(relative_path);

                    ensure!(path_in_host.try_exists()?, "can't find {}", path.as_str());

                    *path = path_in_host;
                }

                Ok(())
            }

            path_in_container_into_path_in_host(spec, options.cloud_init.as_mut())?;
            path_in_container_into_path_in_host(spec, options.ignition.as_mut())?;
        }

        Ok(options)
    }
}
