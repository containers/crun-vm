// SPDX-License-Identifier: GPL-2.0-or-later

use std::env;

use anyhow::Result;

fn main() -> Result<()> {
    crun_qemu::main(env::args_os().skip(1))
}
