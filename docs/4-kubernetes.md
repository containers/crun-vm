# 4. Running VMs in **Kubernetes**

crun-vm can also be used as a [Kubernetes] runtime, allowing you to run VMs as
regular pods.

Note that there is already a very featureful project, [KubeVirt], that enables
using Kubernetes to run VMs with great configurability. However, for simple use
cases, crun-vm may be a good fit.

<details open>
  <summary><b>Navigation</b></summary>

  1. [Installing crun-vm](1-installing.md)
  2. [Running VMs with **Podman** or **Docker**](2-podman-docker.md)
  3. [Running VMs as **systemd** services](3-systemd.md)
  4. **Running VMs in **Kubernetes****
     - [**Setting up**](#setting-up)
     - [**Creating VMs**](#creating-vms)
     - [**Interacting with VMs**](#interacting-with-vms)
       - [Exec'ing into VMs](#execing-into-vms)
       - [Port forwarding](#port-forwarding)
     - [**First-boot configuration**](#first-boot-configuration)
  5. [**crun-vm(1)** man page](5-crun-vm.1.ronn)

</details>

## Setting up

You can use the [util/minikube-start.sh] script to set up a local [minikube]
Kubernetes cluster with crun-vm available as a runtime, and use it to easily try
out the examples below (note that this also points `kubectl` at the minikube
cluster):

```console
$ ./minikube-start.sh
   Compiling crun-vm v0.2.0 (/home/afaria/crun-vm)
    Finished dev [unoptimized + debuginfo] target(s) in 1.70s
😄  [crun-vm-example] minikube v1.32.0 on Fedora 40
[...]
🏄  Done! kubectl is now configured to use "crun-vm-example" cluster and "default" namespace by default
runtimeclass.node.k8s.io/crun-vm created
```

Once you're done, you can delete the cluster with:

```console
$ minikube -p crun-vm-example delete
```

To enable crun-vm on a real Kubernetes cluster, follow the instructions in [1.
Installing crun-vm].

## Creating VMs

To run a VM in your cluster, simply create a pod that references the
`RuntimeClass` corresponding to crun-vm (here we assume it is named `crun-vm`):

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-vm
spec:
  containers:
    - name: my-vm
      image: quay.io/crun-vm/example-http-server:latest
      ports:
        - containerPort: 80
  runtimeClassName: crun-vm
```

All the image formats supported by crun-vm when using Podman or Docker are also
supported here. See [2. Running VMs with **Podman** or
**Docker**](2-podman-docker.md#booting-vms) to know more.

You can inspect the VM's console output with the standard [`kubectl logs`]
command:

```console
$ kubectl logs my-vm
```

## Interacting with VMs

### Exec'ing into VMs

Assuming a VM supports cloud-init or Ignition, you can `ssh` into it using
`kubectl exec`:

```console
$ kubectl exec my-vm -- --as fedora whoami
fedora

$ kubectl exec -it my-vm -- --as fedora bash
[fedora@my-vm ~]$
```

The supported crun-vm specific options like `--as fedora` are the same as when
using crun-vm with Podman or Docker. See [2. Running VMs with **Podman** or
**Docker**](2-podman-docker.md#execing-into-vms) to know more.

### Port forwarding

The VM pod defined above actually exposes an HTTP server on port 80. To talk to
it, we must first forward a local port to the VM:

```console
$ kubectl port-forward my-vm 8000:80
Forwarding from 127.0.0.1:8000 -> 80
Forwarding from [::1]:8000 -> 80
```

With this command running, navigate to [`localhost:8000`] on your browser, or
run the following on a second terminal:

```console
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

## First-boot configuration

Options supported when using crun-vm with Podman or Docker, like `--password`,
`--cloud-init`, and `--ignition`, are also supported here (see [2. Running VMs
with **Podman** or **Docker**](2-podman-docker.md#execing-into-vms) for more
information).

However, paths given to `--cloud-init` and `--ignition` are interpreted in the
context of the VM, instead of the host. This means that first-boot configuration
files can be retrieved from mounted volumes. For instance, you could store your
cloud-init configuration in a `ConfigMap`:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-cloud-init-config
data:
  meta-data: ""
  user-data: |
    #cloud-config
    runcmd:
      - echo 'Hello, world!' > /home/fedora/hello-world
```

And apply it to your VMs like so:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-other-vm
spec:
  containers:
    - name: my-other-vm
      image: quay.io/containerdisks/fedora:40
      args:
        - --cloud-init=/etc/cloud-init
      volumeMounts:
        - name: cloud-init-vol
          mountPath: /etc/cloud-init
  volumes:
    - name: cloud-init-vol
      configMap:
        name: my-cloud-init-config
  runtimeClassName: crun-vm
```

[`kubectl logs`]: https://kubernetes.io/docs/reference/kubectl/
[`localhost:8000`]: http://localhost:8000/
[1. Installing crun-vm]: 1-installing.md
[Kubernetes]: https://kubernetes.io/
[KubeVirt]: https://kubevirt.io/
[minikube]: https://minikube.sigs.k8s.io/
[util/minikube-start.sh]: ../util/minikube-start.sh
