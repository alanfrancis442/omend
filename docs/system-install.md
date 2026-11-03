# System install and removal

To create a native binary bundle with installation instructions, run
`sh packaging/build-release.sh` (add `--offline` when dependencies are cached).
The archive and SHA-256 checksum are written to `dist/`, outside Cargo's build
cache. See [Binary installation](binary-install.md) to install that bundle.

The daemon unit starts `/usr/bin/omend`. Build the binaries first, then copy them into place:

```bash
cargo build --release --locked -p omend -p omen-cli -p omen-gui -p omen-audio
sudo install -m 755 target/release/omend /usr/bin/omend
sudo install -m 755 target/release/omen-cli /usr/bin/omen-cli
sudo install -m 755 target/release/omen-gui /usr/bin/omen-gui
sudo install -D -m 644 packaging/omen-gui.desktop /usr/share/applications/omen-gui.desktop
sudo install -D -m 644 packaging/omen-gui.svg /usr/share/icons/hicolor/scalable/apps/omen-gui.svg
```

## Install

Hardware control is restricted to root and members of the `omen` group. As the
normal desktop user who will run the GUI and audio helper, create the group if
needed and explicitly grant access before installing/reloading the D-Bus policy:

```bash
getent group omen >/dev/null || sudo groupadd --system omen
sudo usermod -aG omen "$USER"
```

Log out of the desktop session and back in after adding membership. The user
audio service must also run in a session with the new group membership. Other
users can read status but cannot change fans, lighting, profiles, saved settings,
or audio levels. Membership is persistent administrator trust, including remote
sessions; it is not limited to an active local desktop. See
[Daemon safety and access](daemon-safety.md) for the lifecycle and safety policy.

```bash
sudo mkdir -p /etc/dbus-1/system.d
sudo install -m 644 packaging/dev.omend.conf /etc/dbus-1/system.d/
sudo install -m 644 packaging/omend.service /etc/systemd/system/omend.service
sudo install -m 755 packaging/omend-sleep.sh /usr/lib/systemd/system-sleep/omend
sudo mkdir -p /etc/omend
if ! sudo test -e /etc/omend/config.toml && ! sudo test -L /etc/omend/config.toml; then
    sudo install -m 644 config/omend.toml /etc/omend/config.toml
fi
sudo systemctl daemon-reload
sudo systemctl reload dbus
sudo systemctl enable --now omend
```

## Background audio lighting

As your normal desktop user, install the audio helper after building:

```bash
sh packaging/install-audio-service.sh
```

This enables a systemd **user** service at login. It captures speaker audio and
feeds the hardware daemon while Audio is selected, even after the GUI closes.
See [Audio lighting](audio-visualizer.md) for controls and troubleshooting.

## Keyboard backlight

`omend` writes `hp::kbd_zoned_backlight-*` under `/sys/class/leds`. Stock `hp-wmi` does not create those nodes, so build the small module in this repo and load it next to `hp-wmi`. It only calls the backlight WMI method. Fan control stays on `hp-wmi`.

```bash
sudo make -C kernel/omen-kbd install
echo omen-kbd | sudo tee /etc/modules-load.d/omen-kbd.conf
sudo modprobe omen-kbd
sudo systemctl restart omend
```

`ls /sys/class/leds` should list `hp::kbd_zoned_backlight-wasd`, `-left`, `-center`, and `-right` on a four-zone keyboard, or `hp::kbd_backlight` on a single-zone keyboard. Restarting `omend` is required so it notices the new nodes.

## Update an existing install

Rebuild the binaries, replace them, and restart the daemon. Reinstall service or
D-Bus policy files whenever they change between versions.

When upgrading from a version with unrestricted D-Bus access, perform the group
setup above, reinstall `packaging/dev.omend.conf` to
`/etc/dbus-1/system.d/dev.omend.conf`, reload D-Bus, and log out/in. Replacing only
the executables does not activate the new access policy.

