// SPDX-License-Identifier: GPL-2.0-or-later

use std::env;
use std::process;

fn main() {
    if let Err(e) = crun_vm::main(env::args_os().skip(1)) {
        eprintln!("{:#}", e);

        let rust_backtrace = env::var_os("RUST_BACKTRACE").unwrap_or_default();
        let rust_lib_backtrace = env::var_os("RUST_LIB_BACKTRACE").unwrap_or_default();

        if (rust_backtrace == "1" || rust_lib_backtrace == "1") && rust_lib_backtrace != "0" {
            eprintln!("\n{}\n", e.backtrace());
        }

        process::exit(1);
    }
}
