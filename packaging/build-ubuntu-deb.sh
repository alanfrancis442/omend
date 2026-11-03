#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
set -eu
# Package file modes must not depend on the builder's umask.
umask 022
deb_repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$deb_repo"
deb_offline=
case "${1:-}" in '' ) ;; --offline) deb_offline=--offline ;; *) echo 'Usage: sh packaging/build-ubuntu-deb.sh [--offline]' >&2; exit 2 ;; esac
[ "$#" -le 1 ] || exit 2
# Do not repackage an Arch/glibc-2.44 build under an Ubuntu filename.
. /etc/os-release
[ "${ID:-}" = ubuntu ] && [ "${VERSION_ID:-}" = 24.04 ] || { echo 'Build requires native Ubuntu 24.04.' >&2; exit 1; }
[ "$(getconf GNU_LIBC_VERSION)" = 'glibc 2.39' ] || { echo 'Expected Ubuntu 24.04 glibc 2.39 build baseline.' >&2; exit 1; }
for deb_tool in dpkg dpkg-deb dpkg-shlibdeps cargo rustc git python3; do command -v "$deb_tool" >/dev/null; done
[ "$(dpkg --print-architecture)" = amd64 ] || { echo 'Only native amd64 is configured.' >&2; exit 1; }
deb_target=$(rustc -vV | sed -n 's/^host: //p')
[ "$deb_target" = x86_64-unknown-linux-gnu ] || { echo 'Rust host must be x86_64-unknown-linux-gnu.' >&2; exit 1; }
[ -z "${CARGO_BUILD_TARGET:-}" ] || [ "$CARGO_BUILD_TARGET" = "$deb_target" ] || exit 1
: "${OMEND_DEB_MAINTAINER:?Set a real package maintainer identity, or an explicitly local-only build identity.}"
case "$OMEND_DEB_MAINTAINER" in *'
'*) echo 'Maintainer must be one line.' >&2; exit 1 ;; esac
# Record cleanliness before any generated output can appear in the checkout.
deb_revision=$(git rev-parse HEAD)
deb_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' crates/omend/Cargo.toml)
deb_version="$deb_version~preview+git$(git rev-parse --short HEAD)"
[ -z "$(git status --porcelain --untracked-files=normal)" ] || deb_version="${deb_version}.dirty"
cargo build --workspace --release --locked --target "$deb_target" --target-dir "$deb_repo/target" ${deb_offline:+"$deb_offline"}
mkdir -p dist
deb_stage=$(mktemp -d "$deb_repo/dist/.deb-stage.XXXXXX")
trap 'rm -rf -- "$deb_stage"' EXIT HUP INT TERM
# Generate notices in staging, never into the source checkout.
python3 scripts/generate-notices.py --output-dir "$deb_stage/notices"
deb_root="$deb_stage/root"
mkdir -p "$deb_root/DEBIAN" "$deb_root/usr/bin" "$deb_root/usr/share/doc/omend-preview" "$deb_stage/debian"
for deb_binary in omend omen-cli omen-gui omen-audio; do
    install -m 755 "target/$deb_target/release/$deb_binary" "$deb_root/usr/bin/$deb_binary"
done
install -D -m 644 config/omend.toml "$deb_root/etc/omend/config.toml"
install -D -m 644 packaging/dev.omend.conf "$deb_root/etc/dbus-1/system.d/dev.omend.conf"
install -D -m 644 packaging/omend.service "$deb_root/usr/lib/systemd/system/omend.service"
install -D -m 755 packaging/omend-sleep.sh "$deb_root/usr/lib/systemd/system-sleep/omend"
install -D -m 644 packaging/omen-gui.desktop "$deb_root/usr/share/applications/omen-gui.desktop"
install -D -m 644 packaging/omen-gui.svg "$deb_root/usr/share/icons/hicolor/scalable/apps/omen-gui.svg"
mkdir -p "$deb_root/usr/lib/systemd/user"
sed 's|ExecStart=.*|ExecStart=/usr/bin/omen-audio|' packaging/omen-audio.service > "$deb_root/usr/lib/systemd/user/omen-audio.service"
cp -R README.md CONTRIBUTING.md SECURITY.md CHANGELOG.md AGENTS.md .github LICENSE LICENSES docs "$deb_root/usr/share/doc/omend-preview/"
cp -R "$deb_stage/notices/THIRD_PARTY_NOTICES.md" "$deb_stage/notices/third-party-licenses" \
    "$deb_stage/notices/license-inventory.json" "$deb_root/usr/share/doc/omend-preview/"
