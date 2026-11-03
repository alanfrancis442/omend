#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
# Installs files only. Group setup and service activation are separate steps.
set -eu
# Installed files must not inherit a permissive caller umask (sudo keeps it).
umask 022
install_bundle=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
install_root=${DESTDIR:-}
case "$install_root" in ''|/*) ;; *) echo 'DESTDIR must be an absolute staging directory' >&2; exit 1 ;; esac
if [ -n "$install_root" ]; then install_root=$(realpath -m -- "$install_root"); fi
if [ -z "$install_root" ] || [ "$install_root" = / ]; then
    # File replacement must not race the running privileged controller.
    if systemctl is-active --quiet omend.service; then
        echo 'Stop omend.service under monitored safe cooling before replacing installed files.' >&2
        exit 1
    else
        install_service_status=$?
        if [ "$install_service_status" -ne 3 ] && [ "$install_service_status" -ne 4 ]; then
            echo 'Cannot establish that omend.service is inactive; no files changed.' >&2
            exit 1
        fi
    fi
fi
for install_binary in omend omen-cli omen-gui; do
    install -D -m 755 "$install_bundle/bin/$install_binary" "$install_root/usr/bin/$install_binary"
done
install -D -m 644 "$install_bundle/packaging/omen-gui.desktop" "$install_root/usr/share/applications/omen-gui.desktop"
install -D -m 644 "$install_bundle/packaging/omen-gui.svg" "$install_root/usr/share/icons/hicolor/scalable/apps/omen-gui.svg"
install -D -m 644 "$install_bundle/packaging/dev.omend.conf" "$install_root/etc/dbus-1/system.d/dev.omend.conf"
install -D -m 644 "$install_bundle/packaging/omend.service" "$install_root/etc/systemd/system/omend.service"
install -D -m 755 "$install_bundle/packaging/omend-sleep.sh" "$install_root/usr/lib/systemd/system-sleep/omend"
install -d "$install_root/etc/omend"
if [ ! -e "$install_root/etc/omend/config.toml" ] && [ ! -L "$install_root/etc/omend/config.toml" ]; then
    install -m 644 "$install_bundle/config/omend.toml" "$install_root/etc/omend/config.toml"
fi
install -d "$install_root/usr/share/licenses/omend"
cp -R "$install_bundle/LICENSE" "$install_bundle/LICENSES" "$install_root/usr/share/licenses/omend/"
if [ -f "$install_bundle/THIRD_PARTY_NOTICES.md" ]; then
    install -m 644 "$install_bundle/THIRD_PARTY_NOTICES.md" "$install_root/usr/share/licenses/omend/"
    cp -R "$install_bundle/third-party-licenses" "$install_root/usr/share/licenses/omend/"
    install -m 644 "$install_bundle/license-inventory.json" "$install_root/usr/share/licenses/omend/"
fi
printf 'Installed files under %s; services have not been started. Follow INSTALL.md.\n' "${install_root:-/}"
