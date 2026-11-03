# Local security scan record

Scanned on 2026-10-19 during local release preparation. The checkout's base HEAD
was `0b9eb6283ead64a10b3126cc072a870db18534db`; the working tree contained
uncommitted release-preparation changes. This record does not attest to a clean
tag or later edits and is not a complete security audit.

## Dependency advisories

`cargo-audit` 0.22.2 examined the existing `Cargo.lock` against RustSec database
commit `999ec89d5d3f10f20be14750ec8c2d8b44e0ac9b`, updated 2026-10-17, with
1,338 advisories. **No known vulnerabilities were reported.** This means no
match in that database at scan time; it does not establish the absence of defects.
The local structured report is `/tmp/omen-dependency-audit.json` and is not a
published or durable CI artifact.

Five informational unmaintained-dependency advisories were reported:

| Advisory | Dependency | Required follow-up |
| --- | --- | --- |
| [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html) | `bincode` | Trace the parent dependency and evaluate maintained serialization choices/upstream migration. |
| [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | `ttf-parser` | Track font-stack upstream migration to a maintained parser. |
| [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html) | `zbus_names` | Review migration with the D-Bus stack; upstream merged name support into zbus 6. |
| [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html) | `zvariant` | Review wire-format/API migration with the D-Bus stack. |
| [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html) | `zvariant_derive` | Review macros and generated protocol behavior during the same migration. |

These are maintenance warnings, not five reported exploitable vulnerabilities.
They remain visible and unresolved; no advisory ignore rules or dependency
version changes were added for this scan. Migration needs compatibility review,
locked dependency/license review and regression coverage, especially for D-Bus
authorization/ownership, serialized configuration and GUI/font behavior. A future
known vulnerability must be fixed or receive an explicit scoped risk decision;
the absence of vulnerabilities today is not a waiver for later findings.

## Secret scanning

Gitleaks 8.30.1 scanned all 15 commits reachable in the local Git history and a
staged copy of tracked plus nonignored untracked working files. Both scans
reported **zero findings**. The downloaded Linux x64 tool archive was checked
against its official release checksum manifest and GitHub release asset digest.
See the [official Gitleaks release](https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1).

Ignored files, unreachable/pruned Git objects, other repositories, external host
configuration, credentials stored elsewhere and future changes are outside that
scope. Pattern-based scans cannot detect every secret or privacy issue. Manually
review diagnostics, history and personal data before publication. Nothing was
sent to a public issue tracker or vulnerability-reporting service by these scans.

## Daemon and D-Bus review

A source review of `omend`, `omen-common`, the D-Bus policy, the systemd unit
and the sleep hook found no remotely reachable input. Mutating methods are
limited by `packaging/dev.omend.conf` to root and the `omen` group; other users
may call only the status getters, Introspectable and Peer. Every method argument
is bounded by its D-Bus type or validated before use (curve point count, range
and order; preset, profile, effect, motion and power-source names; zone IDs;
2–8 custom colors; saved-curve names and count). Sysfs paths come only from
discovery under `/sys`, never from callers. Configuration saves are atomic and
refuse symlinks, hard links and symlinked parent directories.

Changes made during this review:

- The unit adds `ExecStopPost=-/usr/bin/omend --recover-fans`, so fan recovery
  is requested even after SIGKILL, abort or a stop timeout (`TimeoutStopSec=15s`).
- The unit adds device, network, kernel-module/log/clock, namespace, realtime,
  personality, W^X and syscall (`@system-service`) restrictions, `UMask=0077`
  and a capability bounding set of `CAP_CHOWN CAP_DAC_OVERRIDE CAP_FOWNER`.
  These passed `systemd-analyze verify` only; startup, saves and sysfs writes
  under the new confinement still need validation on hardware.
- The root daemon no longer loads a configuration from its working directory,
  and warns about a configuration modifiable by non-root users.
- Hand-edited `hysteresis_secs` is bounded to 1–30 s, matching D-Bus input.
- Audio level updates are limited to one LED write per 15 ms.
- The sleep hook calls `/usr/bin/omen-cli` by absolute path with a 10 s timeout.

Accepted residual risks: authorization is by bus policy and group membership,
not polkit or an in-daemon credential check, so an outdated permissive policy
file grants control to every local user. `omen` members are trusted and can set
low-PWM curves up to the 100°C override, or trigger a configuration fsync per
persistent setter call. The unit stays root with sysfs writable and no
model-specific sysfs allowlist.

## Ongoing checks and publication

[Security CI](../.github/workflows/security.yml) installs pinned scanner versions,
fetches full history, checks the locked dependencies against the current RustSec
database and scans history plus nonignored working files with redacted findings.
The pinned Gitleaks download and checksum-manifest hashes originate from the
official release metadata; updating the tool requires reviewing those hashes.
No secret findings are uploaded as public report artifacts. A configured workflow
is not evidence that a hosted run passed; retain its future run identity.

Local source preparation and audits can finish before hosting/release accounts
exist. The canonical repository is <https://github.com/alanfrancis442/omend>, and
the private security contact is GitHub private vulnerability reporting; enabling
and testing it after the first push, a hosted CI run, a reviewed clean tag and
fresh release-time audits remain publication gates. Do not invent contacts or
treat this local record as a hardware safety sign-off. See [SECURITY.md](../SECURITY.md)
and [release validation](release-validation.md).
