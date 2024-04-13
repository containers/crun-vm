// SPDX-License-Identifier: GPL-2.0-or-later

#![allow(clippy::items_after_test_module)]

use std::env;
use std::io::{BufWriter, Write};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use camino::Utf8Path;
use test_case::test_matrix;
use uuid::Uuid;

fn simple_test_case(image: &str, home_dir: &str) -> TestCase {
    TestCase {
        run_args: vec![image.to_string(), "".to_string()],
        exec_user: Utf8Path::new(home_dir).file_name().unwrap().to_string(),
        test_script: "".to_string(),
    }
}

fn complex_test_case(image: &str, home_dir: &str) -> TestCase {
    TestCase {
        run_args: vec![
            "-h=my-test-vm".to_string(),
            format!("-v=./util:{home_dir}/util"),
            format!("-v=./README.md:{home_dir}/README.md:z,ro"), // "ro" is so qemu uses shared lock
            format!("--mount=type=tmpfs,dst={home_dir}/tmp"),
            image.to_string(),
            format!("--cloud-init={REPO_PATH}/examples/cloud-init/config"),
            format!("--ignition={REPO_PATH}/examples/ignition/config.ign"),
        ],
        exec_user: Utf8Path::new(home_dir).file_name().unwrap().to_string(),
        test_script: format!(
            "
            mount -l | grep '^virtiofs-0 on {home_dir}/util type virtiofs'
            mount -l | grep '^tmpfs on {home_dir}/tmp type tmpfs'
            [[ -b ~/README.md ]]
            sudo grep 'This project is released under' ~/README.md
            "
        ),
    }
}

#[test_matrix(
    // engines
    [
        Engine::Podman,
        Engine::Docker,
    ],

    // cases
    [
        simple_test_case("quay.io/containerdisks/fedora:39", "/home/fedora"),
        simple_test_case("quay.io/crun-vm/example-fedora-coreos:39", "/var/home/core"),

        complex_test_case("quay.io/containerdisks/fedora:39", "/home/fedora"),
        complex_test_case("quay.io/crun-vm/example-fedora-coreos:39", "/var/home/core"),
    ]
)]
fn test_run(engine: Engine, case: TestCase) {
    env::set_var("RUST_BACKTRACE", "1");

    let container_name = get_random_container_name();

    // launch VM

    let status = engine
        .command("run")
        .arg(format!("--name={}", container_name))
        .arg("--rm")
        .arg("--detach")
        .args(&case.run_args)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    assert!(status.success());

    // run the test script

    let result = (|| -> Result<()> {
        let mut exec_child = engine
            .command("exec")
            .arg("-i")
            .arg(&container_name)
            .arg("--as")
            .arg(&case.exec_user)
            .arg("bash")
            .arg("-s")
            .stdin(Stdio::piped())
            .spawn()?;

        {
            let mut writer = BufWriter::new(exec_child.stdin.take().unwrap());
            writer.write_all("set -ex\n".as_bytes())?;
            writer.write_all("! command -v cloud-init || cloud-init status --wait\n".as_bytes())?;
            writer.write_all(case.test_script.as_bytes())?;
            writer.write_all("\n".as_bytes())?;
            writer.flush()?;
            // stdin is closed when writer is dropped
        }

        match exec_child.wait()?.code().unwrap() {
            0 => Ok(()),
            n => Err(anyhow!("test script failed with exit code {n}")),
        }
    })();

    // terminate the VM

    let status = engine
        .command("stop")
        .arg(&container_name)
        .stdin(Stdio::null())
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    assert!(status.success());

    result.unwrap();
}

const BINARY_PATH: &str = env!("CARGO_BIN_EXE_crun-vm");
const REPO_PATH: &str = env!("CARGO_MANIFEST_DIR");

struct TestCase {
    run_args: Vec<String>,
    exec_user: String,
    test_script: String,
}

fn get_random_container_name() -> String {
    format!("crun-vm-test-{}", Uuid::new_v4())
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
                Engine::Podman => cmd.arg(format!("--runtime={}", BINARY_PATH)),
                Engine::Docker => cmd.arg("--runtime=crun-vm"),
            };
        }

        cmd.env("RUST_BACKTRACE", "1");
        cmd
    }
}
