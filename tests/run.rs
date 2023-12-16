// SPDX-License-Identifier: GPL-2.0-or-later

mod util;

use util::{expect_failure, expect_success, podman, podman_run, REPO_PATH};

#[test]
fn podman_run_raw() {
    let container_name = "crun-qemu-test-podman_run_raw";

    expect_success(podman_run([
        "--name",
        container_name,
        "-dit",
        "quay.io/kubevirt/alpine-container-disk-demo",
        "unused",
    ]));

    expect_success(podman(["rm", "--force", "--time=0", container_name]));
}

#[test]
fn podman_run_qcow2() {
    let container_name = "crun-qemu-test-podman_run_qcow2";

    expect_success(podman_run([
        "--name",
        container_name,
        "-dit",
        "quay.io/containerdisks/fedora:39",
        "unused",
    ]));

    expect_success(podman(["rm", "--force", "--time=0", container_name]));
}

#[test]
fn podman_run_invalid() {
    let container_name = "crun-qemu-test-podman_run_invalid";

    let output = podman_run(["--name", container_name, "-dit", "fedora:39", "unused"]);

    let _ = podman(["rm", "--force", "--time=0", container_name]);

    expect_failure(output);
}

#[test]
fn podman_run_mounts() {
    let container_name = "crun-qemu-test-podman_run_mounts";

    expect_success(podman_run([
        "--name",
        container_name,
        "-dit",
        &format!("-v={}/examples/cloud-init/config:/cloud-init", REPO_PATH),
        &format!("-v={}/util:/home/fedora/util", REPO_PATH),
        "quay.io/containerdisks/fedora:39",
        "unused",
    ]));

    expect_success(podman(["rm", "--force", "--time=0", container_name]));
}
