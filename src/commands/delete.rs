// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{ensure, Result};
use camino::Utf8PathBuf;
use serde::Deserialize;

use crate::util::{crun, ensure_unmounted};

pub fn delete(args: &liboci_cli::Delete, raw_args: &[impl AsRef<OsStr>]) -> Result<()> {
    // get container root path

    // the container might not exist because creation failed midway through, so we ignore errors
    let root_path = get_root_path(&args.container_id).ok();

    // actually delete the container

    crun(raw_args)?;

    // clean up crun-vm mounts so that user doesn't have to deal with them when they decide to
    // delete crun-vm's state/private directory

    if let Some(root_path) = root_path {
        let private_dir_path: Utf8PathBuf = root_path
            .canonicalize()?
            .parent()
            .unwrap()
            .to_path_buf()
            .try_into()?;

        let image_dir_path = private_dir_path.join("root/crun-vm/image");
        let image_file_path = image_dir_path.join("image");

        ensure_unmounted(image_file_path)?;
        ensure_unmounted(image_dir_path)?;
    }

    Ok(())
}

fn get_root_path(container_id: &str) -> Result<Utf8PathBuf> {
    let output = Command::new("crun")
        .arg("state")
        .arg(container_id)
        .stderr(Stdio::null())
        .output()?;

    ensure!(output.status.success());

    #[derive(Deserialize)]
    struct ContainerState {
        rootfs: PathBuf,
    }

    let state: ContainerState = serde_json::from_slice(&output.stdout)?;

    Ok(state.rootfs.try_into()?)
}
