# Optional experimental keyboard module

`kernel/omen-kbd` is an out-of-tree GPL-2.0-only module that exposes supported HP
keyboard zones as Linux LED interfaces. It is separate from the Rust workspace
and excluded from the portable userspace bundle. Fan control does not require
this module. First inspect existing LED nodes; a module already supplied by your
kernel may be sufficient.

The source recognizes one-zone and four-zone keyboard types. It rejects per-key
keyboards, absent backlights and unknown types. Recognition is not a tested
hardware compatibility guarantee. See [hardware support](support-hardware.md).

Before calling any keyboard method, the module requires the HP BIOS WMI GUID and
DMI data with system vendor `HP` and a product name containing `OMEN` or
`Victus`; otherwise it refuses to load with `ENODEV`. The `force=1` module
parameter skips only the DMI check and logs a warning. It is not a supported
configuration. The module also refuses to stay loaded when the firmware probe
rejects the keyboard, rather than leaving an unbound platform device behind.

The LED class files (`brightness`, `multi_intensity`) keep the LED core's
root-only write permissions. The additional
`/sys/devices/platform/omen-kbd/backlight` file, which reports whether the
backlight key has turned the lamp off, is readable only by root (mode `0400`)
because every read evaluates a firmware method. `omend` reads it as root.
The module has no writable attributes of its own.

The in-tree `hp-wmi` driver does not register these zone LEDs on the kernels
reviewed here. If a future `hp-wmi` registers the same LED names, the LED core
renames the later device with a suffix, and both drivers would drive the same
firmware method. Do not load this module on such a kernel.

## Build without installing

Install development headers matching the exact target kernel and the distribution's
kernel build tools. From the repository root:

```sh
make -C kernel/omen-kbd
```

By default the Makefile uses `/lib/modules/$(uname -r)/build`. For a different target
header tree use `make -C kernel/omen-kbd KDIR=/path/to/kernel/build`. A successful
build does not demonstrate safe module loading or keyboard behavior. Kernel APIs
change; rebuild for each target kernel rather than copying an arbitrary `.ko`.

## Installation and maintenance limits

This project does not currently provide a validated DKMS package, automatic
kernel-upgrade rebuild, Secure Boot signing enrollment procedure, or a signed
module. Secure Boot/module-signature enforcement may reject an unsigned module.
Do not disable platform security merely to load it. Distribution-specific signing
and enrollment instructions require separate validation and are release gates.

The Makefile's `install` target builds, writes into the target kernel's module
tree and invokes `depmod`; it is a host-changing action, not a development check.
Loading/unloading also changes running kernel state and can interact with other
keyboard drivers. No automated install/load command is recommended until the
kernel, model, signing requirements and recovery plan have been reviewed.

Keep the exact build source and installed file path if you deliberately perform
a manual installation. A future kernel update may make the module unavailable.
Before removal, disable any local module auto-load configuration and arrange a
safe maintenance window; remove only the recorded module file and refresh the
target kernel's module dependency database. Unloading a module in use may fail.
The userspace uninstall procedure does not remove manually installed kernel modules.

## Compile and source-review evidence — 2026-10-19

The current source compiled successfully with `make -C kernel/omen-kbd W=1`
against the already installed matching `7.1.8-arch1-3` headers, using
`cc (GCC) 16.2.1 20260810`. The build completed CC, MODPOST, link and BTF steps
without warnings in `/tmp/omen-kernel-build.log`. The local output's SHA-256 was
`51663dbe4d4274be67f835d498fe92cd945169578aab5ef722ea664daeb3b362` and its
vermagic was `7.1.8-arch1-3 SMP preempt mod_unload`. This identifies a local
compile artifact, not a portable or signed binary. It was **not installed or
loaded** and no keyboard or other hardware writes were performed.

A targeted source review found and addressed two concerns:

- Delayed work could survive a partially successful probe that subsequently
  failed: the normal remove callback is not invoked for that failure. A managed
  cleanup action now cancels work after later LED resources unregister and before
  the managed state is freed. Successful removal also retains early cancellation.
- A short successful color-table response was padded with zeros before a later
  read-modify-write, which could overwrite unreturned firmware table bytes. Such
  truncated color reads are now rejected. Write acknowledgements retain their
  existing variable-payload behavior; null ACPI buffer pointers are rejected.

The largest fixed color offset is `25 + 3 * 3 + 2 = 36`, within the 128-byte
table, and registered zone counts are limited to one or four. Callers currently
use fixed input/output sizes. This targeted review is not a complete proof of
WMI protocol correctness, buffer safety or concurrency behavior. Failed-probe
fault injection, firmware response variants, simultaneous access with other
drivers, actual unload/suspend behavior and physical lighting remain unvalidated.

[Compile-only CI](../.github/workflows/kernel.yml) installs Ubuntu 24.04's generic
kernel headers and compiles for that header tree, which can differ from the
hosted runner's running kernel. A callback compatibility branch handles the
platform-driver remove API transition visible in the upstream
[Linux 6.8 header](https://github.com/torvalds/linux/blob/v6.8/include/linux/platform_device.h)
and [Linux 6.11 header](https://github.com/torvalds/linux/blob/v6.11/include/linux/platform_device.h).
No hosted run has passed yet in this record. Compile coverage does not create a
supported kernel range or authorize loading; the userspace preview still excludes
the module from its binary bundle/support claim.

## Security review — 2026-11-03

A second source review covered sysfs handlers, permissions, firmware-call
buffers, teardown, suspend, init error paths, DMI gating and licensing. Changes:

- A firmware status code above `INT_MAX` was converted to a negative `int`,
  which callers could return to userspace as an arbitrary errno. Status codes
  are now clamped to a positive value and reported as `EIO`.
- The `backlight` attribute was world-readable and each read evaluated an ACPI
  method under the driver mutex, letting any local user generate unbounded
  firmware calls. It is now `0400` and registered through the driver core's
  `dev_groups`, so it appears and disappears with the bound device instead of
  being created after probe and ignored on failure.
- Delayed color writes could fire on the system workqueue in the middle of a
  suspend transition. A sleep callback now runs any pending write before the
  device suspends. Resume relies on the LED core restoring each zone.
- The module loaded on any HP system exposing the BIOS GUID, and stayed loaded
  with an unbound device when probe failed. It now requires OMEN/Victus DMI
  data (overridable with `force=1`) and fails module load if probe fails.
- The Makefile's `install` target ran `depmod -a` for the running kernel even
  when `KDIR` named another kernel; it now uses the target tree's release.
- The SPDX license identifier is on line 1, as kernel tooling expects.
  `MODULE_LICENSE("GPL")` matches the `GPL-2.0-only` file header and the
  `REUSE.toml` annotation; the kernel uses `"GPL"` for GPL version 2 without
  distinguishing "only" from "or later".

Reviewed without change: the module defines no store handlers, uses
`sysfs_emit` for its one show handler, bounds all fixed color-table offsets
within the 128-byte table, zero-initializes request buffers, rejects short
color-table reads, serializes all firmware calls on one mutex, and cancels
delayed work before freeing state on both remove and failed probe. Turning the
backlight off during suspend clears the "turned off by key" latch, so a lamp the
user switched off with the key may come back on after resume. This is a usability
limitation, not a safety issue.

The revised source compiled with `make W=1` in a scratch copy against the
installed `7.1.8-arch1-3` headers with no warnings; `checkpatch.pl --no-tree`
reports only the intentional `LINUX_VERSION_CODE` compatibility branch. The
revised module was **not installed or loaded** and no hardware writes were
performed. Suspend/resume, DMI matching on other models and physical lighting
remain unvalidated.