```bash
cargo build --release --locked -p omend -p omen-cli -p omen-gui -p omen-audio
sudo install -m 755 target/release/omend /usr/bin/omend
sudo install -m 755 target/release/omen-cli /usr/bin/omen-cli
sudo install -m 755 target/release/omen-gui /usr/bin/omen-gui
sudo systemctl restart omend
```

Update the per-user audio helper as your normal desktop user:

```bash
sh packaging/install-audio-service.sh
```

Open the window with `omen-gui`. Fan curve is the graph: drag a point, click empty space to add one (up to eight), and use the steppers for the selected point, the axis, and the spin-down delay. Apply sends the curve to the running daemon. Cancel restores the last applied curve. Copy graph and Paste graph use the Wayland clipboard as `temp:percent` pairs.

Applying a curve works against an older daemon. The spin-down delay control does not, until `omend` is replaced and restarted as above.

## Uninstall

Disable the background audio helper as your normal desktop user:

```bash
systemctl --user disable --now omen-audio.service
rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/omen-audio.service"
rm -f "$HOME/.local/bin/omen-audio"
systemctl --user daemon-reload
```

System removal requires verified cooling after the daemon stops. Firmware Auto
alone did not establish safe recovery on the test host. Follow the monitored
hardware procedure in [Hardware validation](hardware-validation.md), then use
the file-only removal helper described below. Do not unload the optional keyboard
module or remove configuration until you have a backup and have confirmed that
cooling remains safe. Remove the kernel module and its boot-load entry separately
only if you installed them; retain `/etc/omend` unless you explicitly want to
discard saved settings.

## Release build validation

The pinned Rust toolchain is in `rust-toolchain.toml`. CI builds and tests on
Ubuntu 24.04; this is the initial runtime build baseline, not a hardware support
claim. The bundle builder explicitly targets the native GNU/Linux host, placing
its executables in `target/<host>/release/`. Source-build commands above use
Cargo's default `target/release/` path instead.

```sh
sh packaging/test-install.sh
sh packaging/build-release.sh --offline
```

The build script runs bundle smoke checks before writing an archive. For a
reviewed candidate, tag a clean commit with all package versions matching,
generate and review dependency notices and license texts, and explicitly attest review:

```sh
OMEND_LICENSE_REVIEWED_REVISION=<full-reviewed-commit-sha> \
    sh packaging/build-release.sh --offline --official v0.1.0
```

The manual release-candidate workflow validates and uploads an artifact for
review. It does not publish a release or certify hardware safety. An attested
license review must cover the exact locked dependency set and distribution
route; a generated inventory alone is insufficient.

## Service confinement and validation gate

The packaged unit sets `NoNewPrivileges`, makes `/usr`, `/boot` and `/etc`
read-only with `ProtectSystem=full`, permits configuration updates only under
`/etc/omend`, hides home directories, protects cgroups, denies creation of
setuid/setgid files, and permits Unix sockets only. It also gives the daemon
private `/dev`, `/tmp` and network namespaces; protects kernel modules, kernel
logs, the clock and hostname; hides other processes in `/proc`; restricts
namespaces, realtime scheduling, personality changes and writable-executable
memory; limits system calls to `@system-service` on the native architecture;
sets `UMask=0077`; and bounds capabilities to `CAP_CHOWN`, `CAP_DAC_OVERRIDE`
and `CAP_FOWNER`, which atomic configuration saves need. `TimeoutStopSec=15s`
limits how long a hung daemon can hold manual fan control, and
`ExecStopPost=-/usr/bin/omend --recover-fans` requests the configured fan
recovery after every exit, including `SIGKILL` and crashes, whenever no daemon
holds `/run/omend.lock`. The root daemon reads configuration only from
`--config` or `/etc/omend/config.toml`, never from its working directory, and
warns when that file or its directory is not root-owned or is group- or
world-writable. The daemon reads `/proc`,
hardware identification and sensor/power values under `/sys`, uses the system
D-Bus socket under `/run`, and owns `/run/omend.lock`. It writes HP fan, platform
profile and LED attributes under `/sys`, plus its atomic configuration files
under `/etc/omend`. `ProtectKernelTunables` is deliberately absent because it
would make required sysfs controls read-only. The unit remains privileged and
sysfs is not restricted to a model-specific allowlist.

