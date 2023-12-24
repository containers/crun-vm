# Example: Using `crun-qemu` in Kubernetes

It is possible to use `crun-qemu` to run VMs as regular pods in a [Kubernetes]
cluster.

## Preparation

To enable the `crun-qemu` on a cluster, follow these steps:

1. Ensure that the cluster is using the [CRI-O] container runtime. Refer to
   Kubernetes' docs on [container runtimes].

2. Install `crun-qemu` on all cluster nodes where pods may be scheduled. Refer
   to the [Installing] section of the README.

3. Append the following to `/etc/crio/crio.conf`:

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

## Using it

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
the command (this is the same behavior as with `podman exec` or `docker exec`,
see [SSH'ing into the VM] in the README):

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

## minikube demo

You can use the `./minikube-start.sh` script in this directory to easily create
a local [minikube] Kubernetes cluster configured as per the steps above (note
that the script will also configure `kubectl` to point at the minikube
cluster):

```console
$ ./minikube-start.sh
   Compiling crun-qemu v0.0.0 (/home/afaria/repos/crun-qemu)
    Finished dev [unoptimized + debuginfo] target(s) in 1.38s
😄  [crun-qemu-example] minikube v1.32.0 on Fedora 39
[...]
🏄  Done! kubectl is now configured to use "crun-qemu-example" cluster and "default" namespace by default
runtimeclass.node.k8s.io/crun-qemu created
```

Try creating the pod defined in the section above and running the `kubectl`
commands described there.

Once you're done, you can delete the cluster with:

```console
$ minikube -p crun-qemu-example delete
```

[container runtimes]: https://kubernetes.io/docs/setup/production-environment/container-runtimes/#cri-o
[CRI-O]: https://cri-o.io/
[Installing]: /README.md#installing
[Kubernetes]: https://kubernetes.io/
[`localhost:8000`]: http://localhost:8000/
[minikube]: https://minikube.sigs.k8s.io/
[SSH'ing into the VM]: /README.md#sshing-into-the-vm
