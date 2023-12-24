# 3. Using `crun-qemu` as a Kuberntes runtime

It is possible to use `crun-qemu` as a [Kubernetes] runtime, allowing you to run
VMs as regular pods.

## Preparation

To enable `crun-qemu` on a Kubernetes cluster, follow these steps:

1. Ensure that the cluster is using the [CRI-O] container runtime. Refer to the
   Kubernetes docs on [container runtimes].

2. Install `crun-qemu` on all cluster nodes where pods may be scheduled. Refer
   to the [installation instructions].

3. Append the following to `/etc/crio/crio.conf` (adjust the `runtime_path` if
   necessary):

   ```toml
   [crio.runtime.runtimes.crun-qemu]
   runtime_path = "/usr/local/bin/crun-qemu"
   ```

4. Create a `RuntimeClass` that references `crun-qemu`:

   ```yaml
   apiVersion: node.k8s.io/v1
   kind: RuntimeClass
   metadata:
     name: crun-qemu
   handler: crun-qemu
   ```

## Using the runtime

> Under [examples/minikube] you can find a script that sets up a local minikube
> Kubernetes cluster with `crun-qemu` available as a runtime. You can use it to
> easily try out the examples below.

From then on, you can run VM images packaged in container images by creating
pods that use this `RuntimeClass`:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-vm
spec:
  containers:
    - name: my-vm
      image: quay.io/crun-qemu/example-http-server:latest
      args:
        - ""  # unused, but must specify command because container image does not
      ports:
        - containerPort: 80
  runtimeClassName: crun-qemu
```

### Logging

The VM's console output is logged:

```console
$ kubectl logs my-vm
```

### SSH'ing into the pod/VM

Assuming the VM supports cloud-init or Ignition, you can also SSH into it using
`kubectl exec`, with the caveat that the user to SSH as is passed in place of
the command (this is the same behavior as with `podman exec` or `docker exec`;
see [SSH'ing into the VM]):

```console
$ kubectl exec my-vm -- fedora whoami
fedora

$ kubectl exec -it my-vm -- fedora
[fedora@my-vm ~]$
```

### Port forwarding

The pod/VM defined above actually exposes an HTTP server on port 80. To talk to
it, we must first forward a local port to the pod/VM:

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

### cloud-init and Ignition

When using `crun-qemu` as a Kubernetes runtime, paths given to `--cloud-init`
and `--ignition` are interpreted in the context of the container/VM, instead of
the host. This means that config files can be retrieved from mounted volumes.
For instance, you could store your cloud-init config in a `ConfigMap`:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-cloud-init-config
data:
  user-data: |
    #cloud-config
    runcmd:
      - echo 'Hello, world!' > /home/fedora/hello-world
```

And pass it to your VMs like so:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-other-vm
spec:
  containers:
    - name: my-other-vm
      image: quay.io/containerdisks/fedora:39
      args:
        - --cloud-init=/etc/cloud-init
      volumeMounts:
        - name: cloud-init-vol
          mountPath: /etc/cloud-init
  volumes:
    - name: cloud-init-vol
      configMap:
        name: my-cloud-init-config
  runtimeClassName: crun-qemu
```

[container runtimes]: https://kubernetes.io/docs/setup/production-environment/container-runtimes/#cri-o
[CRI-O]: https://cri-o.io/
[examples/minikube]: /examples/minikube
[installation instructions]: 1-installing.md
[Kubernetes]: https://kubernetes.io/
[`localhost:8000`]: http://localhost:8000/
[SSH'ing into the VM]: 2-podman-docker.md#sshing-into-the-vm
