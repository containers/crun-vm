// SPDX-License-Identifier: GPL-2.0-or-later

use std::error::Error;
use std::fs::File;
use std::io;

use crate::util::crun;

pub fn exec(
    global_args: &liboci_cli::GlobalOpts,
    args: &mut liboci_cli::Exec,
) -> Result<(), Box<dyn Error>> {
    assert!(args.command.is_empty());

    let process_config_path = args.process.as_ref().expect("process config");
    let mut process: oci_spec::runtime::Process =
        serde_json::from_reader(File::open(process_config_path)?)?;

    let command = process.args().as_ref().expect("command specified");

    let ssh_user = command
        .first()
        .expect("first command argument is user to ssh as into the vm");

    let mut new_command = vec![];
    if ssh_user != "-" {
        new_command.extend([
            "ssh".to_string(),
            "-o".to_string(),
            "LogLevel=ERROR".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-l".to_string(),
            ssh_user.clone(),
            "localhost".to_string(),
        ]);
    }
    new_command.extend(command.iter().skip(1).cloned());

    process.set_args(Some(new_command));

    serde_json::to_writer(File::create(process_config_path)?, &process)?;

    crun_exec(global_args, args)?;

    Ok(())
}

fn crun_exec(global_args: &liboci_cli::GlobalOpts, args: &liboci_cli::Exec) -> io::Result<()> {
    // build crun argument list

    let mut arg_list = Vec::<String>::new();

    if global_args.debug {
        arg_list.push("--debug".to_string());
    }

    if let Some(path) = &global_args.log {
        arg_list.push("--log".to_string());
        arg_list.push(path.to_str().expect("path is utf-8").to_string());
    }

    if let Some(format) = &global_args.log_format {
        arg_list.push("--log-format".to_string());
        arg_list.push(format.clone());
    }

    if let Some(path) = &global_args.root {
        arg_list.push("--root".to_string());
        arg_list.push(path.to_str().expect("path is utf-8").to_string());
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
        arg_list.push(path.to_str().expect("path is utf-8").to_string());
    }

    if let Some(cwd) = &args.cwd {
        arg_list.push("--cwd".to_string());
        arg_list.push(cwd.to_str().expect("path is utf-8").to_string());
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
        arg_list.push(path.to_str().expect("path is utf-8").to_string());
    }

    if let Some(label) = &args.process_label {
        arg_list.push("--process-label".to_string());
        arg_list.push(label.clone());
    }

    if let Some(path) = &args.pid_file {
        arg_list.push("--pid-file".to_string());
        arg_list.push(path.to_str().expect("path is utf-8").to_string());
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
