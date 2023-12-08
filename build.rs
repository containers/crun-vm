// SPDX-License-Identifier: GPL-3.0-only

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=runner/");

    // check if runner Containerfile *contents* changed (Cargo checks timestamps)

    let containerfile_copy = Path::new(&env::var("OUT_DIR")?).join("runner.Containerfile");

    let status = Command::new("cmp")
        .arg("--quiet")
        .arg("runner/Containerfile")
        .arg(&containerfile_copy)
        .spawn()?
        .wait()?;

    if status.code().unwrap() != 0 {
        // build runner image into an OCI archive

        let status = Command::new("podman")
            .arg("image")
            .arg("build")
            .arg("--tag")
            .arg(&format!("oci-archive:{}/runner.oci", env::var("OUT_DIR")?))
            .arg("--squash-all") // TODO: this resolves a podman build failure but should probably be removed
            .arg("runner/")
            .spawn()?
            .wait()?;
        check_status(status)?;

        // create a container from that OCI archive

        let output = Command::new("podman")
            .arg("container")
            .arg("create")
            .arg(&format!("oci-archive:{}/runner.oci", env::var("OUT_DIR")?))
            .stdout(Stdio::piped())
            .output()?;
        check_status(output.status)?;

        let container_id = std::str::from_utf8(&output.stdout)?.trim();

        // extract the container's root filesystem

        let result = extract_root(container_id);

        // remove the container

        let status = Command::new("podman")
            .arg("container")
            .arg("rm")
            .arg("--ignore")
            .arg("--force")
            .arg("--time=0")
            .arg(container_id)
            .spawn()?
            .wait()?;
        check_status(status)?;

        result?;

        fs::copy("runner/Containerfile", &containerfile_copy)?;
    }

    Ok(())
}

fn extract_root(container_id: &str) -> Result<(), Box<dyn Error>> {
    let status = Command::new("podman")
        .arg("container")
        .arg("export")
        .arg("-o")
        .arg(&format!("{}/runner.tar", env::var("OUT_DIR")?))
        .arg(container_id)
        .spawn()?
        .wait()?;
    check_status(status)?;

    Ok(())
}

fn check_status(status: ExitStatus) -> Result<(), Box<dyn Error>> {
    match status.success() {
        true => Ok(()),
        false => Err(Box::new(io::Error::other(format!("{:?}", status)))),
    }
}
