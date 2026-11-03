#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Validate recovery timer syntax without installing or starting a unit."""
import importlib.util
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("hardware_recovery", ROOT / "hardware-recovery.py")
recovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recovery)

systemd_run = shutil.which("systemd-run")
systemd_analyze = shutil.which("systemd-analyze")
if not systemd_run or not systemd_analyze:
    raise RuntimeError("systemd-run and systemd-analyze are required for timer syntax validation")

unit = "omen-recovery-fixture"
helper = Path("/var/backups/omend/fixture/rollback-omend.py")
command = recovery.timer_command(unit, helper)
assert command[0] == systemd_run
assert "--on-active=120s" in command
assert "--on-unit-inactive=15s" in command
assert "--timer-property=AccuracySec=1s" in command
assert "--property=StartLimitIntervalSec=0" in command
assert command[-3] == "/usr/bin/python3"
assert command[-2:] == [str(helper), "--rollback"]
quick_command = recovery.timer_command(unit, helper, first_delay="2s", repeat_interval="1s")
assert "--on-active=2s" in quick_command
assert "--on-unit-inactive=1s" in quick_command
assert recovery.timer_command(unit, helper, interpreter="/bin/sh")[-3] == "/bin/sh"

help_text = subprocess.run([systemd_run, "--help"], check=True, text=True,
                           capture_output=True).stdout
for option in ("--on-active", "--on-unit-inactive", "--timer-property"):
    assert option in help_text, f"systemd-run does not document {option}"

with tempfile.TemporaryDirectory(prefix="omend-systemd-verify-") as temporary:
    directory = Path(temporary)
    service = directory / f"{unit}.service"
    timer = directory / f"{unit}.timer"
    service.write_text(
        "[Unit]\nDescription=omend rollback fixture\nStartLimitIntervalSec=0\n"
        "[Service]\nType=oneshot\nTimeoutStartSec=45s\nExecStart=/usr/bin/true\n"
    )
    timer.write_text(
        f"[Unit]\nDescription=omend repeating rollback fixture\n"
        f"[Timer]\nUnit={unit}.service\nOnActiveSec=120s\n"
        "OnUnitInactiveSec=15s\nAccuracySec=1s\n"
    )
    result = subprocess.run([systemd_analyze, "verify", str(service), str(timer)],
                            check=False, text=True, capture_output=True)
    assert result.returncode == 0, result.stderr

print("systemd recovery service/timer syntax passed; no unit was installed or started.")
