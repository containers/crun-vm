// SPDX-License-Identifier: GPL-2.0-or-later

use std::iter;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, ensure, Result};
use clap::Parser;
use lazy_static::lazy_static;
use regex::Regex;

use crate::commands::create::runtime_env::RuntimeEnv;
use crate::util::PathExt;

#[derive(Clone, Debug)]
pub struct Blockdev {
    pub source: PathBuf,
    pub target: PathBuf,
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
            source: PathBuf::from(&captures[1]),
            target: PathBuf::from(&captures[2]),
            format: captures[3].to_string(),
        };

        Ok(blockdev)
    }
}

#[derive(Clone, Debug)]
pub struct VfioPciAddress {
    pub domain: u16,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

impl VfioPciAddress {
    fn from_path(path: impl AsRef<Path>) -> Result<Self> {
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

impl VfioPciMdevUuid {
    fn from_path(path: impl AsRef<Path>) -> Result<Self> {
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

        let capture = PATTERN
            .captures(path.as_str())
            .ok_or_else(|| anyhow!("not a valid vfio-pci mediated device sysfs path"))?;

        Ok(VfioPciMdevUuid(capture[2].to_string()))
    }
}

#[derive(Debug)]
pub struct CustomOptions {
    pub blockdev: Vec<Blockdev>,
    pub persistent: bool,
    pub cloud_init: Option<PathBuf>,
    pub ignition: Option<PathBuf>,
    pub vfio_pci: Vec<VfioPciAddress>,
    pub vfio_pci_mdev: Vec<VfioPciMdevUuid>,
    pub password: Option<String>,
    pub merge_libvirt_xml: Vec<PathBuf>,
    pub print_libvirt_xml: bool,
}

impl TryFrom<CustomOptionsRaw> for CustomOptions {
    type Error = anyhow::Error;

    fn try_from(opts: CustomOptionsRaw) -> Result<Self> {
        Ok(Self {
            blockdev: opts.blockdev,
            persistent: opts.persistent,
            cloud_init: opts.cloud_init,
            ignition: opts.ignition,
            vfio_pci: opts
                .vfio_pci
                .iter()
                .map(VfioPciAddress::from_path)
                .collect::<Result<_>>()?,
            vfio_pci_mdev: opts
                .vfio_pci_mdev
                .iter()
                .map(VfioPciMdevUuid::from_path)
                .collect::<Result<_>>()?,
            password: opts.password,
            merge_libvirt_xml: opts.merge_libvirt_xml,
            print_libvirt_xml: opts.print_libvirt_xml,
        })
    }
}

#[derive(clap::Parser, Debug)]
struct CustomOptionsRaw {
    #[clap(long)]
    blockdev: Vec<Blockdev>,

    #[clap(long)]
    persistent: bool,

    #[clap(long)]
    cloud_init: Option<PathBuf>,

    #[clap(long)]
    ignition: Option<PathBuf>,

    #[clap(long)]
    vfio_pci: Vec<PathBuf>,

    #[clap(long)]
    vfio_pci_mdev: Vec<PathBuf>,

    #[clap(long)]
    password: Option<String>,

    #[clap(long)]
    merge_libvirt_xml: Vec<PathBuf>,

    #[clap(long)]
    print_libvirt_xml: bool,
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
        let mut options = CustomOptionsRaw::parse_from(
            iter::once(&"podman run [<podman-opts>] <image>".to_string()).chain(args),
        );

        fn all_are_absolute(iter: impl IntoIterator<Item = impl AsRef<Path>>) -> bool {
            iter.into_iter().all(|p| p.as_ref().is_absolute())
        }

        fn path_in_container_into_path_in_host(
            spec: &oci_spec::runtime::Spec,
            path: impl AsRef<Path>,
        ) -> Result<PathBuf> {
            let mount = spec
                .mounts()
                .iter()
                .flatten()
                .filter(|m| m.source().is_some())
                .filter(|m| path.as_ref().starts_with(m.destination()))
                .last()
                .ok_or_else(|| anyhow!("can't find {}", path.as_str()))?;

            let relative_path = path.as_ref().strip_prefix(mount.destination()).unwrap();
            let path_in_host = mount.source().as_ref().unwrap().join(relative_path);

            ensure!(path_in_host.try_exists()?, "can't find {}", path.as_str());

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
                        && all_are_absolute(&options.vfio_pci)
                        && all_are_absolute(&options.vfio_pci_mdev)
                        && all_are_absolute(&options.merge_libvirt_xml),
                    concat!(
                        "paths specified using --blockdev, --cloud-init, --ignition, --vfio-pci,",
                        " --vfio-pci-mdev, or --merge-libvirt-xml must be absolute when using",
                        " crun-qemu as a Docker runtime",
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
                        " --merge-libvirt-xml must be absolute when using crun-qemu as a",
                        " Kubernetes runtime",
                    ),
                );

                ensure!(
                    options.vfio_pci.is_empty() && options.vfio_pci_mdev.is_empty(),
                    concat!(
                        "options --vfio-pci and --vfio-pci-mdev are not allowed when using",
                        " crun-qemu as a Kubernetes runtime",
                    )
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

        options.try_into()
    }
}
