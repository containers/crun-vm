# Example: Minikube cluster with crun-qemu available as a runtime

You can use the `./minikube-start.sh` script in this directory to create a local
[minikube] Kubernetes cluster with crun-qemu available as a runtime (note that
this also points `kubectl` at the minikube cluster):

```console
$ ./minikube-start.sh
   Compiling crun-qemu v0.0.0 (/home/afaria/repos/crun-qemu)
    Finished dev [unoptimized + debuginfo] target(s) in 1.38s
😄  [crun-qemu-example] minikube v1.32.0 on Fedora 39
[...]
🏄  Done! kubectl is now configured to use "crun-qemu-example" cluster and "default" namespace by default
runtimeclass.node.k8s.io/crun-qemu created
```

Try going through the examples at [Using crun-qemu as a Kubernetes runtime].

Once you're done, you can delete the cluster with:

```console
$ minikube -p crun-qemu-example delete
```

[minikube]: https://minikube.sigs.k8s.io/
[Using crun-qemu as a Kubernetes runtime]: /docs/3-kubernetes.md
