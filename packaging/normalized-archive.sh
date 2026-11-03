#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
set -eu
[ "$#" -eq 4 ] || { echo 'Usage: normalized-archive.sh STAGING_DIRECTORY TOP_DIRECTORY OUTPUT EPOCH' >&2; exit 2; }
case "$4" in ''|*[!0-9]*) echo 'Archive epoch must be nonnegative integer seconds.' >&2; exit 2 ;; esac
# GNU format omits pax atime/ctime headers; group/other write bits are dropped.
# GNU tar invokes gzip directly so compression errors propagate without pipelines.
tar --format=gnu --sort=name --mtime="@$4" --owner=0 --group=0 --numeric-owner --mode=go-w -C "$1" -I 'gzip -n' -cf "$3" "$2"
