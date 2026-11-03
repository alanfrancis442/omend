#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
set -eu
# Bundle file modes must not depend on the builder's umask.
umask 022
release_repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$release_repo"
release_offline=
release_tag=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --offline) release_offline=--offline; shift ;;
        --official) [ "$#" -ge 2 ] || exit 2; release_tag=$2; shift 2 ;;
        *) echo 'Usage: sh packaging/build-release.sh [--offline] [--official vVERSION]' >&2; exit 2 ;;
    esac
done
release_host=$(rustc -vV | sed -n 's/^host: //p')
case "$release_host" in *-unknown-linux-gnu) ;; *) echo 'Only native GNU/Linux bundles are supported.' >&2; exit 1 ;; esac
if [ -n "${CARGO_BUILD_TARGET:-}" ] && [ "$CARGO_BUILD_TARGET" != "$release_host" ]; then
    echo 'CARGO_BUILD_TARGET must match the native rustc host.' >&2; exit 1
fi
release_revision=$(git rev-parse HEAD)
release_short=$(git rev-parse --short HEAD)
release_dirty=$(git status --porcelain --untracked-files=normal)
release_kind=local
release_built_at=$(date -u +%Y%m%dT%H%M%S%NZ)
release_epoch=${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}
case "$release_epoch" in ''|*[!0-9]*) echo 'SOURCE_DATE_EPOCH must be nonnegative integer seconds.' >&2; exit 2 ;; esac
if [ -n "$release_tag" ]; then
    [ -z "$release_dirty" ] || { echo 'Official bundles require a clean checkout.' >&2; exit 1; }
    release_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' crates/omend/Cargo.toml)
    [ "$release_tag" = "v$release_version" ] || { echo 'Tag must match the daemon package version.' >&2; exit 1; }
    [ "$(git rev-parse "refs/tags/$release_tag^{commit}")" = "$release_revision" ] || { echo 'Tag must identify HEAD.' >&2; exit 1; }
    for release_manifest in crates/*/Cargo.toml; do
        [ "$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$release_manifest")" = "$release_version" ] || { echo 'Workspace versions differ.' >&2; exit 1; }
    done
    [ "${OMEND_LICENSE_REVIEWED_REVISION:-}" = "$release_revision" ] || { echo 'Official bundles require OMEND_LICENSE_REVIEWED_REVISION=full reviewed commit SHA.' >&2; exit 1; }
    release_kind=official-candidate
    release_short=$release_tag
    release_built_at=$(date -u -d "@$release_epoch" +%Y%m%dT%H%M%SZ)
elif [ -n "$release_dirty" ]; then
    release_short="$release_short-dirty-$release_built_at"
fi
# Explicit --target prevents .cargo/config target settings from mislabelling binaries.
cargo build --release --workspace --locked --target "$release_host" --target-dir "$release_repo/target" ${release_offline:+"$release_offline"}
release_name="omend-$release_short-$release_host"
mkdir -p dist
release_stage=$(mktemp -d "$release_repo/dist/.stage.XXXXXX")
trap 'rm -rf -- "$release_stage"' EXIT HUP INT TERM
release_bundle="$release_stage/$release_name"
mkdir -p "$release_bundle/bin" "$release_bundle/config" "$release_bundle/packaging" "$release_bundle/docs"
for release_binary in omend omen-cli omen-gui omen-audio; do
    install -m 755 "$release_repo/target/$release_host/release/$release_binary" "$release_bundle/bin/$release_binary"
done
cp -R LICENSE LICENSES "$release_bundle/"
cp config/omend.toml "$release_bundle/config/"
cp -R packaging/. "$release_bundle/packaging/"
cp -R licensing .github "$release_bundle/"
cp AGENTS.md "$release_bundle/"
cp -R docs/. "$release_bundle/docs/"
for release_doc in README.md CONTRIBUTING.md SECURITY.md CHANGELOG.md; do
    cp "$release_doc" "$release_bundle/"
done
cp docs/binary-install.md "$release_bundle/INSTALL.md"
# Local test runs can leave bytecode caches; they are not release content.
find "$release_bundle" -name __pycache__ -type d -prune -exec rm -rf -- {} +
mkdir -p "$release_bundle/scripts"
cp scripts/generate-notices.py scripts/fetch-license-supplements.py "$release_bundle/scripts/"
python3 scripts/generate-notices.py --output-dir "$release_bundle"
{
    printf 'Bundle kind: %s\nBuild host: %s\nRevision: %s\nTag: %s\n' "$release_kind" "$release_host" "$release_revision" "${release_tag:-none}"
    if [ -n "$release_dirty" ]; then printf 'Dirty checkout: yes\n'; else printf 'Dirty checkout: no\n'; fi
    printf 'Build timestamp UTC: %s\nArchive epoch: %s\n' "$release_built_at" "$release_epoch"
    printf 'License reviewed revision: %s\n' "${OMEND_LICENSE_REVIEWED_REVISION:-not attested}"
    rustc --version
    cargo --version
    getconf GNU_LIBC_VERSION
} > "$release_bundle/BUILD.txt"
cp Cargo.lock "$release_bundle/"
(cd "$release_bundle" && find . -type f ! -name CONTENTS.sha256 -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > CONTENTS.sha256)
sh packaging/check-bundle.sh "$release_bundle"
sh packaging/normalized-archive.sh "$release_stage" "$release_name" "$release_repo/dist/$release_name.tar.gz" "$release_epoch"
(cd dist && sha256sum "$release_name.tar.gz" > "$release_name.tar.gz.sha256")
printf 'Release bundle: dist/%s.tar.gz\n' "$release_name"
