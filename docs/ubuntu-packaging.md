# Experimental Ubuntu package recipe

`packaging/build-ubuntu-deb.sh` is a new, **unvalidated experimental recipe** for
Ubuntu 24.04 amd64. It is not a claim of distribution support. It rejects other
distributions, non-amd64 Rust hosts and a build glibc other than 2.39 so that the
Arch/glibc-2.44 development binaries cannot be mislabeled as Ubuntu packages.
It always performs a native locked source build; it does not import host tarball
binaries. The optional keyboard kernel module is excluded.

The configured [manual CI workflow](../.github/workflows/ubuntu-package.yml) installs
build dependencies, builds the package, inspects control/contents and extracts
it to a temporary directory. It does not install it, activate services or publish
it. A successful hosted workflow and fresh-system transaction tests are pending.
No `.deb` was built on the Arch development host, which lacks the matching build
baseline and dpkg packaging tools.

## Build on the declared baseline

Use the pinned Rust toolchain and native dependencies in that workflow, plus
`dpkg-dev`. Set `OMEND_DEB_MAINTAINER` to the actual package-maintenance identity;
for a strictly local disposable CI build the workflow uses a clearly local-only
`root@localhost` identity. This is not a project security/support contact and must
be replaced before publication. No account creation is required for local source
preparation.

```sh
sh packaging/build-ubuntu-deb.sh
```

`--offline` applies to Cargo after dependencies have been fetched. Outputs go to
`dist/` with a `~preview+git…` version and dirty suffix when applicable. The build
generates and includes third-party notices but does not resolve outstanding
license-review gates. Do not distribute a package before that review.

`dpkg-shlibdeps` derives ELF-linked runtime requirements from the actual four
binaries and the build system's package database; missing dependency information
fails the build. Explicit display/font dependencies cover libraries loaded at
runtime that an ELF scan may not discover. Audio recommends PulseAudio utilities;
PipeWire's PulseAudio compatibility service is optional. These declarations need
verification in a fresh desktop environment, not merely extraction.

## Deliberately manual service lifecycle

All four maintainer hooks refuse transactions when `omend.service` is active,
activating, deactivating or unqueryable, or when any `omend` process remains.
They do not stop/start/reload services, change group membership or write hardware.
This means the package deliberately requires operator preparation and can refuse
an ordinary unattended upgrade. Service/process checking is not a lock against
a separate administrator starting a daemon concurrently; use an exclusive
maintenance window.

Do not stop the controller merely to satisfy the guard while safe firmware
cooling is unresolved. Arrange a specific monitored cooling/recovery plan first.
The previous firmware-Auto test aborted; no new installation or physical test is
authorized by implementing this recipe. `Pre-Depends` supplies systemd/procps for
the guard; systemd-less/chroot installation is intentionally not supported.

The package installs the daemon, CLI, GUI and audio helper into `/usr/bin`, the
system service into `/usr/lib/systemd/system`, and an inactive user audio unit
into `/usr/lib/systemd/user`. Existing manually installed `/etc/systemd/system`
or per-user units can override these files and require deliberate migration.
The daemon config and D-Bus policy are dpkg conffiles, preserving local edits
through normal upgrade prompts; purge intentionally removes conffiles. Review a
retained old policy because it may keep permissive access rules.

Create the trusted `omen` group and select its members explicitly, as in
[system installation](system-install.md). D-Bus/systemd reload, any enable/start,
user audio activation and launcher migration remain separate reviewed operator
steps. The package does not promise rollback of those manual changes.

## Checks completed and still required

Shell syntax and nine mocked inactivity/error guard scenarios pass locally;
the tests invoke no real service/process tools. The non-Ubuntu build guard
rejects the current host before Cargo or package mutation. Actual dpkg package
construction, ELF dependency extraction, clean Ubuntu installation/upgrade/
remove/purge, configuration prompts, rollback, GUI/audio runtime and service
confinement remain pending. Those results and hardware validation are required
before describing this as a supported distro package.
