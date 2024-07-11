// SPDX-License-Identifier: GPL-2.0-or-later

use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter};

use anyhow::{bail, Result};
use clap::Parser;

use crate::util::{crun, fix_selinux_label};

pub fn exec(args: &liboci_cli::Exec, raw_args: &[impl AsRef<OsStr>]) -> Result<()> {
    assert!(args.command.is_empty());

    // load exec process config

    let process_config_path = args.process.as_ref().expect("process config");
    let mut process: oci_spec::runtime::Process =
        serde_json::from_reader(File::open(process_config_path).map(BufReader::new)?)?;

    let command = process.args().as_ref().expect("command specified");

    let new_command = build_command(command)?;
    process.set_args(Some(new_command));

    fix_selinux_label(&mut process);

    // store modified exec process config

    serde_json::to_writer(
        File::create(process_config_path).map(BufWriter::new)?,
        &process,
    )?;

    // actually exec

    crun(raw_args)?;

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(no_binary_name = true, disable_help_flag = true)]
struct ExecArgs {
    #[clap(long = "as", default_value = "root")]
    user: String,

    #[clap(long, conflicts_with = "user")]
    container: bool,

    #[clap(long = "timeout")]
    timeout: Option<u32>,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

fn build_command(original_command: &Vec<String>) -> Result<Vec<String>> {
    let mut args: ExecArgs = ExecArgs::parse_from(original_command);

    let timeout = if let Some(t) = args.timeout {
        t
    } else if let Some(t) = env::var_os("CRUN_VM_EXEC_TIMEOUT") {
        match t.to_str().and_then(|t| t.parse().ok()) {
            Some(t) => t,
            None => bail!("env var CRUN_VM_EXEC_TIMEOUT has invalid value"),
        }
    } else {
        0
    };

    if args.command.starts_with(&["".to_string()]) {
        args.command.remove(0);
    }

    let command = if args.container {
        if args.command.is_empty() {
            vec!["/bin/bash".to_string()]
        } else {
            args.command
        }
    } else {
        [
            "/crun-vm/exec.sh".to_string(),
            timeout.to_string(),
            args.user,
        ]
        .into_iter()
        .chain(args.command)
        .collect()
    };

    Ok(command)
}
