#!/bin/sh
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
set -eu

umask 022
if [ "$(id -u)" -eq 0 ]; then
    echo 'Run as the normal desktop user, not root or sudo: this installs a per-user service.' >&2
    exit 1
fi
[ "$#" -le 1 ] || { echo 'Usage: sh packaging/install-audio-service.sh [OMEN_AUDIO_BINARY]' >&2; exit 2; }
audio_repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
# A release bundle ships bin/omen-audio; a source checkout falls back to Cargo output.
if [ "$#" -eq 1 ]; then
    audio_binary=$1
elif [ -x "$audio_repo/bin/omen-audio" ]; then
    audio_binary="$audio_repo/bin/omen-audio"
else
    audio_binary="$audio_repo/target/release/omen-audio"
fi
: "${HOME:?HOME must be set}"
audio_config=${XDG_CONFIG_HOME:-"$HOME/.config"}
audio_bin_dir="$HOME/.local/bin"
audio_unit_dir="$audio_config/systemd/user"

if [ ! -x "$audio_binary" ]; then
    echo "Build first: cargo build --release -p omen-audio -p omen-gui --locked" >&2
    exit 1
fi
install -d "$audio_bin_dir" "$audio_unit_dir"
install -m 755 "$audio_binary" "$audio_bin_dir/omen-audio"
install -m 644 "$audio_repo/packaging/omen-audio.service" "$audio_unit_dir/omen-audio.service"
systemctl --user daemon-reload
systemctl --user enable omen-audio.service
systemctl --user restart omen-audio.service
systemctl --user is-active --quiet omen-audio.service
echo "Background audio is enabled. It starts at login and stays running when the GUI closes."
