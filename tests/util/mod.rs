// SPDX-License-Identifier: GPL-2.0-or-later

use std::io::{self, Write};
use std::process::{Command, Output};

pub const BINARY_PATH: &str = env!("CARGO_BIN_EXE_crun-qemu");
pub const REPO_PATH: &str = env!("CARGO_MANIFEST_DIR");

pub fn expect_success(output: Output) {
    if !output.status.success() {
        io::stdout().write_all(&output.stdout).unwrap();
        io::stdout().write_all(&output.stderr).unwrap();
    }

    assert!(output.status.success());
}

pub fn expect_failure(output: Output) {
    if output.status.success() {
        io::stdout().write_all(&output.stdout).unwrap();
        io::stdout().write_all(&output.stderr).unwrap();
    }

    assert!(!output.status.success());
}

#[must_use]
pub fn podman<'a>(args: impl IntoIterator<Item = &'a str>) -> Output {
    Command::new("podman")
        .arg("--log-level=debug")
        .arg(format!("--runtime={}", BINARY_PATH))
        .args(args)
        .env("RUST_BACKTRACE", "1")
        .output()
        .unwrap()
}

#[must_use]
pub fn podman_run<'a>(args: impl IntoIterator<Item = &'a str>) -> Output {
    podman(
        ["run", "--security-opt=label=disable"]
            .into_iter()
            .chain(args),
    )
}
