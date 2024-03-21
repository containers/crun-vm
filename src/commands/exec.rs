// SPDX-License-Identifier: GPL-2.0-or-later

use std::{
    fs::File,
    io::{BufReader, BufWriter},
};

use anyhow::Result;
use clap::Parser;

use crate::crun::crun_exec;

pub fn exec(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Exec) -> Result<()> {
    assert!(args.command.is_empty());

    let process_config_path = args.process.as_ref().expect("process config");
    let mut process: oci_spec::runtime::Process =
        serde_json::from_reader(File::open(process_config_path).map(BufReader::new)?)?;

    let command = process.args().as_ref().expect("command specified");

    let new_command = build_command(command);
    process.set_args(Some(new_command));

    serde_json::to_writer(
        File::create(process_config_path).map(BufWriter::new)?,
        &process,
    )?;

    crun_exec(global_args, args)?;

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(no_binary_name = true, disable_help_flag = true)]
struct ExecArgs {
    #[clap(long = "as", default_value = "root")]
    user: String,

    #[clap(long, conflicts_with = "user")]
    container: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

fn build_command(original_command: &Vec<String>) -> Vec<String> {
    let mut args: ExecArgs = ExecArgs::parse_from(original_command);

    if args.command.starts_with(&["".to_string()]) {
        args.command.remove(0);
    }

    if args.container {
        if args.command.is_empty() {
            vec!["/bin/bash".to_string()]
        } else {
            args.command
        }
    } else {
        ["/crun-vm/exec.sh".to_string(), args.user]
            .into_iter()
            .chain(args.command)
            .collect()
    }
}
