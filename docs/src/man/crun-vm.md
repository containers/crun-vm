crun-vm 1 "User Commands"
==================================================

# NAME

crun-vm - Run VMs as easily as you run containers

# SYNOPSIS

**crun-vm** \[OPTIONS\] \<COMMAND\>

# DESCRIPTION

crun-vm is an OCI Runtime that enables Podman, Docker, and Kubernetes to run QEMU-compatible Virtual Machine (VM) images.

# COMMANDS

  create      Create a container

  start       Start a previously created container

  state       Show the container state

  kill        Send the specified signal to the container

  delete      Release any resources held by the container

  checkpoint  Checkpoint a running container

  events      Show resource statistics for the container

  exec        Execute a process within an existing container

  features    Return the features list for a container

  list        List created containers

  pause       Suspend the processes within the container

  ps          Display the processes inside the container

  resume      Resume the processes within the container

  run         Create a container and immediately start it

  update      Update running container resource constraints

  spec        Command generates a config.json

  help        Print this message or the help of the given subcommand(s)

# OPTIONS

  -l, --log \<LOG\>                set the log file to write youki logs to (default is '/dev/stderr')

        --debug                    change log level to debug, but the `log-level` flag takes precedence

        --log-format <LOG_FORMAT>  set the log format ('text' (default), or 'json') (default: "text")

  -r, --root \<ROOT\>              root directory to store container state

  -s, --systemd-cgroup           Enable systemd cgroup manager, rather then use the cgroupfs directly

  -h, --help                     Print help

