# Public release readiness checklist

Reviewed: 2026-10-19. **Local release preparation is substantially complete;
physical safety and public release sign-off remain open.** This is not a supported
binary release or a complete security audit. The owner explicitly requested that
hardware tests stay pending and will create release accounts later.

## Completed local work

- [x] System-bus authorization for root and the `omen` group; other users receive
  status access. Simulated policy tests and narrow separate-user checks on this
  host cover allowed reads, denied mutations and group/root control. Broader
  broker/session validation remains below.
- [x] Exclusive daemon ownership before hardware writes, read-only session
  daemons, bounded fan curves, critical-temperature override for manual curves,
  sensor/write-failure handling and normal-shutdown recovery requests.
- [x] Separate experimental maximum-fan recovery with explicit opt-in; disabled
  by default. Tests cover partial writes, retry, sensor recovery, mode-only
  discovery, read-only access and saved settings. This is not physically validated.
- [x] Atomic configuration replacement with file/directory sync, preserved Unix
  ownership/mode, malformed-file preservation and observable D-Bus save errors.
  Symlinks, hard links, nonregular destinations and extended attributes/ACLs are
  refused rather than silently altered. Tests cover partial-write failure and
  permission errors; actual full-storage and power-loss behavior remains open.
- [x] **72 locked release tests pass**, including private D-Bus and process
  lifecycle checks. Local evidence: `/tmp/omen-backlog-tests.log`.
- [x] Workspace formatting passes and release Clippy passes with **warnings
  denied**. Evidence: `/tmp/omen-backlog-clippy.log`.
- [x] Rust 1.99.0 pinned as the tested toolchain/baseline, with native build
  dependencies documented. This does not establish the oldest possible MSRV.
- [x] CI, security scans, compile-only optional-module checks and manually
  dispatched candidate workflows configured. Actions/scanner versions are
  pinned; actual hosted execution remains open.
- [x] Native builder uses an explicit target, locked release build, full revision,
  provenance, file/archive checksums, normalized archive metadata and unique
  dirty-candidate names. Full binary reproducibility remains unproven. Official
  candidate mode requires a clean matching version tag and exact-commit license
  review attestation. No tag, commit or publication has been created by this work.
- [x] File-only installer, private backup/removal/restore tooling, icon and desktop
  entry provided. Staged fresh install, config-preserving upgrade, backup,
  removal and rollback checks pass without service activation or hardware writes.
- [x] The obsolete local desktop launcher now points to `/usr/bin/omen-gui`, with
  its previous file backed up. It no longer depends on Cargo's build output.
- [x] Minimal systemd confinement reviewed and packaged while retaining sysfs
  writes. Real confined-unit/hardware validation remains open; the unit has not
  been installed on the host.
- [x] Slint royalty-free desktop route selected; actual `AboutSlint` widget added
  to a top-level About page. See [Licensing](licensing.md).
- [x] Reproducible offline inventory for 472 resolved packages, package and nested
  asset/license texts, pinned upstream supplements and hashes. All 391 packages
  in the default active native dependency tree have inventoried license texts.
  Three missing-text entries are inactive optional packages; new features require
  reassessment. Inventory generation is not legal clearance.
- [x] REUSE 6.2.0 project SPDX coverage passes. Imported license texts retain their
  original contents and separate attribution metadata.
- [x] Cargo-audit 0.22.2 reports **zero known vulnerabilities** using current
  RustSec data; five unmaintained-dependency warnings remain visible. Gitleaks
  8.30.1 reports zero findings in 15 reachable commits and current nonignored
  working files. See [Security audit](security-audit.md) for exact scope and limits.
- [x] README, contribution/security policies, issue templates, privacy guidance,
  hardware evidence matrix, unreleased changelog and release validation gates
  added. Editor hook configuration (`.cursor/`) is untracked. The canonical
  repository is <https://github.com/alanfrancis442/omend>; vulnerabilities go
  through GitHub private vulnerability reporting, which must be enabled in the
  repository settings and tested after the first push.
- [x] Headless GUI rendering checks pass for Dashboard, Settings and About at
  desktop and compact sizes. A labeled synthetic Settings preview is included;
  the software renderer does not faithfully render rotated dashboard gauges.
  Interactive accessibility and real graphics-session checks remain open.
- [x] Experimental Ubuntu 24.04 amd64 `.deb` recipe and manual CI configured;
  nine mocked package service guards pass. Actual Ubuntu construction and
  package lifecycle validation remain pending.
