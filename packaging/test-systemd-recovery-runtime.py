#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Exercise the recovery timer in a disposable, networkless systemd container.

The image must already exist locally. The container has no network and no host
bind mounts; cleanup runs even if a check fails. This does not run the rollback
helper against host files or hardware.
"""
import argparse
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("hardware_recovery", ROOT / "hardware-recovery.py")
recovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recovery)


def docker(args, *, check=True, timeout=30):
    result = subprocess.run(["docker", *args], check=False, text=True,
                            capture_output=True, timeout=timeout)
    if check and result.returncode:
        details = result.stderr.strip() or result.stdout.strip()
        raise RuntimeError(f"docker {' '.join(args)} failed: {details}")
    return result


def wait_for_systemd(name, timeout_seconds):
    deadline = time.monotonic() + timeout_seconds
    last_error = "systemd manager did not answer"
    while time.monotonic() < deadline:
        result = docker(["exec", name, "/usr/bin/systemctl", "show-environment"],
                        check=False, timeout=5)
        if result.returncode == 0:
            return
        last_error = result.stderr.strip() or result.stdout.strip() or last_error
        time.sleep(0.5)
    raise TimeoutError(last_error)


def read_count(name, counter):
    result = docker(["exec", name, "/usr/bin/cat", str(counter)], check=False)
    if result.returncode:
        return 0
    return int(result.stdout.strip())


def diagnostics(name, unit):
    output = []
    for args in (
        ["/usr/bin/systemctl", "list-timers", "--all", "--no-pager"],
        ["/usr/bin/systemctl", "status", f"{unit}.timer", f"{unit}.service", "--no-pager"],
        ["/usr/bin/journalctl", "-u", f"{unit}.service", "--no-pager", "-n", "30"],
    ):
        result = docker(["exec", name, *args], check=False)
        output.append(result.stdout + result.stderr)
    return "\n".join(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", default="kindest/node:v1.36.1",
                        help="cached image that boots systemd as PID 1")
    parser.add_argument("--boot-timeout", type=float, default=60)
    parser.add_argument("--run-seconds", type=float, default=6)
    args = parser.parse_args()

    docker(["image", "inspect", args.image], timeout=10)
    name = "omen-systemd-recovery-" + uuid.uuid4().hex[:12]
    unit = "omen-recovery-runtime-" + uuid.uuid4().hex[:8]
    runtime_dir = Path("/var/backups/omend") / unit
    counter = runtime_dir / "count"
    container_started = False
    try:
        # The cached kind node image boots systemd. No host paths/devices are
        # mounted and the container cannot reach the network.
        docker(["run", "--detach", "--rm", "--privileged", "--network", "none",
               "--name", name, args.image], timeout=30)
        container_started = True
        wait_for_systemd(name, args.boot_timeout)
        docker(["exec", name, "/usr/bin/mkdir", "-p", str(runtime_dir)])

        with tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".sh", delete=False) as script:
            script.write(
                "#!/bin/sh\n"
                f"counter={counter}\n"
                "if [ -f \"$counter\" ]; then read count < \"$counter\"; else count=0; fi\n"
                "count=$((count + 1))\n"
                "printf '%s\\n' \"$count\" > \"$counter\"\n"
                "if [ \"$count\" -eq 1 ]; then exit 1; fi\n"
            )
            helper = Path(script.name)
        try:
            docker(["cp", str(helper), f"{name}:{runtime_dir}/helper.sh"])
        finally:
            helper.unlink(missing_ok=True)

        command = recovery.timer_command(
            unit, runtime_dir / "helper.sh", first_delay="2s", repeat_interval="1s",
            interpreter="/bin/sh",
        )
        docker(["exec", name, *command], timeout=20)
        active = docker(["exec", name, "/usr/bin/systemctl", "is-active", f"{unit}.timer"],
                        check=False)
        if active.returncode or active.stdout.strip() != "active":
            raise RuntimeError("generated recovery timer did not become active")

        time.sleep(args.run_seconds)
        count = read_count(name, counter)
        if count < 2:
            raise RuntimeError(f"repeating timer ran only {count} times\n{diagnostics(name, unit)}")

        docker(["exec", name, "/usr/bin/systemctl", "stop", f"{unit}.timer"])
        docker(["exec", name, "/usr/bin/systemctl", "stop", f"{unit}.service"], check=False)
        stopped_count = read_count(name, counter)
        time.sleep(2)
        if read_count(name, counter) != stopped_count:
            raise RuntimeError("timer kept firing after cancellation")
        print(f"systemd runtime timer passed: failed first run retried ({count} invocations); cancellation stopped repeats")
    finally:
        if container_started:
            docker(["rm", "--force", name], check=False, timeout=20)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"isolated systemd runtime check failed: {error}", file=sys.stderr)
        raise SystemExit(1)
