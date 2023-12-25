// SPDX-License-Identifier: GPL-2.0-or-later

#![allow(clippy::items_after_test_module)]

use std::ffi::OsStr;
use std::io;
use std::process::{Command, Stdio};

use test_case::test_matrix;
use uuid::Uuid;

#[test_matrix(
    // engine
    [
        Engine::Podman,
        Engine::Docker,
    ],

    // args
    [
        [
            "quay.io/containerdisks/fedora:39",
            ""
        ],
        [
            "-h=my-test-vm",
            "-v=./util:/home/fedora/util",
            "quay.io/containerdisks/fedora:39",
            &format!("--cloud-init={REPO_PATH}/examples/cloud-init/config"),
            &format!("--ignition={REPO_PATH}/examples/ignition/config.ign"),
        ],
    ]
)]
#[test_matrix(
    // engine
    [
        Engine::Podman,
    ],

    // args
    [
        [
            "-h=my-test-vm",
            "-v=./util:/home/fedora/util",
            "quay.io/containerdisks/fedora:39",
            "--cloud-init=examples/cloud-init/config",
            "--ignition=examples/ignition/config.ign",
        ],
    ]
)]
fn test_run(engine: Engine, args: impl IntoIterator<Item = impl AsRef<OsStr>>) {
    let container_name = get_random_container_name();

    // launch VM

    let mut run_child: std::process::Child = engine
        .command("run")
        .arg(format!("--name={}", container_name))
        .arg("--rm")
        .args(args)
        .spawn()
        .unwrap();

    // wait until we can exec into the VM

    let result = (|| -> io::Result<()> {
        loop {
            assert!(run_child.try_wait()?.is_none(), "run command exited");

            let status = engine
                .command("exec")
                .arg(&container_name)
                .arg("fedora")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
                .wait()?;

            if status.success() {
                break Ok(());
            }
        }
    })();

    // terminate the VM

    engine
        .command("stop")
        .arg(&container_name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    run_child.wait().unwrap();

    result.unwrap();
}

const BINARY_PATH: &str = env!("CARGO_BIN_EXE_crun-qemu");
const REPO_PATH: &str = env!("CARGO_MANIFEST_DIR");

fn get_random_container_name() -> String {
    format!("crun-qemu-test-{}", Uuid::new_v4())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Engine {
    Podman,
    Docker,
}

impl Engine {
    pub fn command(self, subcommand: &str) -> Command {
        let engine = match self {
            Engine::Podman => "podman",
            Engine::Docker => "docker",
        };

        let mut cmd = Command::new(engine);
        cmd.arg(subcommand);

        if subcommand == "run" {
            match self {
                Engine::Podman => {
                    cmd.arg(format!("--runtime={}", BINARY_PATH));
                }
                Engine::Docker => {
                    cmd.arg("--security-opt=label=disable");
                    cmd.arg("--runtime=crun-qemu");
                }
            }
        }

        cmd.env("RUST_BACKTRACE", "1");
        cmd
    }
}
