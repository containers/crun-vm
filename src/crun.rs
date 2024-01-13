// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::{OsStr, OsString};
use std::process::Command;

use anyhow::{ensure, Result};

/// Run `crun`.
///
/// `crun` will inherit this process' standard streams.
///
/// TODO: It may be better to use libcrun directly, although its public API purportedly isn't in
/// great shape: https://github.com/containers/crun/issues/1018
pub fn crun(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<()> {
    let status = Command::new("crun").args(args).spawn()?.wait()?;
    ensure!(status.success(), "crun failed");

    Ok(())
}

pub fn crun_create(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Create) -> Result<()> {
    let mut a = Vec::<OsString>::new();

    fn add(list: &mut Vec<OsString>, arg: impl AsRef<OsStr>) {
        list.push(arg.as_ref().to_os_string());
    }

    // build crun argument list

    if global_args.debug {
        add(&mut a, "--debug");
    }

    if let Some(path) = &global_args.log {
        add(&mut a, "--log");
        add(&mut a, path);
    }

    if let Some(format) = &global_args.log_format {
        add(&mut a, "--log-format");
        add(&mut a, format);
    }

    if args.no_pivot {
        add(&mut a, "--no-pivot");
    }

    if let Some(path) = &global_args.root {
        add(&mut a, "--root");
        add(&mut a, path);
    }

    if global_args.systemd_cgroup {
        add(&mut a, "--systemd-cgroup");
    }

    add(&mut a, "create");

    add(&mut a, "--bundle");
    add(&mut a, &args.bundle);

    if let Some(path) = &args.console_socket {
        add(&mut a, "--console-socket");
        add(&mut a, path);
    }

    if args.no_new_keyring {
        add(&mut a, "--no-new-keyring");
    }

    add(&mut a, "--preserve-fds");
    add(&mut a, args.preserve_fds.to_string());

    if let Some(path) = &args.pid_file {
        add(&mut a, "--pid-file");
        add(&mut a, path);
    }

    add(&mut a, &args.container_id);

    // run crun

    crun(a)
}

pub fn crun_exec(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Exec) -> Result<()> {
    let mut a = Vec::<OsString>::new();

    fn add(list: &mut Vec<OsString>, arg: impl AsRef<OsStr>) {
        list.push(arg.as_ref().to_os_string());
    }

    // build crun argument list

    if global_args.debug {
        add(&mut a, "--debug");
    }

    if let Some(path) = &global_args.log {
        add(&mut a, "--log");
        add(&mut a, path);
    }

    if let Some(format) = &global_args.log_format {
        add(&mut a, "--log-format");
        add(&mut a, format);
    }

    if let Some(path) = &global_args.root {
        add(&mut a, "--root");
        add(&mut a, path);
    }

    if global_args.systemd_cgroup {
        add(&mut a, "--systemd-cgroup");
    }

    add(&mut a, "exec");

    if let Some(profile) = &args.apparmor {
        add(&mut a, "--apparmor");
        add(&mut a, profile);
    }

    if let Some(path) = &args.console_socket {
        add(&mut a, "--console-socket");
        add(&mut a, path);
    }

    if let Some(cwd) = &args.cwd {
        add(&mut a, "--cwd");
        add(&mut a, cwd);
    }

    for cap in &args.cap {
        add(&mut a, "--cap");
        add(&mut a, cap);
    }

    if args.detach {
        add(&mut a, "--detach");
    }

    if let Some(path) = &args.cgroup {
        add(&mut a, "--cgroup");
        add(&mut a, path);
    }

    for (name, value) in &args.env {
        add(&mut a, "--env");
        add(&mut a, format!("{name}={value}"));
    }

    if args.no_new_privs {
        add(&mut a, "--no-new-privs");
    }

    add(&mut a, "--preserve-fds");
    add(&mut a, args.preserve_fds.to_string());

    if let Some(path) = &args.process {
        add(&mut a, "--process");
        add(&mut a, path);
    }

    if let Some(label) = &args.process_label {
        add(&mut a, "--process-label");
        add(&mut a, label);
    }

    if let Some(path) = &args.pid_file {
        add(&mut a, "--pid-file");
        add(&mut a, path);
    }

    if args.tty {
        add(&mut a, "--tty");
    }

    if let Some((uid, gid)) = &args.user {
        add(&mut a, "--user");
        add(
            &mut a,
            match gid {
                Some(gid) => format!("{uid}:{gid}"),
                None => format!("{uid}"),
            },
        );
    }

    add(&mut a, &args.container_id);

    a.extend(args.command.iter().map(Into::into));

    // run crun

    crun(a)
}
