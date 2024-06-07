// SPDX-License-Identifier: GPL-2.0-or-later

use std::process::Command;

fn main() {
    // Ensure selinux is linked
    println!("cargo:rustc-link-lib=selinux");

    // Define the input markdown file and the output man page file
    let input = "docs/src/man/crun-vm.md";
    let output = "docs/src/man/crun-vm.1";

    // Run the go-md2man command
    let status = Command::new("go-md2man")
        .arg("-in")
        .arg(input)
        .arg("-out")
        .arg(output)
        .status()
        .expect("Failed to run go-md2man");

    if !status.success() {
        panic!("go-md2man failed to generate the man page");
    } else {
        println!("Generated man page at {}", output);
    }
}
