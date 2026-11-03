# OMEN 16-xf0xxx shutdown investigation

Read-only investigation following the aborted physical observation. No fan
settings, services, kernel modules or installed files were changed during this
investigation. The earlier rollback remains in effect.

## Driver identity and evidence

- Product: OMEN by HP Gaming Laptop 16-xf0xxx; board 8BCA; BIOS F.25.
- CPU: AMD Ryzen 7 7840HS with Radeon 780M Graphics.
- Running kernel: 7.1.8-arch1-3; package: linux 7.1.8.arch1-3.
- Module file: `/usr/lib/modules/7.1.8-arch1-3/kernel/drivers/platform/x86/hp/hp-wmi.ko.zst`.
- Compressed module SHA-256: `9495f2e1973a720b4d0880594ea1f1c1a6930d55f701ed70c2e160b9fecf0dd5`.
- Loaded-module and on-disk module srcversion both:
  `6B0E7BF7560EF7F9BC1EFEB`.
- Versioned upstream source:
  [Linux v7.1.8 hp-wmi.c](https://github.com/gregkh/linux/blob/v7.1.8/drivers/platform/x86/hp/hp-wmi.c).
  The downloaded raw file SHA-256 was
  `7a682e8c195ad5e1909d14bc9722815afe5c804047d2896e0c5e2f71b77bad06`.

The local kernel headers do not contain this driver's C source. We compared the
downloaded versioned source against disassembly of the installed module's
`hp_wmi_apply_fan_settings`, `hp_wmi_fan_speed_set` and `hp_wmi_hwmon_read` paths.
The relevant operations match. This is a targeted binary/source comparison,
not a reproducible rebuild proving the complete source identity. Current
upstream master has different fan-control code and should not be substituted
for the running version during diagnosis.

## What Auto means in this driver

The versioned source and installed-module disassembly show:

1. Mode 2 requests the driver's Auto path. On the supported manual-control
   branch, it disables maximum-fan mode and sends the special zero-speed reset
   command for both fans. Zero is intended as an automatic-control sentinel.
2. After success, the driver cancels its periodic fan-setting keep-alive.
   Manual/maximum modes instead schedule periodic reapplication.
3. Mode readback returns stored driver state rather than reading a firmware
   fan-control acknowledgement. PWM readback derives from measured RPM.

Consequently, `pwm1_enable=2` is evidence of requested driver state, not proof
of subsequent cooling. The application's shutdown writes mode 2; it does not
directly issue a manual zero-PWM command. The driver's special reset must not
be confused with proof that firmware deliberately keeps fans off indefinitely.

## Recorded shutdown timeline

[Machine-readable CSV](2026-10-19-shutdown-timeline.csv). Times are relative to
the first post-stop sample; individual sysfs attributes were read sequentially,
not atomically. Mode readback remained 2 throughout.

| Elapsed seconds | CPU °C | Fan1 RPM | Fan2 RPM |
| --- | --- | --- | --- |
| 0.000 | 49.875 | 3300 | 3600 |
| 5.409 | 46.625 | 0 | 0 |
| 10.810 | 51.750 | 0 | 0 |
| 11.886 | 69.000 | 0 | 0 |
| 12.962 | 74.125 | 0 | 0 |
| 14.040 | 77.750 | 0 | 0 |
| 15.119 | 83.000 | 0 | 0 |

The temperature rise spans several samples, not just one anomalous value.
The journal confirms SIGTERM handling. The kernel journal had no matching
hp-wmi/thermal/throttling messages in the inspected 10:19–10:23 UTC interval;
absence of messages is not proof that firmware reset or cooling succeeded.

AMD lists a 100°C maximum operating temperature for this CPU in its
[7840HS specification](https://www.amd.com/en/products/processors/laptop/ryzen/7000-series/amd-ryzen-7-7840hs.html).
The 80°C guard was our deliberately lower test limit, not a vendor failure
threshold or a validated laptop fan-curve threshold. The 83°C observation does
not by itself establish overheating or permanent fan failure. Because the test
aborted there, it cannot distinguish delayed firmware hysteresis from a reset
quirk or a nonfunctional firmware handoff. No synchronized CPU power/load trace
or independent physical fan measurement was captured.

## Consequences for application safety

The root cause is **unresolved**. Neither a kernel patch nor an arbitrary new
temperature threshold is justified by this evidence alone. Do not mark Auto
handoff safe or expand the test limits to obtain a passing result.

The source currently uses the same Auto request for several distinct cases:

| Trigger | Current software behavior | Missing evidence |
| --- | --- | --- |
| Normal shutdown | Request Auto and exit | Firmware cooling after exit |
| Missing/implausible CPU or GPU sensor | Immediately request Auto | Cooling when temperature feedback is unavailable |
| Failed manual write | Attempt Auto recovery | Reset effectiveness and cooling after partial writes |
| Zero curve target | Return to Auto after hysteresis | Behavior after a custom curve releases control |
| User selects Auto | Leave cooling to firmware | Model-specific firmware response across temperatures |

Successful sysfs writes should be described as requests, not verified physical
recovery. This distinction also applies to fake-sysfs tests: they establish
the commands and retries, not the hardware effect.

## Next work, in order

1. Define explicit recovery policy and evidence requirements separately from
   user-selected Auto. Keep the current candidate experimental until a fallback
   has physical validation; an observation guard is not a production watchdog.
2. Implement and simulate a distinct driver maximum-fan request and transition
   error handling if that becomes the chosen emergency fallback. Maximum mode
   is available in the inspected driver, but its physical behavior on this
   laptop has not been tested. Do not silently deploy it as a proven solution.
3. Validate that recovery path at low workload with an operator, preserved
   settings and an independent way to recover control before revisiting Auto
   handoff. Do not run crash or load tests yet.
4. Investigate firmware handoff with the exact kernel/BIOS and vendor thermal
   behavior, preferably on a dedicated test machine. Capture CPU power/load,
   both fans and temperature timing; prepare a sanitized upstream driver report
   if a reproducible firmware reset problem is established. Nothing has been
   sent to maintainers or other third parties.

No production cooling-policy change or installation was made based on this
investigation. The existing 52-test result remains software evidence only.

## Subsequent source implementation

The separate maximum-fan request and simulated transition/error handling are
now implemented behind `thermal.experimental_emergency_max`, disabled by
default. See [Daemon safety](../daemon-safety.md#experimental-maximum-fan-recovery).
The later candidate passes 62 release tests; it has not been installed or
physically exercised. The investigation and raw evidence above still describe
the earlier candidate. Hardware validation and the root-cause investigation
remain open.
