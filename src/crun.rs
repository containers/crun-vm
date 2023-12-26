// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::OsStr;
use std::io;
use std::process::Command;

use crate::util::PathExt;

/// Run `crun`.
///
/// `crun` will inherit this process' standard streams.
///
/// TODO: It may be better to use libcrun directly, although its public API purportedly isn't in
/// great shape: https://github.com/containers/crun/issues/1018
pub fn crun<I, S>(args: I) -> io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new("crun").args(args).spawn()?.wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("crun failed"))
    }
}

pub fn crun_create(
    global_args: &liboci_cli::GlobalOpts,
    args: &liboci_cli::Create,
) -> io::Result<()> {
    // build crun argument list

    let mut arg_list = Vec::new();

    if global_args.debug {
        arg_list.push("--debug");
    }

    if let Some(path) = &global_args.log {
        arg_list.push("--log");
        arg_list.push(path.as_str());
    }

    if let Some(format) = &global_args.log_format {
        arg_list.push("--log-format");
        arg_list.push(format);
    }

    if args.no_pivot {
        arg_list.push("--no-pivot");
    }

    if let Some(path) = &global_args.root {
        arg_list.push("--root");
        arg_list.push(path.as_str());
    }

    if global_args.systemd_cgroup {
        arg_list.push("--systemd-cgroup");
    }

    arg_list.push("create");

    arg_list.push("--bundle");
    arg_list.push(args.bundle.as_str());

    if let Some(path) = &args.console_socket {
        arg_list.push("--console-socket");
        arg_list.push(path.as_str());
    }

    if args.no_new_keyring {
        arg_list.push("--no-new-keyring");
    }

    arg_list.push("--preserve-fds");
    let preserve_fds = args.preserve_fds.to_string();
    arg_list.push(&preserve_fds);

    if let Some(path) = &args.pid_file {
        arg_list.push("--pid-file");
        arg_list.push(path.as_str());
    }

    arg_list.push(&args.container_id);

    // run crun

    crun(arg_list)
}

pub fn crun_exec(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Exec) -> io::Result<()> {
    // build crun argument list

    let mut arg_list = Vec::<String>::new();

    if global_args.debug {
        arg_list.push("--debug".to_string());
    }

    if let Some(path) = &global_args.log {
        arg_list.push("--log".to_string());
        arg_list.push(path.as_string());
    }

    if let Some(format) = &global_args.log_format {
        arg_list.push("--log-format".to_string());
        arg_list.push(format.clone());
    }

    if let Some(path) = &global_args.root {
        arg_list.push("--root".to_string());
        arg_list.push(path.as_string());
    }

    if global_args.systemd_cgroup {
        arg_list.push("--systemd-cgroup".to_string());
    }

    arg_list.push("exec".to_string());

    if let Some(profile) = &args.apparmor {
        arg_list.push("--apparmor".to_string());
        arg_list.push(profile.clone());
    }

    if let Some(path) = &args.console_socket {
        arg_list.push("--console-socket".to_string());
        arg_list.push(path.as_string());
    }

    if let Some(cwd) = &args.cwd {
        arg_list.push("--cwd".to_string());
        arg_list.push(cwd.as_string());
    }

    for cap in &args.cap {
        arg_list.push("--cap".to_string());
        arg_list.push(cap.clone());
    }

    if args.detach {
        arg_list.push("--detach".to_string());
    }

    if let Some(path) = &args.cgroup {
        arg_list.push("--cgroup".to_string());
        arg_list.push(path.clone());
    }

    for (name, value) in &args.env {
        arg_list.push("--env".to_string());
        arg_list.push(format!("{name}={value}"));
    }

    if args.no_new_privs {
        arg_list.push("--no-new-privs".to_string());
    }

    arg_list.push("--preserve-fds".to_string());
    arg_list.push(args.preserve_fds.to_string());

    if let Some(path) = &args.process {
        arg_list.push("--process".to_string());
        arg_list.push(path.as_string());
    }

    if let Some(label) = &args.process_label {
        arg_list.push("--process-label".to_string());
        arg_list.push(label.clone());
    }

    if let Some(path) = &args.pid_file {
        arg_list.push("--pid-file".to_string());
        arg_list.push(path.as_string());
    }

    if args.tty {
        arg_list.push("--tty".to_string());
    }

    if let Some((uid, gid)) = &args.user {
        arg_list.push("--user".to_string());
        arg_list.push(match gid {
            Some(gid) => format!("{uid}:{gid}"),
            None => uid.to_string(),
        });
    }

    arg_list.push(args.container_id.clone());

    arg_list.extend(args.command.iter().cloned());

    // run crun

    crun(arg_list)
}
