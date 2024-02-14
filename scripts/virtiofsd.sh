#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later

# libvirt doesn't let us pass --modcaps to virtiofsd (which we use to avoid
# having virtiofsd unsuccessfully attempt to acquire additional capabilities),
# so we tell libvirt to use this script instead.

/usr/libexec/virtiofsd --modcaps=-mknod:-setfcap "$@"
