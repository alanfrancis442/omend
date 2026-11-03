#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
# Used unchanged as preinst/postinst/prerm/postrm. No service or hardware writes.
set -eu
if ! command -v systemctl >/dev/null 2>&1 || ! command -v pgrep >/dev/null 2>&1; then
    echo 'Cannot verify cooling controller inactivity: systemctl and pgrep are required.' >&2
    exit 1
fi
if ! deb_service_state=$(systemctl show --property=ActiveState --value omend.service); then
    echo 'Cannot query omend service state; refusing package transaction.' >&2
    exit 1
fi
case "$deb_service_state" in
    inactive|failed) ;;
    *) echo 'omend must be inactive before package installation, upgrade or removal. Arrange monitored cooling and stop it explicitly.' >&2; exit 1 ;;
esac
if pgrep -x omend >/dev/null; then
    echo 'An omend process remains; refusing package transaction.' >&2
    exit 1
else
    deb_pgrep_status=$?
    [ "$deb_pgrep_status" -eq 1 ] || { echo 'Process inactivity check failed.' >&2; exit 1; }
fi
# Deliberately no daemon-reload, D-Bus reload, service activation, or group edits.
