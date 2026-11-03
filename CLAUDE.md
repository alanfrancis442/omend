# CLAUDE.md

Shared instructions for every coding agent live in AGENTS.md and are imported
here. This file adds Claude Code specifics and the invariants established by
the pre-release security audit.

@AGENTS.md

## Hard safety boundaries

This project drives cooling hardware as root. Without explicit, per-action
approval from the owner, never:

- run `omend --system`, `omend --recover-fans`, or start/stop/restart
  `omend.service`;
- use `sudo`, run `packaging/install-system.sh` or `manage-system-files.py`
  against `/`, or install, enable or remove services or the `.deb`;
- `insmod`/`modprobe`/`rmmod` the kernel module, or run `make install` in
  `kernel/omen-kbd`;
- write to `/sys`, `/etc/omend`, `/etc/dbus-1` or `/run/omend.lock`;
- run thermal, stress or fan-stop experiments.

Building, testing, linting and the repository's temp-dir test scripts are safe.
Build the kernel module only in a scratch copy, not in `kernel/omen-kbd`.
Passing tests never prove hardware safety. Say so rather than implying it.

## Validation before declaring work done

```sh
cargo fmt --all -- --check
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline   # must be warning-free
sh packaging/test-install.sh                # also runs the Python packaging tests
python3 -I packaging/test-deb-guards.py
python3 -I packaging/check-markdown-links.py .   # ignore node_modules/ hits
uvx --from reuse==6.2.0 reuse lint          # must stay fully compliant
```

The `dbus-daemon ... Rejected send message` lines in test output are expected:
the policy tests confirm that unauthorized setters are denied.

## Invariants to preserve

- **D-Bus authorization** lives in `packaging/dev.omend.conf`: only root may own
  `dev.omend`. Setters are limited to root and the `omen` group, and everyone
  else gets read-only methods. A new method must be added to the policy and to
  the denial test in `crates/omend/src/tests.rs`.
- **Input validation** happens in the daemon for every D-Bus argument (bounded
  vectors, ranges, parsed names). Never build a filesystem path from caller
  input; sysfs paths come only from discovery.
- **Fail-safe**: any exit path must hand the fans back to the firmware. The
  unit's `ExecStopPost=--recover-fans` covers SIGKILL and crashes; keep it and
  `TimeoutStopSec`.
- **Root config** is read only from `--config` or `/etc/omend/config.toml`,
  never from the working directory. Config saves stay atomic and refuse
  symlinks.
- **systemd hardening** in `packaging/omend.service` is intentional; each line
  has a comment saying why. If you change it, update the "Service confinement"
  section of `docs/system-install.md`. `ProtectKernelTunables` must stay off,
  because it makes sysfs controls read-only.
- **Kernel module**: keep the DMI gate (`force=1` override), root-only
  attributes and the GPL-2.0-only SPDX header with `MODULE_LICENSE("GPL")`.
- **Scripts**: POSIX `sh` with `set -eu`, quoted variables, `mktemp` with trap
  cleanup, `umask 022`, and no dependence on `target/` or the source checkout at
  install time.
- **CI**: actions pinned by commit SHA, `permissions: contents: read`,
  `persist-credentials: false`, no `pull_request_target`, and untrusted
  `${{ }}` values passed through `env:` only.
- **Licensing**: every new file needs an SPDX header or a `REUSE.toml` entry.
  Rust code is `MIT OR Apache-2.0`; the kernel module is `GPL-2.0-only`.

## Privacy and publishing

- Never commit home-directory paths, personal email addresses, hostnames,
  serial numbers, MACs or UUIDs, including in hardware logs under
  `docs/hardware-results/`. Redact before adding evidence.
- Do not invent contacts or claims. Security reports go through GitHub private
  vulnerability reporting at <https://github.com/alanfrancis442/omend>.
- The known-issue warning about fans staying at 0 RPM after firmware Auto must
  stay in the README and CHANGELOG until it is resolved on hardware.

## Git

- The default branch is `main`. Branch for non-trivial work; commit or push only
  when asked.
- Commit as `alanfrancis442 <119670162+alanfrancis442@users.noreply.github.com>`
  (already set in the repository's git config).
- Do not add `Co-Authored-By` or other AI attribution trailers to commits.
- Never push the local `pre-squash-backup` branch; it holds pre-release history
  containing personal details.
