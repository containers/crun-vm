// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::OsStr;
use std::process::Command;

use anyhow::{ensure, Result};

use crate::util::PathExt;

/// Run `crun`.
///
/// `crun` will inherit this process' standard streams.
///
/// TODO: It may be better to use libcrun directly, although its public API purportedly isn't in
/// great shape: https://github.com/containers/crun/issues/1018
pub fn crun<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new("crun").args(args).spawn()?.wait()?;

    ensure!(status.success(), "crun failed");

    Ok(())
}

pub fn crun_create(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Create) -> Result<()> {
    // build crun argument list

    let mut arg_list = Vec::<String>::new();
    let mut arg = |arg: &str| {
        arg_list.push(arg.to_string());
    };

    if global_args.debug {
        arg("--debug");
    }

    if let Some(path) = &global_args.log {
        arg("--log");
        arg(path.as_str());
    }

    if let Some(format) = &global_args.log_format {
        arg("--log-format");
        arg(format);
    }

    if args.no_pivot {
        arg("--no-pivot");
    }

    if let Some(path) = &global_args.root {
        arg("--root");
        arg(path.as_str());
    }

    if global_args.systemd_cgroup {
        arg("--systemd-cgroup");
    }

    arg("create");

    arg("--bundle");
    arg(args.bundle.as_str());

    if let Some(path) = &args.console_socket {
        arg("--console-socket");
        arg(path.as_str());
    }

    if args.no_new_keyring {
        arg("--no-new-keyring");
    }

    arg("--preserve-fds");
    arg(&args.preserve_fds.to_string());

    if let Some(path) = &args.pid_file {
        arg("--pid-file");
        arg(path.as_str());
    }

    arg(&args.container_id);

    // run crun

    crun(arg_list)
}

pub fn crun_exec(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Exec) -> Result<()> {
    // build crun argument list

    let mut arg_list = Vec::<String>::new();
    let mut arg = |arg: &str| {
        arg_list.push(arg.to_string());
    };

    if global_args.debug {
        arg("--debug");
    }

    if let Some(path) = &global_args.log {
        arg("--log");
        arg(path.as_str());
    }

    if let Some(format) = &global_args.log_format {
        arg("--log-format");
        arg(format);
    }

    if let Some(path) = &global_args.root {
        arg("--root");
        arg(path.as_str());
    }

    if global_args.systemd_cgroup {
        arg("--systemd-cgroup");
    }

    arg("exec");

    if let Some(profile) = &args.apparmor {
        arg("--apparmor");
        arg(profile);
    }

    if let Some(path) = &args.console_socket {
        arg("--console-socket");
        arg(path.as_str());
    }

    if let Some(cwd) = &args.cwd {
        arg("--cwd");
        arg(cwd.as_str());
    }

    for cap in &args.cap {
        arg("--cap");
        arg(cap);
    }

    if args.detach {
        arg("--detach");
    }

    if let Some(path) = &args.cgroup {
        arg("--cgroup");
        arg(path);
    }

    for (name, value) in &args.env {
        arg("--env");
        arg(&format!("{name}={value}"));
    }

    if args.no_new_privs {
        arg("--no-new-privs");
    }

    arg("--preserve-fds");
    arg(&args.preserve_fds.to_string());

    if let Some(path) = &args.process {
        arg("--process");
        arg(path.as_str());
    }

    if let Some(label) = &args.process_label {
        arg("--process-label");
        arg(label);
    }

    if let Some(path) = &args.pid_file {
        arg("--pid-file");
        arg(path.as_str());
    }

    if args.tty {
        arg("--tty");
    }

    if let Some((uid, gid)) = &args.user {
        arg("--user");
        arg(&match gid {
            Some(gid) => format!("{uid}:{gid}"),
            None => format!("{uid}"),
        });
    }

    arg(&args.container_id);

    arg_list.extend(args.command.iter().cloned());

    // run crun

    crun(arg_list)
}
