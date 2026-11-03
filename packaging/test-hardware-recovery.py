#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Fixture-only checks for the hardware recovery helper; never touch the host."""
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("hardware_recovery", ROOT / "hardware-recovery.py")
recovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recovery)


def make_tree(root, files):
    for relative, content in files.items():
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)


def make_snapshot(root, backup):
    backup.mkdir(parents=True, mode=0o700)
    files = {}
    for relative in recovery.MANAGED:
        source = root / relative
        target = backup / "files" / relative
        recovery.copy_preserving_metadata(source, target)
        files[relative] = recovery.metadata(target)
    return {"format": 1, "target": {"product": recovery.PRODUCT, "board": recovery.BOARD},
            "service": {"active": True,
                        "exec_start": "/usr/bin/omend --system --config /etc/omend/config.toml",
                        "drop_ins": []},
            "files": files}


with tempfile.TemporaryDirectory() as temporary:
    temp = Path(temporary)
    prepare_root = temp / "prepare-root"
    (prepare_root / "sys/class/dmi/id").mkdir(parents=True)
    (prepare_root / "sys/class/dmi/id/product_name").write_text(recovery.PRODUCT)
    (prepare_root / "sys/class/dmi/id/board_name").write_text(recovery.BOARD)
    prepare_files = {relative: f"original:{relative}\n" for relative in recovery.MANAGED}
    prepare_files["etc/omend/config.toml"] = 'profile = "balanced"\n[thermal]\npreset = "custom"\n'
    make_tree(prepare_root, prepare_files)
    prepare_proc = temp / "prepare-proc/123"
    prepare_proc.mkdir(parents=True)
    (prepare_proc / "exe").symlink_to(prepare_root / "usr/bin/omend")
    backup_parent = temp / "var/backups/omend"
    backup_parent.mkdir(parents=True, mode=0o700)
    bad_backup_parent = temp / "bad-backups/omend"
    bad_backup_parent.mkdir(parents=True, mode=0o700)
    service_values = {
        "ActiveState": "active",
        "DropInPaths": "",
        "ExecStart": "{ path=/usr/bin/omend ; argv[]=/usr/bin/omend --system --config /etc/omend/config.toml ; }",
        "MainPID": "123",
    }
    with patch.object(recovery, "ensure_root"), patch.object(recovery.os, "chown"):
        try:
            recovery.prepare(
                root=prepare_root,
                backup_parent=bad_backup_parent,
                expected_helper_sha256="0" * 64,
                service_reader=lambda field: service_values[field],
                proc_root=temp / "prepare-proc",
                owner_uid=os.geteuid(),
            )
            raise AssertionError("unreviewed helper hash was accepted")
        except ValueError as error:
            assert "independently reviewed" in str(error)
        assert list(bad_backup_parent.iterdir()) == []
        prepared = recovery.prepare(
            root=prepare_root,
            backup_parent=backup_parent,
            expected_helper_sha256=recovery.digest(ROOT / "hardware-recovery.py"),
            service_reader=lambda field: service_values[field],
            proc_root=temp / "prepare-proc",
            owner_uid=os.geteuid(),
        )
    prepared_backup = Path(prepared["backup"])
    prepared_data = json.loads((prepared_backup / recovery.MANIFEST_NAME).read_text())
    assert prepared_data["helper_sha256"] == recovery.digest(prepared_backup / recovery.HELPER_NAME)
    assert stat.S_IMODE((prepared_backup / recovery.HELPER_NAME).stat().st_mode) == 0o500
    with patch.object(recovery, "BACKUP_PARENT", backup_parent):
        verified_backup, _ = recovery.verify_snapshot(prepared_backup, owner_uid=os.geteuid())
        assert verified_backup == prepared_backup
        copied_helper = prepared_backup / recovery.HELPER_NAME
        copied_helper.chmod(0o600)
        copied_helper.write_text("tampered")
        try:
            recovery.verify_snapshot(prepared_backup, owner_uid=os.geteuid())
            raise AssertionError("tampered standalone helper was accepted")
        except ValueError as error:
            assert "checksum mismatch" in str(error)
    assert prepared["timer_unit"].endswith(".timer")
    prepared_unit = prepared["arm_timer_command"][1].split("=", 1)[1]
    assert prepared["arm_timer_command"] == recovery.timer_command(prepared_unit, Path(prepared["helper"]))
    assert prepared["verify_timer_command"][1] == "is-active"
    assert prepared["cancel_timer_command"][1] == "stop"

    sysroot = temp / "sys"
    (sysroot / "class/dmi/id").mkdir(parents=True)
    (sysroot / "class/dmi/id/product_name").write_text(recovery.PRODUCT)
    (sysroot / "class/dmi/id/board_name").write_text(recovery.BOARD)
    hp = sysroot / "class/hwmon/hwmon17"
    hp.mkdir(parents=True)
    (hp / "name").write_text("hp")
    (hp / "pwm1_enable").write_text("2")
    (hp / "pwm1").write_text("0")
    recovery.force_manual_max(sysroot)
    assert (hp / "pwm1_enable").read_text() == "1"
    assert (hp / "pwm1").read_text() == "255"

    root = temp / "root"
    backup = temp / "backup"
    (root / "sys/class/dmi/id").mkdir(parents=True)
    (root / "sys/class/dmi/id/product_name").write_text(recovery.PRODUCT)
    (root / "sys/class/dmi/id/board_name").write_text(recovery.BOARD)
    originals = {relative: f"original:{relative}\n" for relative in recovery.MANAGED}
    make_tree(root, originals)
    snapshot = make_snapshot(root, backup)
    for relative in recovery.MANAGED:
        (root / relative).write_text(f"candidate:{relative}\n")

    events = []
    state = {"active": "active"}

    def runner(argv):
        events.append(argv[1] if len(argv) > 1 else argv[0])
        if argv[1:3] == ["show", "omend.service"]:
            field = next(item.split("=", 1)[1] for item in argv if item.startswith("--property="))
            values = {
                "ActiveState": state["active"],
                "MainPID": "123",
                "DropInPaths": "",
                "ExecStart": "{ path=/usr/bin/omend ; argv[]=/usr/bin/omend --system --config /etc/omend/config.toml ; }",
            }
            return subprocess.CompletedProcess(argv, 0, values[field] + "\n", "")
        if argv[1:3] == ["stop", "omend.service"]:
            state["active"] = "inactive"
        if argv[1:3] == ["start", "omend.service"]:
            state["active"] = "active"
        return subprocess.CompletedProcess(argv, 0, "", "")

    proc = temp / "proc/123"
    proc.mkdir(parents=True)
    (proc / "exe").symlink_to(root / "usr/bin/omend")
    with patch.object(recovery, "ensure_root"), patch.object(recovery, "verify_snapshot", return_value=(backup, snapshot)), \
            patch.object(recovery.os, "fchown"), patch.object(recovery.os, "chown"), \
            patch.object(recovery, "force_manual_max", side_effect=lambda _sysfs: events.append("max")):
        recovery.rollback(backup, root=root, runner=runner, sysfs=sysroot, proc_root=temp / "proc")
    assert events.index("stop") < events.index("max") < events.index("daemon-reload") < events.index("start")
    assert state["active"] == "active"
    for relative, content in originals.items():
        assert (root / relative).read_text() == content

    # Repeated timer invocations after rollback only verify state; they do not
    # stop the restored daemon or repeat a fan write.
    events.clear()
    with patch.object(recovery, "read_rollback_state", return_value={
            "format": 1, "complete": True, "manual_max_requested": True}), \
            patch.object(recovery, "verify_restored_state") as verified, \
            patch.object(recovery, "force_manual_max", side_effect=AssertionError("unexpected hardware write")):
        message = recovery._rollback_locked(backup, snapshot, root=root, runner=runner,
                                            sysfs=sysroot, proc_root=temp / "proc")
    verified.assert_called_once()
    assert "operator cooling verification" in message
    assert events == []

    # If the independent fan request fails, files and the old service still get
    # restored, but the helper returns failure so systemd keeps retrying it.
    for relative in recovery.MANAGED:
        (root / relative).write_text(f"candidate-again:{relative}\n")
    (backup / recovery.STATE_NAME).unlink()
    events.clear()
    state["active"] = "active"
    with patch.object(recovery, "ensure_root"), patch.object(recovery, "verify_snapshot", return_value=(backup, snapshot)), \
            patch.object(recovery.os, "fchown"), patch.object(recovery.os, "chown"), \
            patch.object(recovery, "force_manual_max", side_effect=OSError("fixture failure")):
        try:
            recovery.rollback(backup, root=root, runner=runner, sysfs=sysroot, proc_root=temp / "proc")
            raise AssertionError("fan recovery failure was reported as success")
        except RuntimeError as error:
            assert "manual maximum request failed" in str(error)
    assert state["active"] == "active"
    assert all((root / relative).read_text() == content for relative, content in originals.items())

    # A failed stop must prevent both sysfs recovery and file restoration.
    for relative in recovery.MANAGED:
        (root / relative).write_text(f"candidate-stop-failure:{relative}\n")
    (backup / recovery.STATE_NAME).unlink()
    events.clear()

    def failed_stop(argv):
        if argv[1:3] == ["stop", "omend.service"]:
            events.append("stop")
            raise subprocess.CalledProcessError(1, argv)
        return runner(argv)

    with patch.object(recovery, "ensure_root"), patch.object(recovery, "verify_snapshot", return_value=(backup, snapshot)), \
            patch.object(recovery.os, "fchown"), patch.object(recovery.os, "chown"), \
            patch.object(recovery, "force_manual_max", side_effect=AssertionError("must not race the service")):
        try:
            recovery.rollback(backup, root=root, runner=failed_stop, sysfs=sysroot, proc_root=temp / "proc")
            raise AssertionError("failed service stop was reported as success")
        except subprocess.CalledProcessError:
            pass
    assert events == ["stop"]
    assert state["active"] == "active"
    assert all((root / relative).read_text().startswith("candidate-stop-failure:") for relative in recovery.MANAGED)

    # An incomplete file restore leaves the candidate service stopped so the
    # repeating timer can retry without running mixed binaries and policy.
    events.clear()
    with patch.object(recovery, "ensure_root"), patch.object(recovery, "verify_snapshot", return_value=(backup, snapshot)), \
            patch.object(recovery.os, "fchown"), patch.object(recovery.os, "chown"), \
            patch.object(recovery, "force_manual_max", side_effect=lambda _sysfs: events.append("max")), \
            patch.object(recovery, "atomic_restore", side_effect=OSError("fixture restore failure")):
        try:
            recovery.rollback(backup, root=root, runner=runner, sysfs=sysroot, proc_root=temp / "proc")
            raise AssertionError("incomplete restoration was reported as success")
        except RuntimeError as error:
            assert "restoration incomplete" in str(error)
    assert state["active"] == "inactive"
    assert "start" not in events

print("Hardware recovery fixtures passed; no system service or hardware was accessed.")
