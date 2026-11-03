# Hardware safety validation

Status: **incomplete; shutdown recovery observation failed its temperature guard**.
Hardware support remains experimental. Unit tests use fake sysfs and do not
establish physical fan response or firmware crash recovery.

## Read-only observation — 2026-10-19

| Item | Observation |
| --- | --- |
| Product | OMEN by HP Gaming Laptop 16-xf0xxx |
| BIOS | F.25 |
| Kernel | 7.1.8-arch1-3 |
| CPU sensor | k10temp, temp1, Tctl |
| GPU sensor | amdgpu, temp1, edge; no hotspot exposed in this observation |
| Fan interface | hp hwmon; pwm1, pwm1_enable, fan1_input, fan2_input |
| Platform profiles | low-power, balanced, performance |
| Keyboard | WASD, left, center and right zone nodes present |
| Installed daemon | /usr/bin/omend, system service active; candidate not installed |

A ten-sample observation at two-second intervals overlapped release compilation.
CPU readings ranged from 49.125 to 100°C, GPU from 41 to 66°C; reported fan speeds
ranged from 100 to 6000 RPM for fan1 and 100 to 6300 RPM for fan2. Manual mode
was reported throughout. This was **not an idle baseline or a controlled thermal
test**. Do not infer safe thresholds, fan response time, or a passing load test
from these samples. No load generator or hardware writes were used by the
observer; the existing installed daemon remained responsible for control.

## Follow-up owner observations — 2026-11-01

The owner supplied two dashboard screenshots during normal use; screenshot times
and sampling intervals are not recorded. One showed CPU 47°C, GPU 45°C, CPU load
12%, GPU load 15%, and both fan readings at 0 RPM with 0% duty. The other showed
CPU 76°C, GPU 66°C, CPU load 98%, GPU load 9%, Fan 1 at 6000 RPM, Fan 2 at
6300 RPM, and 100% duty. The owner reports that temperatures are usually below
the fan curve threshold at idle and that fan duty sometimes reaches 33%.

These are useful observations of the current custom-curve behavior at two
different load levels. They are not a controlled time series, do not establish
vendor temperature limits, and do not validate behavior after the daemon stops.
The previously observed shutdown recovery failure remains unresolved; do not
repeat that stop test on the everyday laptop yet.

