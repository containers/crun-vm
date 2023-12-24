// SPDX-License-Identifier: GPL-2.0-or-later

use std::io;
use std::iter;
use std::path::{Path, PathBuf};

use clap::Parser;
use lazy_static::lazy_static;
use regex::Regex;

use crate::commands::create::runtime_env::RuntimeEnv;

#[derive(Debug)]
pub struct VfioPciAddress {
    pub domain: u16,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

impl VfioPciAddress {
    fn from_sys_path(path: impl AsRef<Path>) -> io::Result<Self> {
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

        let path = path.as_ref().canonicalize()?;
        let path = path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not utf-8"))?;

        let capture = PATTERN.captures(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a valid vfio-pci device sysfs path",
            )
        })?;

        let address = VfioPciAddress {
            domain: u16::from_str_radix(&capture[2], 16).unwrap(),
            bus: u8::from_str_radix(&capture[3], 16).unwrap(),
            slot: u8::from_str_radix(&capture[4], 16).unwrap(),
            function: u8::from_str_radix(&capture[5], 16).unwrap(),
        };

        Ok(address)
    }
}

#[derive(Debug)]
pub struct VfioPciMdevUuid(pub String);

impl VfioPciMdevUuid {
    fn from_sys_path(path: impl AsRef<Path>) -> io::Result<Self> {
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

        let path = path.as_ref().canonicalize()?;
        let path = path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not utf-8"))?;

        let capture = PATTERN.captures(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a valid vfio-pci mediated device sysfs path",
            )
        })?;

        Ok(VfioPciMdevUuid(capture[2].to_string()))
    }
}

#[derive(clap::Parser, Debug)]
struct CustomOptionsRaw {
    #[clap(long)]
    cloud_init: Option<PathBuf>,

    #[clap(long)]
    ignition: Option<PathBuf>,

    #[clap(long)]
    vfio_pci: Vec<PathBuf>,

    #[clap(long)]
    vfio_pci_mdev: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct CustomOptions {
    pub cloud_init: Option<PathBuf>,
    pub ignition: Option<PathBuf>,
    pub vfio_pci: Vec<VfioPciAddress>,
    pub vfio_pci_mdev: Vec<VfioPciMdevUuid>,
}

impl CustomOptions {
    pub fn from_spec(spec: &oci_spec::runtime::Spec, env: RuntimeEnv) -> io::Result<Self> {
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
        let mut options = CustomOptionsRaw::parse_from(
            iter::once(&"podman run ... <image>".to_string()).chain(args),
        );

        if env.needs_absolute_custom_opt_paths() {
            fn any_is_relative(iter: impl IntoIterator<Item = impl AsRef<Path>>) -> bool {
                iter.into_iter().any(|p| p.as_ref().is_relative())
            }

            if any_is_relative(&options.cloud_init)
                || any_is_relative(&options.ignition)
                || any_is_relative(&options.vfio_pci)
                || any_is_relative(&options.vfio_pci_mdev)
            {
                return Err(io::Error::other(format!(
                    concat!(
                        "paths specified using --cloud-init, --ignition, --vfio-pci, or",
                        " --vfio-pci-mdev must be absolute when using crun-qemu with {}",
                    ),
                    env.name().unwrap()
                )));
            }
        }

        if env == RuntimeEnv::Kubernetes {
            fn path_in_container_into_path_in_host(
                spec: &oci_spec::runtime::Spec,
                path: Option<&mut PathBuf>,
            ) -> io::Result<()> {
                if let Some(path) = path {
                    let mount = spec
                        .mounts()
                        .iter()
                        .flatten()
                        .filter(|m| m.source().is_some())
                        .filter(|m| path.starts_with(m.destination()))
                        .last()
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!("can't find {}", path.to_str().unwrap()),
                            )
                        })?;

                    let relative_path = path.strip_prefix(mount.destination()).unwrap();
                    let path_in_host = mount.source().as_ref().unwrap().join(relative_path);

                    if !path_in_host.try_exists()? {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("can't find {}", path.to_str().unwrap()),
                        ));
                    }

                    *path = path_in_host;
                }

                Ok(())
            }

            path_in_container_into_path_in_host(spec, options.cloud_init.as_mut())?;
            path_in_container_into_path_in_host(spec, options.ignition.as_mut())?;
        }

        let options = CustomOptions {
            cloud_init: options.cloud_init,
            ignition: options.ignition,
            vfio_pci: options
                .vfio_pci
                .iter()
                .map(VfioPciAddress::from_sys_path)
                .collect::<io::Result<_>>()?,
            vfio_pci_mdev: options
                .vfio_pci_mdev
                .iter()
                .map(VfioPciMdevUuid::from_sys_path)
                .collect::<io::Result<_>>()?,
        };

        Ok(options)
    }
}
