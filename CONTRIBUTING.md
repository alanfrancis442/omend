# Contributing

This project is preparing an experimental public preview. Hardware compatibility
and cooling recovery remain under investigation; do not describe a model as
supported based on detection or unit tests alone.

## Development

Read [AGENTS.md](AGENTS.md) for workspace layout and local conventions. Preserve
`Cargo.lock`, keep changes focused, and build with `--locked`. Use `npm ci` only
if working on the optional graft navigation tooling; Node is not an application
runtime requirement.

```sh
cargo fmt --all -- --check
cargo test --workspace --release --locked
cargo clippy --workspace --all-targets --release --locked
```

Use `--offline` only when dependencies are cached. Tests should run as an ordinary
user with `dbus-daemon` available. Builds, linting and tests do not require system
service installation. Prefer fake-sysfs and private-bus regression tests for
hardware/error paths. They verify requests and state transitions, not cooling.

## Changes and review

Open a pull request against `main`; direct pushes and force-pushes are
restricted. The required checks are `linux` (build, tests, fmt, clippy),
`compile` (optional keyboard module), `dependencies` (cargo-audit) and
`secrets` (gitleaks). Workflows from first-time external contributors need
maintainer approval to run. Changes under `crates/omend/`, `packaging/`,
`kernel/` or `.github/` get extra scrutiny because the daemon runs as root and
drives cooling hardware; the code owners are requested automatically.

Explain the problem, resulting behavior, validation, and remaining limitations.
Add regression tests for safety or authorization changes. Keep generated archives
under ignored `dist/` and build output under ignored `target/`. Never commit
machine-specific logs, serial numbers, credentials or personal checkout paths.
New project code should follow the existing SPDX headers; do not change the
kernel module's GPL-2.0-only license.

No contributor license agreement is required. Unless explicitly stated otherwise,
contributions to project userspace and documentation are offered under
MIT OR Apache-2.0, and contributions to `kernel/omen-kbd` under GPL-2.0-only,
matching the files they modify. Contribute only material you have permission to
license that way and preserve third-party notices.

Installation, service restarts, custom fan settings, loading modules and fault
tests affect the host. Keep them separate from development checks. Follow the
[hardware validation sequence](docs/hardware-validation.md) with an operator and
rollback plan; do not run destructive fault or thermal load tests just to submit
a patch.

## Reporting problems

Use the issue tracker at <https://github.com/alanfrancis442/omend/issues> for
ordinary bugs and feature requests. There is no dedicated support service or
response-time commitment.

Include the source revision or artifact checksum, distribution, kernel, BIOS,
product family/model, reproduction steps, expected/actual behavior and whether
the daemon or policy was locally modified. For hardware reports, use the
read-only commands in [hardware support](docs/support-hardware.md). Inspect logs
before sharing and include only the relevant interval. A safety failure report
does not require repeating the failed experiment.

For suspected vulnerabilities follow [SECURITY.md](SECURITY.md), not a public
bug report containing exploit details.
