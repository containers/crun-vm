// SPDX-License-Identifier: GPL-2.0-or-later

mod commands;
mod crun;
mod util;

use std::ffi::OsStr;

use anyhow::Result;
use clap::Parser;
use crun::crun;

// Adapted from https://github.com/containers/youki/blob/main/crates/youki/src/main.rs
#[derive(Parser, Debug)]
struct Args {
    #[clap(flatten)]
    global: liboci_cli::GlobalOpts,

    #[clap(subcommand)]
    command: Command,
}

// Adapted from https://github.com/containers/youki/blob/main/crates/youki/src/main.rs
#[derive(Parser, Debug)]
#[clap(no_binary_name = true)]
enum Command {
    #[clap(flatten)]
    Standard(Box<liboci_cli::StandardCmd>),

    #[clap(flatten)]
    Common(Box<liboci_cli::CommonCmd>),
}

pub fn main(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<()> {
    let args = args
        .into_iter()
        .map(|a| a.as_ref().to_os_string())
        .collect::<Vec<_>>();

    let parsed_args = Args::parse_from(&args);

    match parsed_args.command {
        Command::Standard(cmd) => {
            if let liboci_cli::StandardCmd::Create(create_args) = *cmd {
                return commands::create::create(&parsed_args.global, &create_args);
            }
        }
        Command::Common(cmd) => {
            if let liboci_cli::CommonCmd::Exec(exec_args) = *cmd {
                return commands::exec::exec(&parsed_args.global, &exec_args);
            }
        }
    }

    // not a command we implement ourselves, just pass it on to crun
    crun(&args)
}
