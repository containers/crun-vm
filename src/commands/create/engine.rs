// SPDX-License-Identifier: GPL-2.0-or-later

use std::fs;
use std::path::Path;

use anyhow::{bail, Result};
use camino::Utf8Path;
use lazy_static::lazy_static;
use regex::Regex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Engine {
    Podman,
    Docker,
    Kubernetes,
}

impl Engine {
    pub fn command(self) -> Option<&'static str> {
        match self {
            Engine::Podman => Some("podman"),
            Engine::Docker => Some("docker"),
            Engine::Kubernetes => None,
        }
    }

    pub fn detect(
        container_id: &str,
        bundle_path: &Utf8Path,
        spec: &oci_spec::runtime::Spec,
        original_root_path: impl AsRef<Utf8Path>,
    ) -> Result<Engine> {
        // TODO: Make this absolutely robust and secure. Probably require engine config to pass us
        // an option specifying what engine is running crun-vm.

        // check if we're under CRI-O under Kubernetes

        {
            let has_kubernetes_secrets_dir = spec.mounts().iter().flatten().any(|m| {
                m.destination()
                    .starts_with("/var/run/secrets/kubernetes.io")
            });

            let has_kubernetes_managed_etc_hosts = spec
                .mounts()
                .iter()
                .flatten()
                .filter(|m| m.destination() == Utf8Path::new("/etc/hosts"))
                .flat_map(|m| m.source())
                .next()
                .map(fs::read_to_string)
                .transpose()?
                .and_then(|hosts| hosts.lines().next().map(|line| line.to_string()))
                .map(|line| line.contains("Kubernetes-managed hosts file"))
                .unwrap_or(false);

            if has_kubernetes_secrets_dir || has_kubernetes_managed_etc_hosts {
                return Ok(Engine::Kubernetes);
            }
        }

        // check if we're under Docker

        {
            let has_dot_dockerenv_file = original_root_path
                .as_ref()
                .join(".dockerenv")
                .try_exists()?;

            if has_dot_dockerenv_file {
                return Ok(Engine::Docker);
            }
        }

        // check if we're under Podman

        {
            let has_mount_on = |p| {
                spec.mounts()
                    .iter()
                    .flatten()
                    .any(|m| m.destination() == Path::new(p))
            };

            let has_dot_containerenv_file =
                has_mount_on("/run/.containerenv") || has_mount_on("/var/run/.containerenv");

            lazy_static! {
                static ref BUNDLE_PATH_PATTERN: Regex =
                    Regex::new(r"/overlay-containers/([^/]+)/userdata$").unwrap();
            }

            let is_podman_bundle_path = match BUNDLE_PATH_PATTERN.captures(bundle_path.as_str()) {
                Some(captures) => &captures[1] == container_id,
                None => false,
            };

            if has_dot_containerenv_file && is_podman_bundle_path {
                return Ok(Engine::Podman);
            }
        }

        // unknown engine

        bail!("could not identify container engine; crun-vm current only supports Podman, Docker, and Kubernetes");
    }
}