These settings have passed unit-file syntax verification only. Real startup,
D-Bus calls, configuration saves, fan/profile/lighting writes, signal recovery
and suspend/resume must be validated under this exact unit before a supported
release. A custom configuration location requires a corresponding
`ReadWritePaths` drop-in; locations under a home directory are hidden by default.

## File backup, removal and rollback

The bundled Python 3 helper operates on managed files only and does not activate
services or send hardware commands. It includes the configuration directory in
backups; removal preserves configuration and the separate user audio install.
Restoring a backup replaces managed files and configuration with their prior
state, including removing files absent at backup time. Backups can contain
private settings and should stay administrator-only; when run as root, restore
refuses a backup directory or manifest that is not root-owned or that is group-
or world-writable.

```sh
sudo python3 packaging/manage-system-files.py backup --backup /var/backups/omend-before-upgrade
```

Before removal or restoration, establish and monitor safe cooling, then stop the
service following the model-specific hardware validation procedure. On this
host, firmware Auto alone has not established safe recovery; do not use an
unattended generic stop/uninstall sequence. The helper refuses live-system
changes unless systemd reports the service inactive.

```sh
sudo python3 packaging/manage-system-files.py remove
# Or restore the backed-up managed files/configuration:
sudo python3 packaging/manage-system-files.py restore --backup /var/backups/omend-before-upgrade
```

After restoration, explicitly reload systemd and D-Bus, restart while monitoring
cooling, and validate operation. The helper does not change group membership,
user audio files, kernel modules, or old desktop launchers. For staging tests,
set `DESTDIR=/absolute/temporary/root` and run without sudo.

A user launcher overrides the system launcher. Inspect
`~/.local/share/applications/omen-gui.desktop` for an `Exec` referencing a source
checkout. Back up that user file, then change its `Exec` to `/usr/bin/omen-gui`,
or move it aside to use the installed system launcher. Confirm the installed
GUI opens before removing any checkout or Cargo build cache.

## Dependency notice generation

Building a bundle also requires Python 3. The builder runs the inventory generator
against cached, locked Cargo metadata for the native host and includes declared
license expressions, copied license/notice texts and a machine-readable report.
It recursively discovers package-contained license/notice files, including bundled
asset and native-vendor subdirectories, while excluding caches and external symlink
directories. Workspace scanning uses only root project texts and `LICENSES/`.
It copies no dependency implementation code and rejects license files that resolve
outside their package. Generation does not select a license route or establish
legal clearance. Missing text entries require manual review even when a package
declares an SPDX expression.

From a source checkout with dependencies cached:

```sh
python3 scripts/generate-notices.py
python3 scripts/generate-notices.py --check
# Or generate/check another private directory:
review_dir=$(mktemp -d)
python3 scripts/generate-notices.py --output-dir "$review_dir"
python3 scripts/generate-notices.py --output-dir "$review_dir" --check
```

The default output is ignored `dist/license-inventory/`. Bundle generation writes
the inventory directly into staging. The report records target, Cargo.lock hash,
package counts and file checksums; inventory includes reachable test/build
dependencies as well as runtime dependencies. The generator script included in a
bundle is an audit aid; rerunning it requires a complete source checkout and Cargo
dependency cache, neither of which installation requires.

Backup and restore preserve source file ownership when run as root, without
following symlinks. Unprivileged staging retains the staging user's ownership.
`DESTDIR` is resolved before deciding whether a service-inactivity guard is
needed; backups inside the managed config/license directories are rejected.

Archives use sorted entries, normalized root ownership, epoch timestamps and
`gzip -n`. `SOURCE_DATE_EPOCH` must contain integer seconds and defaults to the
source commit time. Official candidates also use that epoch in `BUILD.txt`; local
dirty archives retain unique UTC names and build timestamps. This normalizes
archive metadata, without claiming full reproducibility across toolchains,
compiler environments or native dependencies.
