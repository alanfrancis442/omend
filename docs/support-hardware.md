# Hardware support and reporting

**No laptop model has completed the project's hardware safety sign-off.** Detection
of a sysfs interface, a successful write, mode readback or passing unit tests does
not establish physical cooling. Userspace and the optional module are experimental.

## Recorded evidence

| Model / environment | Evidence | Status |
| --- | --- | --- |
| OMEN by HP Gaming Laptop 16-xf0xxx; BIOS F.25; kernel 7.1.8-arch1-3; Ryzen 7 7840HS | Temperature, two fan, platform-profile and zone interfaces detected. Narrow deployed authorization checks and short Balanced observation recorded. Firmware-Auto shutdown observation aborted at 83°C with both fans reporting zero RPM. | Unresolved recovery; not signed off |
| Other OMEN/Victus models, kernels and BIOS versions | No controlled results recorded here | Unvalidated |

The maximum-fan emergency recovery option is disabled by default and lacks
physical validation. The installed system used during the recorded attempt was
rolled back. See [full validation](hardware-validation.md) and the
[driver investigation](hardware-results/2026-10-19-driver-investigation.md).

Compatibility varies by model, firmware, kernel driver and permissions. Some
devices expose only temperatures or profiles. Zone lighting requires suitable LED
nodes; per-key keyboards are not supported by the optional zone module. There
is no established distribution/glibc compatibility matrix for local binary bundles.

## Collect a read-only report

After building, these commands inspect hardware or service state:

```sh
./target/release/omen-cli doctor
uname -r
cat /etc/os-release
systemctl status omend --no-pager
```

If a daemon is already installed, `omen-cli status --json` can add its reported
state. An absent service, missing permissions or `doctor` exit status 2 is useful
evidence; do not install a root daemon merely to make a report succeed. Obtain
product family and BIOS version from your firmware settings or a locally reviewed
hardware report; do not share a serial number.

Report the source revision/artifact checksum, power source, kernel, BIOS, model,
whether the optional module is loaded, and the exact affected feature. Include
only relevant redacted logs. For temperature/fan observations include timestamps
and both fan readings, and distinguish requested mode from measured behavior.
Do not rerun an aborted safety test or weaken its guard to produce a passing result.
Use [contribution guidance](../CONTRIBUTING.md) for reporting channels.

## Evidence needed for sign-off

Follow the staged [hardware validation sequence](hardware-validation.md): controlled
baseline and rollback, authorization, normal stop/start and recovery, presets and
power changes, suspend/resume, then independently recoverable faults and bounded
load. Results apply only to the tested environment. A contributor's successful
single operation is useful evidence, not certification for an entire product family.
