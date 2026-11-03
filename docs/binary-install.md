# Install a native Linux binary bundle

This bundle contains the daemon, GUI, CLI, audio helper, configuration sample,
service files, project license texts, and a dependency license inventory. No Rust toolchain or source checkout is
needed to install it. Check `BUILD.txt` for the build architecture and glibc
version. This is a native build, not a distribution-independent package; runtime
library, desktop, D-Bus, and hardware compatibility must be checked on the target.
Desktop integrations use `pactl`, `wpctl`, `wl-copy`, and `wl-paste` when available.

Verify the downloaded archive before extracting it:

```sh
sha256sum -c omend-*.tar.gz.sha256
tar -xzf omend-<revision>-<target>.tar.gz
cd omend-<revision>-<target>
```

The `.sha256` file detects corrupted downloads only; it is not a signature. Obtain
the archive and checksum from the project's official release page over HTTPS and
compare the checksum with the one published there.

Replace the angle-bracket placeholders with the archive's actual name. From the
extracted directory, install the binaries and system integration:

First, as the normal desktop user, grant hardware-control access explicitly:

```sh
getent group omen >/dev/null || sudo groupadd --system omen
sudo usermod -aG omen "$USER"
```

Log out and back in for the GUI and audio service to receive the new membership.
Root and members of `omen` can change hardware and saved settings; other users
can read status. Membership applies to background and remote sessions too.
For an upgrade, establish monitored safe cooling and stop the existing daemon
before replacing its files; see the model-specific procedure in
`docs/hardware-validation.md`. The installer refuses replacement while the
system service is active. It never changes fan mode or stops a service itself.
Then install the system files:

```sh
sudo sh packaging/install-system.sh
sudo systemctl daemon-reload
sudo systemctl reload dbus
sudo systemctl enable --now omend
sudo systemctl restart omend
```

These commands install and start a privileged hardware service. Existing
configuration is preserved. Run the audio helper installation as your normal
desktop user:

```sh
sh packaging/install-audio-service.sh
```

Do not run it with `sudo`; it refuses root because it installs into your home
directory. From a bundle it installs `bin/omen-audio` by default.

Open `/usr/bin/omen-gui`. If an older user launcher at
`~/.local/share/applications/omen-gui.desktop` points to a source checkout's
`target/` directory, change its `Exec=` line to `Exec=/usr/bin/omen-gui`.
Installed launchers must refer to installed binaries. The extracted bundle and
build cache are not needed after installation.

The optional keyboard kernel module is not included; it must be built separately
against the target kernel's headers. See `docs/system-install.md` for kernel
setup and removal instructions, and `docs/audio-visualizer.md` for audio setup.
The source-build commands in those documents apply to a source checkout; the
prebuilt executables in this bundle are in `bin/`.

## Review and rollback

Before an upgrade, stop desktop clients and record `systemctl status omend` and
the installed version. Back up `/usr/bin/omend`, `/usr/bin/omen-cli`,
`/usr/bin/omen-gui`, `/etc/omend`, `/etc/dbus-1/system.d/dev.omend.conf`,
`/etc/systemd/system/omend.service`, and the sleep hook. Keep the previous bundle.
The installer preserves an existing configuration and installs files only; group
membership, D-Bus reload and service activation remain explicit steps above.
For rollback, restore those files and configuration from the backup, reload
systemd and D-Bus, then restart the daemon while monitoring temperature and fans.
Do not perform unattended upgrades while manual fan control is active.

`sh packaging/check-bundle.sh "$PWD"` verifies the extracted contents, dynamic
libraries, command-line help and a temporary-root installation. It does not
validate host service activation, GUI sessions or hardware behavior. The same
installer supports `DESTDIR=/absolute/staging/root` for packaging without root.

Build records name the exact revision, target, toolchain and host glibc. The dependency inventory does not establish license clearance. A local
bundle can contain uncommitted changes and is labelled accordingly. An official
candidate requires a clean `vVERSION` tag, matching workspace versions and an
explicit license-review attestation for that exact commit. That attestation is
a human review gate, not an automated conclusion that distribution is lawful.
Host compatibility and hardware validation remain separate release gates.

The bundle also includes `packaging/manage-system-files.py` for file-only backup,
removal and rollback, with configuration preserved on removal. Python 3 is
required for this optional helper. See `docs/system-install.md` for its commands
and the monitored cooling prerequisite; the helper never activates services.
