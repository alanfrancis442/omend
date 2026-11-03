#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
set -eu
check_files_only=
if [ "${1:-}" = --files-only ]; then check_files_only=yes; shift; fi
[ "$#" -eq 1 ] || { echo 'Usage: sh packaging/check-bundle.sh [--files-only] EXTRACTED_BUNDLE' >&2; exit 2; }
check_bundle=$(CDPATH= cd -- "$1" && pwd)
for check_file in bin/omend bin/omen-cli bin/omen-gui bin/omen-audio config/omend.toml \
    packaging/install-system.sh packaging/omen-gui.svg packaging/omen-gui.desktop \
    packaging/dev.omend.conf packaging/omend.service packaging/omend-sleep.sh \
    packaging/omen-audio.service packaging/install-audio-service.sh LICENSE INSTALL.md BUILD.txt Cargo.lock CONTENTS.sha256; do
    [ -f "$check_bundle/$check_file" ] || { echo "Missing bundle file: $check_file" >&2; exit 1; }
done
(cd "$check_bundle" && sha256sum -c CONTENTS.sha256 >/dev/null)
python3 "$check_bundle/packaging/check-markdown-links.py" "$check_bundle"
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$check_bundle/packaging/omen-gui.desktop"
fi
check_stage=$(mktemp -d)
trap 'rm -rf -- "$check_stage"' EXIT HUP INT TERM
DESTDIR="$check_stage" sh "$check_bundle/packaging/install-system.sh" >/dev/null
for check_binary in omend omen-cli omen-gui; do
    cmp "$check_bundle/bin/$check_binary" "$check_stage/usr/bin/$check_binary"
done
cmp "$check_bundle/config/omend.toml" "$check_stage/etc/omend/config.toml"
printf 'existing user configuration\n' > "$check_stage/etc/omend/config.toml"
DESTDIR="$check_stage" sh "$check_bundle/packaging/install-system.sh" >/dev/null
[ "$(cat "$check_stage/etc/omend/config.toml")" = 'existing user configuration' ] || { echo 'Upgrade overwrote configuration.' >&2; exit 1; }
# This check intentionally does not activate services, open the GUI or write hardware.
if [ -z "$check_files_only" ]; then
    for check_binary in omend omen-cli omen-gui omen-audio; do
        check_linkage=$(ldd "$check_bundle/bin/$check_binary")
        case "$check_linkage" in *'not found'*) printf '%s\n' "$check_linkage" >&2; exit 1 ;; esac
    done
    for check_binary in omend omen-cli omen-audio; do
        "$check_bundle/bin/$check_binary" --help >/dev/null
    done
fi
printf 'Bundle checks passed: %s\n' "$check_bundle"
