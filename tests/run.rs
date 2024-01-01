// SPDX-License-Identifier: GPL-2.0-or-later

#![allow(clippy::items_after_test_module)]

use std::env;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use test_case::test_matrix;
use uuid::Uuid;

fn simple_test_case(image: &str, home_dir: &str) -> TestCase {
    let exec_user = Path::new(home_dir)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    TestCase {
        run_args: vec![image.to_string(), "".to_string()],
        exec_user,
        test_script: "".to_string(),
    }
}

fn complex_test_case(
    image: &str,
    home_dir: &str,
    cloud_init_and_ignition_prefix: &str,
) -> TestCase {
    let exec_user = Path::new(home_dir)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let cloud_init_and_ignition_prefix = match cloud_init_and_ignition_prefix {
        "" => "".to_string(),
        prefix => format!("{prefix}/"),
    };

    TestCase {
        run_args: vec![
            "-h=my-test-vm".to_string(),
            format!("-v=./util:{home_dir}/util"),
            format!("-v=./README.md:{home_dir}/README.md:z,ro"), // "ro" is so qemu uses shared lock
            format!("--mount=type=tmpfs,dst={home_dir}/tmp"),
            image.to_string(),
            format!("--cloud-init={cloud_init_and_ignition_prefix}examples/cloud-init/config"),
            format!("--ignition={cloud_init_and_ignition_prefix}examples/ignition/config.ign"),
        ],
        exec_user,
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
        simple_test_case("quay.io/crun-qemu/example-fedora-coreos:39", "/var/home/core"),

        complex_test_case("quay.io/containerdisks/fedora:39", "/home/fedora", REPO_PATH),
        complex_test_case("quay.io/crun-qemu/example-fedora-coreos:39", "/var/home/core", REPO_PATH),
    ]
)]
#[test_matrix(
    // engines
    [
        Engine::Podman,
    ],

    // cases
    [
        complex_test_case("quay.io/containerdisks/fedora:39", "/home/fedora", ""),
        complex_test_case("quay.io/crun-qemu/example-fedora-coreos:39", "/var/home/core", ""),
    ]
)]
fn test_run(engine: Engine, case: TestCase) {
    env::set_var("RUST_BACKTRACE", "1");

    let container_name = get_random_container_name();

    // launch VM

    let mut run_child: std::process::Child = engine
        .command("run")
        .arg(format!("--name={}", container_name))
        .arg("--rm")
        .args(&case.run_args)
        .spawn()
        .unwrap();

    // wait until we can exec into the VM

    let result = (|| -> Result<()> {
        loop {
            assert!(run_child.try_wait()?.is_none(), "run command exited");

            let status = engine
                .command("exec")
                .arg(&container_name)
                .arg(&case.exec_user)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
                .wait()?;

            if status.success() {
                break;
            }
        }

        thread::sleep(Duration::from_secs(3)); // work around some flakiness

        // run the test script

        let mut exec_child = engine
            .command("exec")
            .arg("-i")
            .arg(&container_name)
            .arg(&case.exec_user)
            .arg("bash")
            .arg("-s")
            .stdin(Stdio::piped())
            .spawn()?;

        {
            let mut writer = BufWriter::new(exec_child.stdin.take().unwrap());
            writer.write_all("set -e\n".as_bytes())?;
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

    let status = run_child.wait().unwrap();
    assert_eq!(status.code(), Some(143)); // SIGTERM

    result.unwrap();
}

const BINARY_PATH: &str = env!("CARGO_BIN_EXE_crun-qemu");
const REPO_PATH: &str = env!("CARGO_MANIFEST_DIR");

struct TestCase {
    run_args: Vec<String>,
    exec_user: String,
    test_script: String,
}

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
