# Release validation and sign-off

This document defines the evidence required to close release gates. It is not a
record that those checks have passed. The [release checklist](release-checklist.md)
tracks actual completion; [hardware validation](hardware-validation.md) records
the physical observations. No supported public binary release is signed off yet.

The current backlog work covers source changes and isolated automated checks.
It does **not** authorize another installation, service restart, fan/profile
change, module load, suspend, induced fault or load test on the host. The previous
controlled shutdown test aborted and rolled back. A new physical test needs a
specific reviewed candidate, recovery/abort plan and explicit operator agreement.

## Source and automation gate

Record the full commit SHA, matching workspace version, proposed tag and review.
The official bundle path must reject a dirty checkout or a tag not identifying
that commit. Local dirty bundles remain development artifacts. Rust 1.99.0 is
the selected tested baseline, not a proven minimum supported Rust version.

From a clean Linux checkout, retain results for:

```sh
cargo fmt --all -- --check
cargo test --workspace --release --locked
cargo clippy --workspace --all-targets --release --locked -- -D warnings
```

Use the CI native dependencies and toolchain recorded in the workflows. Tests
must run unprivileged with a private D-Bus daemon and fake hardware, without
installing services. A workflow file's existence is not a passing hosted CI run;
attach the actual run identity and logs to the reviewed revision. Root-skipped
lifecycle coverage does not replace the unprivileged run.

### Configured native build setup

The CI workflows select Ubuntu 24.04 with these native packages:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config cmake clang \
  libfontconfig1-dev libfreetype6-dev libxcb1-dev libx11-dev libx11-xcb-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libegl1-mesa-dev \
  libudev-dev dbus desktop-file-utils
```

These are development/validation dependencies, not a complete binary runtime
package manifest. Installing them changes the system; CI does so inside its job
environment. Hosted runs passed on 2026-11-04 (see the release checklist);
logs are retained by GitHub for 90 days. Use `rustup show active-toolchain` to confirm the
repository pin before running Cargo. The native bundle builder explicitly uses
the Rust host target and reads binaries from `target/<host>/release`; ordinary
Cargo source-build examples use `target/release` when no target override is set.

Future native `.deb`/`.rpm` packages need distribution-specific dependency and
service-lifecycle validation. The current archive remains a host-specific tarball.
An [experimental Ubuntu `.deb` recipe](ubuntu-packaging.md) is available for
compile/package CI; it has not yet established package/runtime support.

## Security and licensing gate

Retain dependency/advisory and secret-scanning reports for the locked graph and
all Git history being published, including scanner version, database revision and
time. Resolve findings or record a justified, scoped acceptance with an owner and
expiry. Network/database failures are failed or incomplete checks, not a clean
audit. Automated secret scans do not replace review of personal paths and logs.

Record the selected Slint license route, applicable exact dependency license
texts and attribution obligations. Ship generated third-party inventory/notices
and required license texts; review omissions and unusual license expressions.
A license-review environment variable in the builder is an attestation mechanism,
not independent proof of legal compliance. Establish a verified private security
contact and canonical public source URL before publication.

## Artifact and installation gate

Build the exact reviewed revision/tag. Retain `BUILD.txt`, content manifest,
archive SHA-256 and the toolchain/architecture/native-library baseline. Check the
extracted bundle's contents and local documentation links, CLI help, linked
libraries and temporary-root install/upgrade/removal checks. These checks must
not change running host services or hardware.

Separately test a fresh supported target system/VM with no source checkout:

- Install the root service and matching D-Bus policy, checking access with real
  trusted/untrusted users and membership changes.
- Launch the GUI as an ordinary user and verify menus/icon, display backend,
  status/error behavior and optional audio helper in its actual session.
- Upgrade with non-default configuration, verify preservation, and exercise
  documented rollback and removal without deleting unrelated user data.
- Remove the extracted bundle and source/build cache after checking launchers;
  verify installed clients/services still resolve installed paths.
- Record the target's distribution, architecture, glibc/runtime libraries,
  systemd/D-Bus implementation and display/audio services. One development-host
  success establishes none of the other target environments.

VM checks establish packaging/session behavior, not physical HP hardware safety.
Applying daemon confinement directives requires deployed validation of discovery,
writes, config persistence and recovery; parser acceptance alone is insufficient.
The optional kernel module remains separately experimental and excluded from a
userspace support claim unless its own build/sign/load/lifecycle checks pass.

## Hardware gate

The recorded Auto shutdown check remains unresolved. Physically validate the
experimental maximum request at low workload with an operator and independently
available recovery before using it to justify later fault tests. Do not infer
cooling from a successful sysfs write or cached mode readback. Verify both fans
and temperature trends and preserve the exact candidate/configuration identity.

Follow the staged sequence in [hardware validation](hardware-validation.md), with
model-specific abort limits, rollback and synchronized observations. Presets,
AC/battery, suspend/resume, failure recovery and bounded load remain separate
checks. SIGKILL, hangs and blocked kernel I/O need recovery independent of daemon
cleanup and should not be induced on an everyday-use machine. Do not raise a
failed test's guard merely to obtain a pass. The 100°C manual-curve override is
a software policy, not evidence of a safe threshold for all supported models.

No laptop model is currently signed off. Publish results only for the exact
tested model/BIOS/kernel combination and distinguish incomplete, aborted and
passing checks. Update the [support matrix](support-hardware.md) from reviewed
evidence rather than broadening a claim to all OMEN/Victus devices.

## Candidate release record

Complete this record before publishing a supported binary:

| Evidence | Required record |
| --- | --- |
| Source | Full SHA, version/tag, clean checkout, review |
| Automated validation | Hosted CI run and retained test/lint/build logs |
| Security | Current advisory/history scan reports and finding dispositions |
| Licensing | Chosen routes, reviewed revision, notices and license texts |
| Artifact | SHA-256, build metadata, contents verification and source archive |
| Compatibility | Declared target baseline and clean-system installation results |
| Hardware | Model/BIOS/kernel, reviewed traces, abort limits and recovery results |
| Reporting | Canonical source URL and tested private security channel |
| Publication | Verified downloadable files, instructions and known limitations |

A narrowly scoped experimental source preview may state its unresolved gates
explicitly. It must not be described as a supported hardware-safe binary release.
