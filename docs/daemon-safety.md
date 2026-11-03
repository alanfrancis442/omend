# Daemon safety and access

These are software safeguards. Hardware recovery, firmware behavior after a
crash, and thermal performance still require validation on each supported model.
The public release checklist remains open until that evidence exists.

On the OMEN 16-xf0xxx, the first controlled shutdown observation aborted at
83°C with both fans reporting zero after Auto was requested. Installed files
were rolled back. See [Hardware validation](hardware-validation.md) for the
trace summary; firmware cooling after shutdown is not validated on this model.

## Access policy

The system D-Bus policy permits control by root and members of the dedicated
`omen` group. Other users can call GetStatus, GetTemperatures, GetFanSpeeds,
Introspectable and Peer methods. All other calls to the daemon are denied by
default, including calls without an interface name. Explicit group membership
also permits background audio updates, config changes, and remote-session use;
this policy is not an active-desktop authorization policy.

Create the group and add only trusted users as described in
[System installation](system-install.md). Log out/in after changing membership.
When upgrading, install the new D-Bus policy as well as the daemon binary, reload
D-Bus, and restart the daemon. A binary update alone does not restrict callers
under an older permissive bus policy. Do not run the GUI or audio helper as root.

The root daemon reads only `--config` or `/etc/omend/config.toml`; it never
falls back to `config/omend.toml` in its working directory. It logs a warning
when that file or its directory is not owned by root or is group/other
writable, because whoever can edit it controls fan and lighting settings.
Audio level updates write at most one LED frame per 15 ms, so a flood of calls
cannot become an unbounded stream of sysfs writes under the engine lock.

Authorization is enforced by the system bus policy; the daemon is not a generic
service for export over an unrestricted bus. Root uses the system bus only.
Non-root session daemons are read-only and disable all sysfs writes, even if the
user has writable sysfs nodes. `omend --session` cannot be used as root.

## Startup and shutdown

The root daemon holds an exclusive kernel file lock on `/run/omend.lock` for its
lifetime, and claims the D-Bus service name before activating hardware. Never
delete a live lock file: replacing its inode defeats lock exclusion. The file
can remain after exit; the kernel releases its lock automatically.

Until activation, and after shutdown begins, the D-Bus interface rejects calls.
On SIGTERM, SIGINT, or an unexpected hardware-worker exit, the worker stops and
the daemon requests the configured recovery mode without overwriting the saved
fan preset. The default request is `pwm1_enable=2` (firmware Auto); experimental
maximum recovery is described below. A failed request is reported as an error.
Dropping an active engine also attempts recovery during ordinary unwinding.

SIGKILL, process abort, machine failure, and a hung process cannot execute this
cleanup in the daemon itself. The packaged unit therefore runs
`omend --recover-fans` as `ExecStopPost` after every exit, including a crash,
SIGKILL, or the 15-second stop timeout for a hung daemon. That one-shot command
requests the same configured recovery mode, refuses to act while any daemon
holds `/run/omend.lock`, serves no D-Bus interface, and its failure does not
change the unit's result. It cannot help after machine or kernel failure, or
when systemd itself does not run. Do not assume a firmware watchdog will
recover correctly without model-specific testing. Manual emergency recovery is to restore firmware fan
control through the supported hp-wmi interface or reboot; exact paths vary by
device. Never test failure recovery under heavy load without a recovery plan.

## Thermal behavior

- Curves contain two to eight strictly increasing temperature points between
  0 and 140°C, with PWM from 0 to 255. Config, D-Bus input and Curve
  deserialization use the same validator; the GUI is not the enforcement point.
- At 100°C or above, every manual preset requests full PWM regardless of the
  saved curve. Firmware Auto remains under firmware control. The 100°C threshold
  is a software policy, not a validated device-specific safe limit;
  supported hardware validation may require a lower threshold.
- Below the first curve point, zero target PWM returns control to firmware after
  the normal spin-down delay. It does not guarantee that physical fans stop.
- If any temperature sensor discovered at startup becomes unreadable, or there
  is no supported temperature reading, manual curves are suspended and firmware
  Auto (or the opted-in experimental maximum mode) is requested immediately,
  bypassing the spin-down delay. Machines with
  one supported temperature sensor may use it without requiring an absent GPU
  sensor. Zero, negative, malformed, and readings above 140°C are unavailable.
- Sensor reads are fresh on every poll; a frozen but plausible hardware reading
  cannot be detected just by comparing values. There is no independent hardware
  watchdog or guarantee that a blocked sysfs read will time out.
- On a failed manual-mode or PWM write, the daemon attempts firmware
  Auto by default, or the opted-in experimental maximum mode, and reports both
  the original and recovery errors when necessary. Recovery does not require
  a usable PWM data file, including when only the mode node exists at discovery.
- Each poll checks reported fan mode against the planned mode and rewrites a
  mismatch without waiting for the periodic timer. Failed writes invalidate the
  cached successful plan so the next poll retries. PWM readback is not compared
  against requested duty: hp-wmi may report scaled measured RPM. Reported mode
  can be cached kernel state and does not prove physical firmware behavior.

These safeguards do not establish safe cooling for every custom curve below
100°C. Use the default curves until model-specific limits have been established.
See [Hardware validation](hardware-validation.md) for current evidence and the
controlled physical test sequence.

## Experimental maximum-fan recovery

`thermal.experimental_emergency_max` defaults to `false`, including in old
configuration files where it is absent. Leave it disabled on installed systems
until the separate hardware validation has been completed. The implementation
and fake-sysfs tests do not resolve the failed firmware handoff observation.

For a deliberately controlled candidate test, setting it to `true` selects
hp-wmi's mode 0 maximum-fan request for:

- normal shutdown, ordinary engine drop and unexpected worker exit;
- missing or implausible temperature readings;
- failed manual-mode or PWM writes, including a missing PWM data node.

This is a separate mode write, not manual PWM 255. It requires only the mode
node. Read-only session daemons reject it even if the option is set. A failed
maximum request is reported and is not silently replaced by an Auto request.
Sensor faults retry on subsequent polls; a valid sensor reading lets the
selected curve resume. A failed normal write invalidates its cached plan so
the next poll retries. Errors describe requests as unverified cooling, even
when the write succeeds.

| Situation | Default | Experimental option enabled |
| --- | --- | --- |
| Recovery trigger listed above | Request Auto (mode 2) | Request maximum (mode 0) |
| Explicit Auto preset or zero curve target | Request Auto | Request Auto |
| Manual Max preset/critical override | Manual PWM 255 | Manual PWM 255 |
| Failure of the recovery write | Report error | Report error; do not fall back to Auto |

Maximum mode may remain active after the daemon exits, including through the
kernel driver's keep-alive. Noise, power use and transition/rollback behavior
therefore require physical validation. This is not an independent watchdog and
does not recover a hung kernel, blocked I/O or a process killed before it can
issue recovery. It does not establish safe handling of sensor failures on
untested models. No new D-Bus method or automatic configuration migration
enables this option; it requires an explicit administrator config change.
