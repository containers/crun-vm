# Example: Minikube cluster with crun-vm available as a runtime

You can use the `./minikube-start.sh` script in this directory to create a local
[minikube] Kubernetes cluster with crun-vm available as a runtime (note that
this also points `kubectl` at the minikube cluster):

```console
$ ./minikube-start.sh
   Compiling crun-vm v0.1.0 (/home/afaria/repos/crun-vm)
    Finished dev [unoptimized + debuginfo] target(s) in 1.38s
😄  [crun-vm-example] minikube v1.32.0 on Fedora 39
[...]
🏄  Done! kubectl is now configured to use "crun-vm-example" cluster and "default" namespace by default
runtimeclass.node.k8s.io/crun-vm created
```

Try going through the examples at [Using crun-vm as a Kubernetes runtime].

Once you're done, you can delete the cluster with:

```console
$ minikube -p crun-vm-example delete
```

[minikube]: https://minikube.sigs.k8s.io/
[Using crun-vm as a Kubernetes runtime]: /docs/3-kubernetes.md
