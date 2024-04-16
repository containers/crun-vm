// SPDX-License-Identifier: GPL-2.0-or-later

mod commands;
mod util;

use std::ffi::OsStr;

use anyhow::{bail, Result};
use clap::Parser;
use util::crun;

// Adapted from https://github.com/containers/youki/blob/main/crates/youki/src/main.rs
#[derive(Parser, Debug)]
#[clap(no_binary_name = true)]
struct Args {
    #[clap(flatten)]
    global: liboci_cli::GlobalOpts,

    #[clap(subcommand)]
    command: Command,
}

// Adapted from https://github.com/containers/youki/blob/main/crates/youki/src/main.rs
#[derive(Parser, Debug)]
enum Command {
    #[clap(flatten)]
    Standard(Box<liboci_cli::StandardCmd>),

    #[clap(flatten)]
    Common(Box<liboci_cli::CommonCmd>),
}

pub fn main(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<()> {
    let raw_args = args
        .into_iter()
        .map(|a| a.as_ref().to_os_string())
        .collect::<Vec<_>>();

    let parsed_args = Args::parse_from(&raw_args);

    match parsed_args.command {
        Command::Standard(cmd) => {
            match *cmd {
                liboci_cli::StandardCmd::Create(args) => commands::create::create(&args, &raw_args),
                liboci_cli::StandardCmd::Delete(args) => commands::delete::delete(&args, &raw_args),
                liboci_cli::StandardCmd::Start(_)
                | liboci_cli::StandardCmd::State(_)
                | liboci_cli::StandardCmd::Kill(_) => {
                    // not a command we implement ourselves, pass it on to crun
                    crun(&raw_args)
                }
            }
        }
        Command::Common(cmd) => {
            match *cmd {
                liboci_cli::CommonCmd::Exec(args) => commands::exec::exec(&args, &raw_args),
                liboci_cli::CommonCmd::Checkpointt(_)
                | liboci_cli::CommonCmd::Events(_)
                | liboci_cli::CommonCmd::Features(_)
                | liboci_cli::CommonCmd::List(_)
                | liboci_cli::CommonCmd::Pause(_)
                | liboci_cli::CommonCmd::Ps(_)
                | liboci_cli::CommonCmd::Resume(_)
                | liboci_cli::CommonCmd::Run(_)
                | liboci_cli::CommonCmd::Update(_)
                | liboci_cli::CommonCmd::Spec(_) => {
                    // not a command we support
                    bail!("Unknown command")
                }
            }
        }
    }
}
