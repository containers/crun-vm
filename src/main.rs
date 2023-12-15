// SPDX-License-Identifier: GPL-2.0-or-later

use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    crun_qemu::main(env::args_os().skip(1))
}
