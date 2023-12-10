// SPDX-License-Identifier: GPL-3.0-only

use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    crun_qemu::main(env::args_os().skip(1))
}
