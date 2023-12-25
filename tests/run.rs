// SPDX-License-Identifier: GPL-2.0-or-later

#![allow(clippy::items_after_test_module)]

use std::env;
use std::io;
use std::io::{BufWriter, Write};
use std::process::{Command, Stdio};

use test_case::test_matrix;
use uuid::Uuid;

#[test_matrix(
    // engines
    [
        Engine::Podman,
        Engine::Docker,
    ],

    // cases
    [
        TestCase {
            run_args: &[
                "quay.io/containerdisks/fedora:39",
                ""
            ],
            test_script: "",
        },
        TestCase {
            run_args: &[
                "-h=my-test-vm",
                "-v=./util:/home/fedora/util",
                "--mount=type=tmpfs,dst=/home/fedora/tmp",
                "quay.io/containerdisks/fedora:39",
                &format!("--cloud-init={REPO_PATH}/examples/cloud-init/config"),
                &format!("--ignition={REPO_PATH}/examples/ignition/config.ign"),
            ],
            test_script: "
                mount -l | grep '^virtiofs-0 on /home/fedora/util type virtiofs'
                mount -l | grep '^tmpfs on /home/fedora/tmp type tmpfs'
                ",
        },
    ]
)]
#[test_matrix(
    // engines
    [
        Engine::Podman,
    ],

    // cases
    [
        TestCase {
            run_args: &[
                "-h=my-test-vm",
                "-v=./util:/home/fedora/util",
                "--mount=type=tmpfs,dst=/home/fedora/tmp",
                "quay.io/containerdisks/fedora:39",
                "--cloud-init=examples/cloud-init/config",
                "--ignition=examples/ignition/config.ign",
            ],
            test_script: "
                mount -l | grep '^virtiofs-0 on /home/fedora/util type virtiofs'
                mount -l | grep '^tmpfs on /home/fedora/tmp type tmpfs'
                ",
        },
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
        .args(case.run_args)
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
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
                .wait()?;

            if status.success() {
                // run the test script

                let mut exec_child = engine
                    .command("exec")
                    .arg("-i")
                    .arg(&container_name)
                    .arg("fedora")
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

                break match exec_child.wait()?.code().unwrap() {
                    0 => Ok(()),
                    n => Err(io::Error::other(format!(
                        "test script failed with exit code {n}"
                    ))),
                };
            }
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

struct TestCase<'a> {
    run_args: &'a [&'a str],
    test_script: &'a str,
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
