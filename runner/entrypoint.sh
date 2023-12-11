#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-only

set -o errexit -o pipefail -o nounset

virtlogd --daemon
virtqemud --daemon

chmod u+w /vm/image

virsh --connect qemu:///session --quiet create /vm/domain.xml --console
