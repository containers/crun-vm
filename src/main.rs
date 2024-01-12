// SPDX-License-Identifier: GPL-2.0-or-later

use std::env;
use std::process;

fn main() {
    if let Err(e) = crun_vm::main(env::args_os().skip(1)) {
        eprintln!("{:#}", e);
        process::exit(1);
    }
}
