#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

set -o errexit -o pipefail -o nounset

script_dir="$( dirname "$0" | xargs readlink -e )"

__minikube() {
    minikube -p=crun-vm-example "$@"
}

__cp() {
    __minikube cp "$@"
}

__ssh() {
    __minikube ssh -- sudo "$@" </dev/null
}

__apt_get() {
    __ssh DEBIAN_FRONTEND=noninteractive apt-get \
        -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold \
        "$@"
}

# build runtime

cargo build

# create minikube cluster

__minikube start --driver=docker --container-runtime=cri-o

# minikube currently uses an old Ubuntu version that doesn't provide all our
# dependencies with the minimum versions we require, so we just hackily add the
# repo for a more recent version

__ssh sed -i s/jammy/mantic/g /etc/apt/sources.list
__apt_get update

# install dependencies

__apt_get install -yq --no-install-recommends \
    libvirt-clients \
    libvirt-daemon \
    libvirt-daemon-system \
    genisoimage \
    passt \
    qemu-system-x86 \
    qemu-utils \
    virtiofsd

# configure runtime

__ssh 'tee --append /etc/crio/crio.conf <<EOF
[crio.runtime.runtimes.crun-vm]
runtime_path = "/usr/local/bin/crun-vm"
EOF'

__cp "${script_dir}/../../target/debug/crun-vm" /usr/local/bin/crun-vm
__ssh chmod +x /usr/local/bin/crun-vm

# reload cluster so that the new runtime is picked up

__minikube start

# create ResourceClass

kubectl create -f - <<EOF
apiVersion: node.k8s.io/v1
kind: RuntimeClass
metadata:
  name: crun-vm
handler: crun-vm
EOF
