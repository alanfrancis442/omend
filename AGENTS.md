# Working in this repository

This is a Linux Rust workspace for HP OMEN/Victus hardware control. Use the
existing Cargo.lock and preserve unrelated working-tree changes.

## Release direction

Prepare changes for a public, open-source release. Keep installable artifacts
independent of the source checkout and Cargo's `target/` directory. Use locked
release builds, include licenses and installation instructions in bundles, and
keep generated packages under `dist/`. Do not clean build output before checking
whether local launchers or services refer to it. Treat local binary bundles as
host-specific until compatibility and hardware behavior have been validated.

## Layout

- `crates/omen-common`: shared configuration, types, fan curves, sensor access,
  lighting effects, and D-Bus contract.
- `crates/omend`: privileged hardware daemon; `engine.rs` applies hardware
  changes and `iface.rs` exposes the D-Bus interface.
- `crates/omen-cli`: command-line client.
- `crates/omen-gui`: Slint desktop client; UI lives in `ui/app.slint`.
- `crates/omen-audio`: user-session audio capture, spectrum analysis, and a
  session D-Bus preview service. It sends levels directly to `omend`; the GUI
  only observes its spectrum. Packaging includes a systemd user service.
- `kernel/omen-kbd`: optional out-of-tree keyboard backlight module.
- `config/omend.toml`: sample daemon configuration.
- `packaging/` and `docs/system-install.md`: system installation and removal.

## Development setup

Use a Rust toolchain supporting edition 2024, with `rustfmt` and `clippy`:

```sh
rustup component add rustfmt clippy
cargo build --workspace --locked
```

The desktop build needs Linux native development libraries appropriate to the
Slint backends (including font, windowing, and graphics dependencies).
`pactl`, `wpctl`, `wl-copy`, and `wl-paste` support desktop integrations.
Kernel module builds additionally require headers matching the target kernel.
Node/npm are used only for graft tooling; install its locked dependencies with
`npm ci` if absent.

## Navigation and validation

Use graft first for source navigation. `graft map` provides the workspace map;
`graft ask "<task>" --source` locates code; `graft callers <symbol>` checks
callers before symbol changes. For multi-file refactors, use `--depth all`.
The local graph can be regenerated with `graft build`.

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked
```

Add `--offline` when dependencies are already cached. Tests do not establish
that hardware writes, system D-Bus access, or the graphical session work.

Treat installation and hardware exercise as separate from development setup:
running the daemon, changing fan/lighting settings, loading kernel modules,
and following the sudo commands in the installation guide affect the host.
Ordinary builds, tests, and linting do not require service installation.
