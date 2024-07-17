# 3. Running VMs as **systemd** services

crun-vm also enables you to define a [systemd] service corresponding to a VM,
and thus manage it through systemd. This relies on Podman's [Quadlet] feature,
through which you can define systemd unit files for containers.

> [!TIP]
>
> This means that system **containers** and **VMs** can both be deployed and
> managed **using the same tooling**, *i.e.*, systemd!

Here, we overview how you can create Quadlet-powered systemd services to manage
VMs. Make sure you have installed both crun-vm and Podman (see [1. Installing
crun-vm]).

<details open>
  <summary><b>Navigation</b></summary>

  1. [Installing crun-vm](1-installing.md)
  2. [Running VMs with **Podman** or **Docker**](2-podman-docker.md)
  3. **Running VMs as **systemd** services**
     - [**Creating a systemd service for a VM**](#creating-a-systemd-service-for-a-vm)
     - [**Further information**](#further-information)
  4. [Running VMs in **Kubernetes**](4-kubernetes.md)
  5. [**crun-vm(1)** man page](5-crun-vm.1.ronn)

</details>

## Creating a systemd service for a VM

The easiest way to do this is using [Podlet], a tool that can generate a systemd
unit file corresponding to a given podman-run command. (Follow the instructions
at https://github.com/containers/podlet to install Podlet.) This means we can
apply it to the podman-run commands we use to launch VMs.

For instance, say you're using this command to launch a VM that runs a web
service (see [2. Running VMs with **Podman** or **Docker**] to learn how crun-vm
can be used with Podman):

```console
$ podman run --runtime crun-vm --detach -p 8000:80 quay.io/crun-vm/example-http-server:latest
```

To convert this invocation into an equivalent systemd container unit definition,
you would run:

```console
$ podlet \
    --install \
    --wanted-by default.target \
    podman run --runtime crun-vm --detach -p 8000:80 quay.io/crun-vm/example-http-server:latest
#example-http-server.container
[Container]
Image=quay.io/crun-vm/example-http-server:latest
PublishPort=8000:80
GlobalArgs=--runtime crun-vm

[Install]
WantedBy=default.target
```

The `--install`, `--wanted-by default.target` options configure the service to
run automatically on boot.

Finally, to actually install this unit definition, you would instead run (using
`sudo` to become root):

```console
$ sudo podlet \
    --name my-web-service \
    --unit-directory \
    --install \
    --wanted-by default.target \
    podman run --runtime crun-vm --detach -p 8000:80 quay.io/crun-vm/example-http-server:latest
Wrote to file: /etc/containers/systemd/my-web-service.container

$ systemctl daemon-reload  # load the new service
```

With this, your web server VM becomes a systemd service:

```console
$ sudo systemctl status my-web-service
○ my-web-service.service
     Loaded: loaded (/etc/containers/systemd/my-web-service.container; generated)
    Drop-In: /usr/lib/systemd/system/service.d
             └─10-timeout-abort.conf
     Active: inactive (dead)

$ sudo systemctl start my-web-service  # start the service without having to reboot

$ sudo systemctl status my-web-service
● my-web-service.service
     Loaded: loaded (/etc/containers/systemd/my-web-service.container; generated)
    Drop-In: /usr/lib/systemd/system/service.d
             └─10-timeout-abort.conf
     Active: active (running) since Tue 2024-04-30 21:14:36 WEST; 4s ago
   Main PID: 1531707 (conmon)
      Tasks: 48 (limit: 76805)
     Memory: 1.1G (peak: 1.1G)
        CPU: 11.768s
[...]

$ curl localhost:8000
<!DOCTYPE HTML>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Directory listing for /</title>
</head>
<body>
[...]
```

## Further information

See [this article] for additional information on Podman Quadlet, and the
[podman-systemd.unit(5)] man page for the reference format of container unit
files.

The `podlet` commands provides several options to further customize the
generated container unit file. Run `podlet -h` to know more.

[1. Installing crun-vm]: 1-installing.md
[2. Running VMs with **Podman** or **Docker**]: 2-podman-docker.md
[Podlet]: https://github.com/containers/podlet
[podman-systemd.unit(5)]: https://docs.podman.io/en/stable/markdown/podman-systemd.unit.5.html
[Quadlet]: https://docs.podman.io/en/stable/markdown/podman-systemd.unit.5.html
[systemd]: https://systemd.io/
[this article]: https://www.redhat.com/sysadmin/quadlet-podman
