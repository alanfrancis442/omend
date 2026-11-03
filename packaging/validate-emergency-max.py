#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Verify an extracted bundle and print a pending, file-only hardware test plan.

This is deliberately NOT a live hardware runner. Independent rollback has not
been implemented and reviewed. Neither root nor --allow-hardware-writes enables
installation, service operations, account changes or sysfs writes.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import tempfile
import tomllib

REQUIRED = {"bin/omend", "packaging/dev.omend.conf", "packaging/omend.service"}
MANIFEST = "CONTENTS.sha256"


def digest(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise ValueError("bundle entries must be regular files")
        return hashlib.file_digest(source, "sha256").hexdigest()


def verify_bundle(bundle):
    """Check all file hashes and reject links, special files and extra entries.

    Integrity against a colocated manifest does not establish authenticity. A
    future live runner must snapshot into a root-private directory and verify
    that snapshot against an independently trusted manifest hash before use.
    """
    if bundle.is_symlink() or not bundle.is_dir():
        raise ValueError("bundle must be an extracted directory, not a symlink")
    actual = set()
    for directory, directories, filenames in os.walk(bundle, followlinks=False):
        for name in directories + filenames:
            path = Path(directory) / name
            mode = path.lstat().st_mode
            if not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                raise ValueError("bundle links and special files are forbidden")
            if stat.S_ISREG(mode):
                actual.add(path.relative_to(bundle).as_posix())
    manifest = bundle / MANIFEST
    if MANIFEST not in actual or manifest.stat().st_size > 8 * 1024 * 1024:
        raise ValueError("missing or oversized CONTENTS.sha256")
    records = {}
    for line in manifest.read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (?:\./)?(.+)", line)
        if not match:
            raise ValueError("unsupported checksum manifest record")
        expected, name = match.groups()
        parts = PurePosixPath(name)
        if (parts.is_absolute() or ".." in parts.parts or "\\" in name
                or parts.as_posix() != name or name == MANIFEST or name in records):
            raise ValueError("unsafe or duplicate checksum manifest entry")
        records[name] = expected
    if not REQUIRED.issubset(records) or set(records) != actual - {MANIFEST}:
        raise ValueError("manifest must cover every bundle file and required candidate files")
    for name, expected in records.items():
        if digest(bundle / name) != expected:
            raise ValueError(f"checksum mismatch: {name}")
    return {"manifest_sha256": digest(manifest), "daemon_sha256": records["bin/omend"],
            "files_verified": len(records)}


def temporary_config(original):
    """Prepare text in memory; never save over the real installed config."""
    before = tomllib.loads(original)
    if not isinstance(before.get("thermal"), dict) or "rgb" not in before or "profile" not in before:
        raise ValueError("an existing thermal, RGB and profile configuration is required")
    section = re.search(r"(?m)^\[thermal\][ \t]*(?:#.*)?\r?$", original)
    if section is None:
        raise ValueError("expected a plain [thermal] section")
    start = section.end()
    next_section = re.search(r"(?m)^\s*\[", original[start:])
    end = start + next_section.start() if next_section else len(original)
    body = original[start:end]
    changes = {"preset": '"balanced"', "experimental_emergency_max": "true",
               "plugged_in": '""', "on_battery": '""'}
    for key, value in changes.items():
        pattern = rf"(?m)^[ \t]*{re.escape(key)}[ \t]*=.*$"
        replacement = f"{key} = {value}"
        if re.search(pattern, body):
            body = re.sub(pattern, lambda _match: replacement, body)
        else:
            body = "\n" + replacement + body
    candidate = original[:start] + body + original[end:]
    after = tomllib.loads(candidate)
    if after["rgb"] != before["rgb"] or after["profile"] != before["profile"]:
        raise ValueError("temporary configuration changed RGB or platform profile")
    for key, expected in {"preset": "balanced", "experimental_emergency_max": True,
                          "plugged_in": "", "on_battery": ""}.items():
        if after["thermal"][key] != expected:
            raise ValueError("temporary recovery setting was not applied")
    return candidate


def plan(identity, config_sha=None):
    return {
        "status": "pending_physical_validation", "execution_enabled": False,
        "candidate": identity, "temporary_config_sha256": config_sha,
        "scope": {"product": "OMEN by HP Gaming Laptop 16-xf0xxx", "board": "8BCA",
                  "model_mismatch": "abort; this procedure does not apply to other models"},
        "guards": {"baseline_below_c": 70, "baseline_consecutive_seconds": 30,
                   "baseline_max_wait_seconds": 180, "abort_at_c": 80,
                   "missing_or_implausible_cpu_gpu_sensor": "abort and roll back",
                   "maximum_host_test_seconds": 90},
        "sequence": [
            "Require an operator present, no heavy workloads, no competing GUI/test/build, and a root recovery session.",
            "Read DMI model/board; identify hp, k10temp and amdgpu by hwmon names, not numeric indices; require both fan RPM nodes.",
            "Require the original daemon active with expected executable/config, no service drop-ins, and readable valid original configuration.",
            "Wait for 30 consecutive seconds with CPU/GPU below 70 C, for at most 180 seconds; fail before installation if not achieved.",
            "Create a root-private /var/backups/omend/<UTC>-<PID> backup of daemon/policy/unit/config, preserving owner/mode/xattrs.",
            "Snapshot candidate into the root-private directory and verify CONTENTS plus its independently trusted hash again; do not install from the user-writable checkout or bundle.",
            "Prepare the standalone helper under a root-owned 0500 path after comparing its hash with the independently reviewed hash; the copied helper must not depend on the source checkout.",
            "Arm and verify an independent repeating systemd rollback timer BEFORE changing installed files or hardware.",
            "Do not change account or group state in this fan-safety test; test authorization separately.",
            "Stop the old daemon; install candidate daemon, matching policy/unit and temporary configuration preserving original RGB/profile with Balanced and empty AC/battery choices.",
            "Reload systemd/D-Bus configuration, start candidate and observe both fans spinning, sampling CPU/GPU/RPM every second with the 80 C guard.",
            "Send normal SIGTERM and require candidate exit; mode 0 readback is only evidence of a request, not proof of cooling.",
            "Require both measured fan speeds at least 4000 RPM within 15 seconds after SIGTERM, then continuously observe that response for another 10 seconds with temperatures below guard.",
            "On every success or failure, request known manual mode 1 then PWM 255, restore exact original files/settings, reload configuration and start the previous daemon.",
            "Verify restored file hashes/metadata, live old executable identity, active service and cooling; only then cancel the independent rollback timer.",
            "Keep raw traces/journal private in the root backup; export only a sanitized JSON result without serial numbers, user names, host paths or unrelated journal content.",
        ],
        "rollback_timer_command_template": [
            "systemd-run", "--unit=<unique-omend-recovery-name>", "--on-active=120s",
            "--on-unit-inactive=15s", "--timer-property=AccuracySec=1s",
            "--property=Type=oneshot", "--property=StartLimitIntervalSec=0",
            "--property=TimeoutStartSec=45s", "/usr/bin/python3",
            "<root-owned-private-rollback-helper>", "--rollback",
        ],
        "rollback_requirements": [
            "The helper must stop only omend.service through systemd and verify the unit is inactive before restoring files, so no daemon writes race with restoration.",
            "Restoration must be idempotent and serialized; the repeating timer verifies restored files/service until the operator confirms cooling and cancels it.",
            "The fan-safety procedure must not modify account or group state; authorization checks are separate.",
            "The independent timer and helper are best-effort software recovery, not protection against kernel hangs, suspend, firmware faults or loss of power.",
        ],
        "excluded": ["heat/load generation", "crash/SIGKILL testing", "sensor unloading",
                     "suspend/resume", "weak fan curves", "Auto recovery testing"],
        "execution_blockers": ["independent rollback-helper review and isolated systemd rollback-path runtime validation",
                               "operator presence and fresh explicit approval", "physical validation still pending"],
    }


def self_test():
    """Only temporary fixture files; no D-Bus, services, accounts or sysfs."""
    with tempfile.TemporaryDirectory(prefix="omend-validation-plan-") as temporary:
        bundle = Path(temporary)
        for name in sorted(REQUIRED):
            target = bundle / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(f"fixture {name}\n")
        manifest = bundle / MANIFEST
        good = "".join(f"{digest(bundle / name)}  ./{name}\n" for name in sorted(REQUIRED))
        manifest.write_text(good)
        identity = verify_bundle(bundle)
        assert identity["files_verified"] == 3
        assert not plan(identity)["execution_enabled"]
        for bad in (good.replace("./bin/omend", "../bin/omend"), good + good.splitlines()[0] + "\n"):
            manifest.write_text(bad)
            try:
                verify_bundle(bundle)
            except ValueError:
                pass
            else:
                raise AssertionError("unsafe manifest accepted")
        manifest.write_text(good)
        (bundle / "unexpected").write_text("not in manifest")
        try:
            verify_bundle(bundle)
        except ValueError:
            pass
        else:
            raise AssertionError("unlisted file accepted")
        (bundle / "unexpected").unlink()
        (bundle / "bin/omend").write_text("tampered")
        try:
            verify_bundle(bundle)
        except ValueError:
            pass
        else:
            raise AssertionError("tampered executable accepted")
        (bundle / "bin/omend").unlink()
        (bundle / "bin/omend").symlink_to("/dev/null")
        try:
            verify_bundle(bundle)
        except ValueError:
            pass
        else:
            raise AssertionError("symlink accepted")
    original = ('profile = "performance"\n[thermal]\npreset = "auto"\n'
                'plugged_in = "quiet"\non_battery = "custom"\n'
                '[rgb]\neffect = "audio"\nbrightness = 123\n')
    candidate = temporary_config(original)
    assert tomllib.loads(candidate)["thermal"]["experimental_emergency_max"]
    assert tomllib.loads(candidate)["rgb"] == tomllib.loads(original)["rgb"]
    assert tomllib.loads(candidate)["profile"] == "performance"
    print("PASS: fixture integrity/rejection and config-preservation checks; no physical test performed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", nargs="?", type=Path, help="extracted bundle with CONTENTS.sha256")
    parser.add_argument("--config", type=Path, help="read-only original config inspection; outputs only generated config hash")
    parser.add_argument("--self-test", action="store_true", help="run temporary file-only fixtures")
    parser.add_argument("--allow-hardware-writes", action="store_true", help="reserved; live execution remains disabled")
    args = parser.parse_args()
    if args.allow_hardware_writes:
        parser.error("this planner is read-only; use no live hardware runner until the recovery protocol is independently reviewed and approved")
    if args.self_test:
        self_test()
        return
    if args.bundle is None:
        parser.error("an extracted bundle or --self-test is required")
    try:
        identity = verify_bundle(args.bundle)
        config_sha = None
        if args.config is not None:
            candidate = temporary_config(args.config.read_text())
            config_sha = hashlib.sha256(candidate.encode()).hexdigest()
        print(json.dumps(plan(identity, config_sha), indent=2))
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