# Debian policy 12.5: every package ships /usr/share/doc/<package>/copyright.
install -m 644 LICENSE "$deb_root/usr/share/doc/omend-preview/copyright"
find "$deb_root/usr/share/doc/omend-preview" -name __pycache__ -type d -prune -exec rm -rf -- {} +
printf '/etc/omend/config.toml\n/etc/dbus-1/system.d/dev.omend.conf\n' > "$deb_root/DEBIAN/conffiles"
for deb_hook in preinst postinst prerm postrm; do
    install -m 755 packaging/deb-service-guard.sh "$deb_root/DEBIAN/$deb_hook"
done
cat > "$deb_stage/debian/control" <<EOF
Source: omend-preview
Section: utils
Priority: optional
Maintainer: $OMEND_DEB_MAINTAINER

Package: omend-preview
Architecture: amd64
Description: Experimental HP OMEN/Victus controls (not hardware validated)
EOF
deb_shlibs=$(cd "$deb_stage" && dpkg-shlibdeps -O \
    -e"$deb_root/usr/bin/omend" -e"$deb_root/usr/bin/omen-cli" \
    -e"$deb_root/usr/bin/omen-gui" -e"$deb_root/usr/bin/omen-audio")
deb_depends=$(printf '%s\n' "$deb_shlibs" | sed -n 's/^shlibs:Depends=//p')
[ -n "$deb_depends" ] || { echo 'No ELF dependency result; refusing package.' >&2; exit 1; }
cat > "$deb_root/DEBIAN/control" <<EOF
Package: omend-preview
Version: $deb_version
Architecture: amd64
Maintainer: $OMEND_DEB_MAINTAINER
Section: utils
Priority: optional
Pre-Depends: systemd, procps
Depends: $deb_depends, dbus, libwayland-client0, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcb1, libegl1, libgl1, libfontconfig1, libfreetype6, fonts-dejavu-core
Recommends: pulseaudio-utils
Suggests: pipewire-pulse, wl-clipboard
Description: Experimental HP OMEN/Victus controls (not hardware validated)
 Native Ubuntu 24.04 amd64 development candidate. No services are started,
 stopped or reloaded automatically. Hardware recovery validation is incomplete.
 Optional out-of-tree kernel module excluded. Read the installed safety docs.
EOF
{
    printf 'Kind: experimental Ubuntu 24.04 amd64 package\nRevision: %s\nVersion: %s\n' "$deb_revision" "$deb_version"
    rustc --version
    getconf GNU_LIBC_VERSION
    printf 'ELF dependencies: %s\n' "$deb_depends"
} > "$deb_root/usr/share/doc/omend-preview/BUILD.txt"
deb_output="$deb_repo/dist/omend-preview_${deb_version}_amd64.deb"
dpkg-deb --root-owner-group --build "$deb_root" "$deb_output"
dpkg-deb --info "$deb_output"
dpkg-deb --contents "$deb_output"
dpkg-deb --extract "$deb_output" "$deb_stage/extracted"
cmp "$deb_root/etc/omend/config.toml" "$deb_stage/extracted/etc/omend/config.toml"
(cd dist && sha256sum "$(basename -- "$deb_output")" > "$(basename -- "$deb_output").sha256")
printf 'Experimental file-only package: %s\n' "$deb_output"