- [x] Optional kernel-module compile coverage and failed-probe cleanup/color-read
  robustness reviewed. Module remains excluded from the userspace binary bundle;
  build evidence and limitations are in [Kernel module](kernel-module.md).

## Current host and artifact state

The previously tested daemon/policy/configuration were rolled back after the
Auto handoff observation aborted. The older installed service and policy remain
active. New source safeguards and packaged confinement are not installed. The
only permanent application-integration change during this batch was the backed-up
user desktop launcher repair.

The final local candidate identity and checksum are recorded separately in
`dist/local-candidate.json` after archive verification. Earlier physical-test archive
hashes in [Hardware validation](hardware-validation.md) describe that earlier
candidate and must not be used as evidence for the current source.

## Remaining hardware gates — pending at the owner's request

- [ ] Independently review `packaging/hardware-recovery.py` and its failure
  transitions before preparing or arming it on the host. Its temporary-fixture
  checks do not establish recovery on a running systemd service or physical fans.
- [ ] Exercise the recovery helper's file restoration through a disposable
  systemd service. The isolated Docker runtime check covers retry after a
  fixture helper fails and timer cancellation; it does not execute the real
  recovery helper's rollback path.
- [ ] Physically validate maximum mode and rollback at low workload with an
  operator and an independent recovery mechanism. The non-writing planner can
  prepare a reviewable sequence; it is not an executable physical test or sign-off.
- [ ] Resolve firmware Auto handoff behavior. The recorded stop observation
  reached 83°C with both fans reading zero; this exceeded the test guard, not a
  demonstrated vendor failure threshold. Cause and eventual firmware response
  remain unproven. See [Driver investigation](hardware-results/2026-10-19-driver-investigation.md).
- [ ] Validate sensor failures, frozen plausible readings, blocked I/O and
  rediscovery on dedicated hardware. Current software cannot guarantee recovery
  from a hung kernel/sysfs call or a daemon killed before cleanup.
- [ ] Validate deployed authorization across supported D-Bus brokers, membership
  changes, remote/background sessions and audio reconnection.
- [ ] Establish model/BIOS/kernel support and thermal limits; cover AC/battery,
  suspend/resume, stop/restart and unsupported devices. Do not heat the everyday
  laptop to the critical threshold to test the override.
- [ ] Only after recovery passes, run bounded load and crash/hang tests with an
  independent recovery mechanism. Unit tests do not close these items.

## Remaining release sign-off

- [ ] Fresh supported VM/system: extracted-archive installation, normal-user GUI,
  About attribution visibility, audio service, menu/icon, configuration upgrades,
  removal and rollback without the source tree or build cache.
- [ ] Run the packaged confined system service on validated hardware. Config files
  carrying ACL/security xattrs currently fail saves explicitly; broader metadata
  support and actual ENOSPC/power-loss tests need separate work.
- [ ] Establish supported architecture/distro/kernel/runtime baseline using the
  clean CI build. The development-host bundle uses glibc 2.44 and is host-specific;
  configuring Ubuntu 24.04 CI does not establish compatibility before it runs.
- [ ] Review exact feature-specific redistribution obligations, optional package
  text gaps and native libraries/assets; record a reviewer and source revision.
  Native Skia requirements apply if that currently inactive renderer is enabled.
- [ ] Create the public repository/release accounts and a verified private security
  reporting route, then populate real package repository metadata and docs.
- [ ] Commit the reviewed tree, select preview/stable scope and version, create the
  corresponding clean tag, and run hosted CI/security checks. Refresh audits at
  release time; no current vulnerability ignore rules have been added.
- [ ] Publish only the reviewed candidate with source, checksums, compatibility
  limits, license notices, known issues and provenance; verify downloads from a
  separate machine/account. Publication is not performed by local preparation.

## Later work / scope decisions

- [ ] Migrate the five unmaintained dependencies through reviewed upstream/API
  changes; informational advisories are not treated as five exploitable findings.
- [ ] Execute the experimental Ubuntu package CI and fresh-system lifecycle
  tests before claiming Ubuntu support; add other distro packages only against
  validated dependency and service-lifecycle baselines.
- [ ] Expand interactive GUI accessibility/visual checks, hardware reports and kernel-version
  coverage. Optional module DKMS, Secure Boot signing and physical WMI validation
  remain outside the userspace-only preview.
- [ ] Add release signing with an owner-controlled identity, provenance verification
  and reproducibility evidence; no signing key or identity has been invented.

## Artifact and sign-off record

Record the final archive path/SHA-256, source tag, CI runs, dependency/license
review, fresh-system lifecycle results and model-specific evidence here. A green
local build does not waive pending hardware or owner/publication gates.
