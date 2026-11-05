# Changelog

## Unreleased — experimental preview

No supported public binary release has been published by this checkout.

- Rust workspace with system hardware daemon, CLI, Slint desktop GUI and optional
  session audio-lighting helper.
- Local release bundles with installation instructions and project licenses.
- Explicit system D-Bus control authorization for root and the `omen` group;
  session daemons are read-only.
- Curve validation, critical-temperature override for manual curves, exclusive
  daemon ownership and recovery/retry handling with simulated regression tests.
- Experimental maximum-fan recovery option, disabled by default and not physically
  validated.
- Atomic daemon configuration replacement with persistence failures returned to
  clients and exposed in status; malformed existing configuration is preserved.
- Pinned Rust build toolchain, configured Linux CI/candidate workflows, staged
  installer checks and explicit source/license gates for official candidates.
- Public contribution, security, privacy and hardware-support documentation.
- Updated `pulldown-cmark` (transitive, via Slint) to 0.13.6 for RUSTSEC-2026-0414
  and RUSTSEC-2026-0433; hosted CI and security checks pass on Ubuntu 24.04.

### Known issues

- After firmware Auto is requested (daemon stop, crash recovery or removal),
  both fans on the OMEN 16-xf0xxx were observed at 0 RPM at 83°C. Unresolved;
  see the README's known issues for precautions.

Hardware validation is incomplete. The recorded OMEN 16-xf0xxx firmware-Auto
shutdown observation aborted at its temperature guard; its root cause remains
unresolved. See [hardware validation](docs/hardware-validation.md) for exact
candidate identities and evidence. Source features listed here are not a claim
that the currently installed host service contains them.
