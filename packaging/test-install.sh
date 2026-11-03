#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
# Tests installation entirely in temporary directories; no privileged operations.
set -eu
install_test_repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
install_test_temp=$(mktemp -d)
trap 'rm -rf -- "$install_test_temp"' EXIT HUP INT TERM
install_test_bundle="$install_test_temp/bundle with spaces"
mkdir -p "$install_test_bundle/bin" "$install_test_bundle/packaging" "$install_test_bundle/config"
cp "$install_test_repo/packaging/install-system.sh" "$install_test_bundle/packaging/"
cp "$install_test_repo/packaging/omen-gui.desktop" "$install_test_repo/packaging/omen-gui.svg" \
    "$install_test_repo/packaging/dev.omend.conf" "$install_test_repo/packaging/omend.service" \
    "$install_test_repo/packaging/omend-sleep.sh" "$install_test_bundle/packaging/"
cp "$install_test_repo/config/omend.toml" "$install_test_bundle/config/"
cp -R "$install_test_repo/LICENSE" "$install_test_repo/LICENSES" "$install_test_bundle/"
for install_test_binary in omend omen-cli omen-gui; do
    printf '#!/bin/sh\nexit 0\n' > "$install_test_bundle/bin/$install_test_binary"
done
install_test_root="$install_test_temp/root with spaces"
DESTDIR="$install_test_root" sh "$install_test_bundle/packaging/install-system.sh" >/dev/null
cmp "$install_test_bundle/config/omend.toml" "$install_test_root/etc/omend/config.toml"
[ -x "$install_test_root/usr/bin/omend" ]
[ -f "$install_test_root/usr/share/icons/hicolor/scalable/apps/omen-gui.svg" ]
printf 'keep existing\n' > "$install_test_root/etc/omend/config.toml"
DESTDIR="$install_test_root" sh "$install_test_bundle/packaging/install-system.sh" >/dev/null
[ "$(cat "$install_test_root/etc/omend/config.toml")" = 'keep existing' ]
# Preserve even a dangling administrator-supplied config symlink.
rm "$install_test_root/etc/omend/config.toml"
ln -s absent.toml "$install_test_root/etc/omend/config.toml"
DESTDIR="$install_test_root" sh "$install_test_bundle/packaging/install-system.sh" >/dev/null
[ -L "$install_test_root/etc/omend/config.toml" ]
if DESTDIR=relative sh "$install_test_bundle/packaging/install-system.sh" >/dev/null 2>&1; then
    echo 'Relative DESTDIR should be rejected' >&2; exit 1
fi
printf 'Staged fresh-install and upgrade checks passed.\n'

DESTDIR="$install_test_root" python3 "$install_test_repo/packaging/manage-system-files.py" backup --backup "$install_test_temp/backup"
DESTDIR="$install_test_root" python3 "$install_test_repo/packaging/manage-system-files.py" remove
[ ! -e "$install_test_root/usr/bin/omend" ]
[ -L "$install_test_root/etc/omend/config.toml" ]
DESTDIR="$install_test_root" python3 "$install_test_repo/packaging/manage-system-files.py" restore --backup "$install_test_temp/backup"
[ -x "$install_test_root/usr/bin/omend" ]
[ -L "$install_test_root/etc/omend/config.toml" ]
printf 'Staged backup, removal and exact managed-file rollback checks passed.\n'

python3 "$install_test_repo/packaging/test-release-tools.py"
python3 "$install_test_repo/packaging/test-hardware-recovery.py"
python3 "$install_test_repo/packaging/test-systemd-recovery.py"