The upstream [hp-wmi driver source](https://github.com/torvalds/linux/blob/master/drivers/platform/x86/hp/hp-wmi.c)
shows that PWM readback can be measured RPM converted to PWM, and mode readback
can be cached driver state. This source is context, not verification of the
exact running kernel. Neither value alone proves the requested cooling was
physically applied. In particular, do not compare requested duty against observed
PWM as an exact fault detector, or assume cached mode proves firmware state.

## Candidate software evidence

Six additional fake-sysfs regressions cover a mode reset before the rewrite
timer expires, variable manual PWM readback, Auto's variable PWM, failed recovery
and retry, failure to enter manual mode, and retry after a failed rewrite.
The mode-reset regression failed before the engine correction and passes after.
The complete locked release suite passes 52 tests; local evidence is
`/tmp/omen-hardware-safety-tests.log`. Keep hardware validation below pending.

The subsequent source candidate adds an opt-in experimental mode-0 maximum-fan
recovery path. It is disabled by default and has not been installed or physically
tested. See [Daemon safety](daemon-safety.md#experimental-maximum-fan-recovery)
for the option, covered transitions and limitations. The recorded archive and
daemon hashes below identify the earlier physical-test candidate, not this later
source revision. The later candidate passes 62 locked release tests, including
nine added emergency-path tests and one added config compatibility test. Local
evidence: `/tmp/omen-emergency-max-tests.log`.

## Controlled installation attempt — 2026-10-19

The operator authorized backup, candidate installation and a low-workload
stop/start test. No stress workload, suspend, crash, SIGKILL, sensor unloading or
hang injection was performed. A VM and desktop applications remained active;
this was not an otherwise idle laboratory environment.

The first preflight aborted before installing anything at a CPU reading of 73°C.
The next attempt waited for 30 consecutive seconds of sampled CPU/GPU readings
below 70°C. A reading at or above 80°C during the subsequent test triggered
rollback. These are operational test guards, not vendor-rated thermal limits.

Candidate identities:

- Archive SHA-256: `f5142affddc6ae7912f22bec54975891c938d3732966d5090d037bae8363aa0f`.
- Daemon SHA-256: `6147b1cf6112074a92cff0f0315c5ee55e4262876a66bf0a20f076809b462574`.
- Root-owned backup and raw trace: `/var/backups/omend/20261019T102050Z-117496`.
- Sanitized results: [JSON report](hardware-results/2026-10-19-omen-16-xf0xxx.json).
- Follow-up: [Driver and timeline investigation](hardware-results/2026-10-19-driver-investigation.md).

| Stage | Observation |
| --- | --- |
| Deployed access policy | On this host's system bus, an untrusted real user could read status and received AccessDenied for an Auto mutation. A fresh process with the new `omen` group could select Auto; root could select Balanced. |
| Candidate Auto, 10 samples | CPU 49.75–58.5°C; mode 2; fans mostly stopped. This does not establish cooling capacity. |
| Candidate Balanced, 15 samples | CPU 50.625–65.5°C; mode 1; fan1 300–3300 RPM, fan2 200–3600 RPM. |
| Stop from Balanced | Journal recorded SIGTERM recovery and reported mode became 2 without an observer write. The stopped observation aborted after about 15 seconds at CPU 83°C, GPU 52°C, both fans 0 RPM. |
| Rollback | Previous daemon, policy, config and service file all match backup byte-for-byte. Running daemon hash matches the old executable. Temporary `omen` membership/group was removed. |

**The shutdown recovery check did not pass.** The candidate requested Auto, but
the observation does not demonstrate adequate firmware cooling. A short CPU
temperature spike and cached mode readback do not identify the root cause or
prove permanent fan failure. Do not repeat the experiment with a higher guard
or under load. Investigate the exact kernel's hp-wmi/firmware behavior, thermal
response and an independent recovery mechanism before another controlled test.

After rollback, five further samples showed CPU 50.125–58.5°C, GPU 45–50°C and
both fans around 2000–2300 RPM. The saved custom configuration was preserved.
The **older installed daemon and older permissive policy remain active**; the
candidate safeguards and new restrictions are not deployed after this rollback.

## Independent rollback preparation

`packaging/hardware-recovery.py` now has a file-only `prepare` action and a
standalone copied rollback helper. The fixture checks in
`packaging/test-hardware-recovery.py` passed without contacting systemd, D-Bus,
or live sysfs. The timer command and equivalent temporary service/timer units
pass `systemd-analyze verify`. A disposable, networkless Docker systemd
container verified that the timer retries after a fixture helper fails once,
then succeeds, and that cancellation stops further runs. This remains a timer
fixture: the recovery helper itself has not been executed through systemd, and
no unit was installed or started on the host. The helper has **not** been
prepared or run on this host.

Preparation requires the exact reviewed helper hash and refuses a mismatched
model/board, inactive or customized service, service drop-ins, or invalid saved
configuration. It snapshots the active daemon, D-Bus policy, systemd unit, and
configuration with hashes, ownership, modes and extended-attribute checks into a
root-private directory. Its printed timer starts after 120 seconds and repeats
read-only state checks every 15 seconds after rollback. On recovery it stops the
managed service, requests manual mode 1 and PWM 255, restores the saved files,
reloads systemd and D-Bus, restarts the previous daemon, and verifies the
restored executable and file metadata. A sysfs write or mode readback is not
proof of fan response; the timer remains armed until an operator confirms both
fans and a safe temperature trend. Account and group state are outside this fan
test and must not be changed by its procedure.

The helper is a recovery aid, not protection against kernel hangs, power loss,
firmware faults, or loss of the systemd manager. Its exact source hash and
failure paths still need review before any installation or hardware exercise.

## Controlled physical validation sequence

Run one stage at a time with an operator present. Record the exact candidate
checksum, source revision, kernel, BIOS, CPU/GPU identity, power source, ambient
conditions, timestamps, temperature and RPM traces, service logs and result.
Never publish serial numbers or unrelated journal content with reports.

1. **Prepare recovery and a cool baseline.** Close builds and other heavy work.
   Confirm the model's vendor temperature limits and choose abort limits below
   them before any load or fault exercise. Identify the hp hwmon by its `name`,
   not a fixed hwmon index. Verify an available root recovery session and the
   driver-supported way to restore firmware control. Preserve the existing
   binaries, configuration, bus policy and service definitions for rollback.
2. **Install the candidate deliberately.** Update binary and policy together,
   configure trusted group membership, and verify normal-user read access and
   control authorization. Restarting changes hardware control; perform it only
   when ready for the following observations. Do not run a second root daemon.
3. **Verify Auto and ordinary stop/start first.** Observe both physical fans
   and temperature trends at low workload. Confirm that service stop restores
   firmware control, that cooling remains functional after stop, and that
   restarting restores the saved selection without overwriting it. A successful
   sysfs write or mode readback alone is insufficient evidence.
4. **Verify supported presets and power transitions.** Test built-in curves
   first, then AC/battery changes. Record fan response and actual temperatures.
   Do not use deliberately weak custom curves or heat the machine to the
   software critical threshold as a test. Keep critical-override testing simulated.
5. **Verify suspend/resume.** Begin at low workload, record pre/post discovery,
   control mode, both fan speeds, temperatures and logs. Confirm the sleep hook
   and mode-reset recovery work with the supported desktop/systemd setup.
6. **Assess crash and hang recovery separately.** Before SIGKILL or hang
   experiments, establish a recovery mechanism independent of the daemon and
   verify it works. Ordinary shutdown cleanup cannot run in these cases. Do not
   assume a firmware timeout restores Auto: kernel keep-alive behavior may retain
   manual settings. Do not induce blocked sysfs I/O or unload sensor drivers on
   this everyday-use host. Use a dedicated test system for destructive faults.
7. **Run bounded load tests last.** Only after the prior stages pass, use an
   agreed workload and duration, independent temperature observation and the
   model-specific abort limits from step 1. Stop immediately on missing readings,
   failed cooling, unexpected mode changes or approach to an abort limit. Record
   steady-state results and recovery after load; one success does not validate
   other BIOS versions, kernels or models.

| Physical check | Result |
| --- | --- |
| Cool idle baseline with exact candidate | Cool pre-install interval recorded; otherwise-idle candidate baseline pending |
| New deployed authorization with separate users | Passed narrow checks on this host; policy subsequently rolled back; broader broker/session checks pending |
| Auto and stop/start firmware recovery | SIGTERM/Auto request observed; physical cooling check aborted at 83°C with both fans reading zero; unresolved |
| Presets and AC/battery transitions | Pending |
| Suspend/resume | Pending |
| Crash/hang recovery with independent recovery | Pending |
| Model-specific limits and bounded load | Pending |

Keep the release checklist open until results and traces are reviewed.
