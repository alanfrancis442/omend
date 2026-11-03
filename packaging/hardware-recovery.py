#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Prepare a private rollback snapshot or recover the previous omend install.

`prepare` writes a private backup but does not change the service or hardware.
It snapshots the active daemon, policy, unit, and configuration, then prints a
systemd timer command. The copied helper can run independently of the checkout.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import tomllib

PRODUCT = "OMEN by HP Gaming Laptop 16-xf0xxx"
BOARD = "8BCA"
MANAGED = (
    "usr/bin/omend",
    "etc/dbus-1/system.d/dev.omend.conf",
    "etc/systemd/system/omend.service",
    "etc/omend/config.toml",
)
BACKUP_PARENT = Path("/var/backups/omend")
HELPER_NAME = "rollback-omend.py"
MANIFEST_NAME = "snapshot.json"
STATE_NAME = "rollback-state.json"


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def regular_file(path):
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError:
        return False
    if not stat.S_ISREG(mode) or path.stat(follow_symlinks=False).st_nlink != 1:
        raise ValueError(f"expected a regular file: {path}")
    return True


def check_parent_chain(path, root):
    path = Path(path)
    root = Path(root)
    current = root
    for component in path.relative_to(root).parts[:-1]:
        current = current / component
        if current.is_symlink() or not current.is_dir():
            raise ValueError(f"path parent must be a real directory: {current}")


def ensure_root():
    if os.geteuid() != 0:
        raise PermissionError("run this administrative helper as root")


def checked_command(argv, *, check=True):
    return subprocess.run(argv, check=check, text=True, capture_output=True, timeout=30)


def service_value(name, runner=checked_command):
    result = runner([
        "/usr/bin/systemctl", "show", "omend.service", f"--property={name}", "--value"
    ])
    return result.stdout.strip()


def check_target(root):
    check_hardware_identity(Path(root) / "sys")


def check_hardware_identity(sysfs):
    product = (Path(sysfs) / "class/dmi/id/product_name").read_text().strip()
    board = (Path(sysfs) / "class/dmi/id/board_name").read_text().strip()
    if product != PRODUCT or board != BOARD:
        raise ValueError(f"unsupported test target: {product!r}, board {board!r}")


def copy_preserving_metadata(source, destination):
    if not regular_file(source):
        raise ValueError(f"required snapshot file is missing: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    shutil.copy2(source, destination, follow_symlinks=False)
    metadata = source.stat(follow_symlinks=False)
    os.chown(destination, metadata.st_uid, metadata.st_gid, follow_symlinks=False)
    if digest(source) != digest(destination):
        raise OSError(f"snapshot copy failed verification: {source}")
    fd = os.open(destination, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def metadata(path):
    st = path.stat(follow_symlinks=False)
    attrs = {}
    if hasattr(os, "listxattr"):
        for name in sorted(os.listxattr(path, follow_symlinks=False)):
            attrs[name] = hashlib.sha256(
                os.getxattr(path, name, follow_symlinks=False)
            ).hexdigest()
    return {
        "sha256": digest(path), "mode": stat.S_IMODE(st.st_mode),
        "uid": st.st_uid, "gid": st.st_gid, "xattrs_sha256": attrs,
    }


def private_backup_directory(path, owner_uid=0):
    path = Path(path)
    if path.parent != BACKUP_PARENT or path.name in ("", ".", ".."):
        raise ValueError("rollback data must be directly under /var/backups/omend")
    if path.is_symlink() or not path.is_dir():
        raise ValueError("rollback directory must be a real directory")
    st = path.stat(follow_symlinks=False)
    if st.st_uid != owner_uid or stat.S_IMODE(st.st_mode) != 0o700:
        raise ValueError("rollback directory must have the expected owner and mode 0700")
    parent_st = path.parent.stat(follow_symlinks=False)
    if path.parent.is_symlink() or parent_st.st_uid != owner_uid or stat.S_IMODE(parent_st.st_mode) & 0o077:
        raise ValueError("rollback parent must have the expected owner and private permissions")
    return path


def timer_command(unit, helper, *, first_delay="120s", repeat_interval="15s",
                  interpreter="/usr/bin/python3"):
    return [
        "/usr/bin/systemd-run", f"--unit={unit}", f"--on-active={first_delay}",
        f"--on-unit-inactive={repeat_interval}", "--timer-property=AccuracySec=1s",
        "--property=Type=oneshot", "--property=StartLimitIntervalSec=0",
        "--property=TimeoutStartSec=45s", interpreter, str(helper), "--rollback",
    ]


def prepare(root=Path("/"), backup_parent=BACKUP_PARENT, expected_helper_sha256=None,
            *, service_reader=service_value, proc_root=Path("/proc"), owner_uid=0):
    ensure_root()
    root = Path(root)
    check_target(root)
    if service_reader("ActiveState") != "active":
        raise ValueError("omend.service must be active before preparing a test")
    if service_reader("DropInPaths"):
        raise ValueError("omend.service drop-ins are present; review them before testing")
    expected_exec = "/usr/bin/omend --system --config /etc/omend/config.toml"
    command = service_reader("ExecStart")
    if ("path=/usr/bin/omend" not in command
            or f"argv[]={expected_exec}" not in command):
        raise ValueError("omend.service does not use the expected daemon/configuration")
    pid = service_reader("MainPID")
    if not pid.isdigit() or int(pid) <= 0:
        raise ValueError("active omend.service has no MainPID")
    if digest(Path(proc_root) / pid / "exe") != digest(root / "usr/bin/omend"):
        raise ValueError("running daemon does not match the installed executable")

    config = root / "etc/omend/config.toml"
    for relative in MANAGED:
        check_parent_chain(root / relative, root)
    tomllib.loads(config.read_text(encoding="utf-8"))
    source_helper = Path(__file__)
    if source_helper.is_symlink() or not source_helper.is_file():
        raise ValueError("the rollback helper source must be a regular file")
    actual_helper_sha256 = digest(source_helper)
    if (not isinstance(expected_helper_sha256, str) or len(expected_helper_sha256) != 64
            or any(char not in "0123456789abcdef" for char in expected_helper_sha256)
            or actual_helper_sha256 != expected_helper_sha256):
        raise ValueError("provide the independently reviewed rollback-helper SHA-256")
    parent = Path(backup_parent)
    parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    check_parent_chain(parent, Path("/"))
    if (parent.is_symlink() or parent.stat().st_uid != owner_uid
            or stat.S_IMODE(parent.stat().st_mode) != 0o700):
        raise ValueError("/var/backups/omend must be a real root-owned directory")
    name = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime()) + f"-{os.getpid()}"
    backup = parent / name
    backup.mkdir(mode=0o700)
    os.chown(backup, owner_uid, 0)
    try:
        entries = {}
        for relative in MANAGED:
            source = root / relative
            destination = backup / "files" / relative
            expected = metadata(source)
            copy_preserving_metadata(source, destination)
            if metadata(destination) != expected or metadata(source) != expected:
                raise OSError(f"source changed or metadata differed during snapshot: {relative}")
            entries[relative] = expected
        helper = backup / HELPER_NAME
        shutil.copyfile(source_helper, helper, follow_symlinks=False)
        if digest(helper) != actual_helper_sha256:
            raise OSError("copied rollback helper failed checksum verification")
        os.chmod(helper, 0o500)
        os.chown(helper, owner_uid, 0)
        data = {
            "format": 1,
            "target": {"product": PRODUCT, "board": BOARD},
            "service": {"active": True, "exec_start": expected_exec, "drop_ins": []},
            "files": entries,
            "helper_sha256": actual_helper_sha256,
        }
        manifest = backup / MANIFEST_NAME
        with manifest.open("w", encoding="utf-8") as output:
            output.write(json.dumps(data, sort_keys=True, indent=2) + "\n")
            output.flush()
            os.fsync(output.fileno())
        os.chmod(manifest, 0o400)
        os.chown(manifest, owner_uid, 0)
        manifest_fd = os.open(manifest, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        try:
            os.fsync(manifest_fd)
        finally:
            os.close(manifest_fd)
        directory_fd = os.open(backup, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    except Exception:
        shutil.rmtree(backup, ignore_errors=True)
        raise
    unit = "omend-hardware-recovery-" + name.replace("T", "-").replace(":", "")
    return {
        "backup": str(backup),
        "helper": str(helper),
        "timer_unit": f"{unit}.timer",
        "arm_timer_command": timer_command(unit, helper),
        "verify_timer_command": ["/usr/bin/systemctl", "is-active", f"{unit}.timer"],
        "cancel_timer_command": ["/usr/bin/systemctl", "stop", f"{unit}.timer"],
        "timer_must_remain_armed_until": [
            "restored file hashes and metadata verified", "old daemon active with expected executable",
            "operator confirms both fans and temperature trend are safe",
        ],
    }


def verify_snapshot(backup, owner_uid=0):
    backup = private_backup_directory(backup, owner_uid=owner_uid)
    manifest_path = backup / MANIFEST_NAME
    helper = backup / HELPER_NAME
    if not regular_file(manifest_path) or not regular_file(helper):
        raise ValueError("rollback helper or snapshot manifest is missing")
    for path in (manifest_path, helper):
        st = path.stat(follow_symlinks=False)
        if st.st_uid != owner_uid or st.st_mode & 0o022:
            raise ValueError("rollback metadata is not protected from group/other writes")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    if data.get("format") != 1 or data.get("target") != {"product": PRODUCT, "board": BOARD}:
        raise ValueError("unsupported rollback snapshot")
    if data.get("service") != {
        "active": True,
        "exec_start": "/usr/bin/omend --system --config /etc/omend/config.toml",
        "drop_ins": [],
    }:
        raise ValueError("snapshot does not describe the expected active service")
    if data.get("helper_sha256") != digest(helper):
        raise ValueError("rollback helper checksum mismatch")
    files = data.get("files")
    if not isinstance(files, dict) or set(files) != set(MANAGED):
        raise ValueError("rollback snapshot has an unexpected file set")
    for relative, expected in files.items():
        source = backup / "files" / relative
        if not regular_file(source) or metadata(source) != expected:
            raise ValueError(f"rollback snapshot integrity/metadata mismatch: {relative}")
    return backup, data


def atomic_restore(source, destination):
    if not regular_file(source):
        raise ValueError(f"snapshot file is missing: {source}")
    if not destination.parent.is_dir() or destination.parent.is_symlink():
        raise ValueError(f"destination parent must be an existing real directory: {destination.parent}")
    with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".omend-restore-", delete=False) as temp:
        staged = Path(temp.name)
    try:
        shutil.copy2(source, staged, follow_symlinks=False)
        original = source.stat(follow_symlinks=False)
        os.chown(staged, original.st_uid, original.st_gid, follow_symlinks=False)
        staged_fd = os.open(staged, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        try:
            os.fsync(staged_fd)
        finally:
            os.close(staged_fd)
        if metadata(staged) != metadata(source):
            raise OSError(f"restored file metadata verification failed: {destination}")
        os.replace(staged, destination)
        parent_fd = os.open(destination.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
        try:
            os.fsync(parent_fd)
        finally:
            os.close(parent_fd)
    finally:
        try:
            staged.unlink()
        except FileNotFoundError:
            pass


def run_checked(argv, runner=checked_command):
    return runner(argv)


def unit_state(runner=checked_command):
    return runner([
        "/usr/bin/systemctl", "show", "omend.service", "--property=ActiveState", "--value"
    ]).stdout.strip()


def write_rollback_state(backup, state):
    path = backup / STATE_NAME
    with tempfile.NamedTemporaryFile(dir=backup, prefix=".rollback-state-", mode="w",
                                     encoding="utf-8", delete=False) as output:
        staged = Path(output.name)
        output.write(json.dumps(state, sort_keys=True) + "\n")
        output.flush()
        os.fsync(output.fileno())
    try:
        os.chown(staged, 0, 0)
        os.chmod(staged, 0o400)
        staged_fd = os.open(staged, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        try:
            os.fsync(staged_fd)
        finally:
            os.close(staged_fd)
        os.replace(staged, path)
        directory_fd = os.open(backup, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        try:
            staged.unlink()
        except FileNotFoundError:
            pass


def read_rollback_state(backup):
    path = backup / STATE_NAME
    if not regular_file(path):
        return None
    st = path.stat(follow_symlinks=False)
    if st.st_uid != 0 or st.st_mode & 0o022:
        raise ValueError("rollback state is not protected from group/other writes")
    state = json.loads(path.read_text(encoding="utf-8"))
    if (state.get("format") != 1 or not isinstance(state.get("complete"), bool)
            or not isinstance(state.get("manual_max_requested"), bool)):
        raise ValueError("invalid rollback state")
    return state


def verify_restored_state(data, *, root, runner, proc_root):
    if unit_state(runner) != "active":
        raise RuntimeError("restored omend.service is not active")
    if service_value("DropInPaths", runner):
        raise RuntimeError("unexpected omend.service drop-ins remain")
    command = service_value("ExecStart", runner)
    expected_exec = data["service"]["exec_start"]
    if ("path=/usr/bin/omend" not in command
            or f"argv[]={expected_exec}" not in command):
        raise RuntimeError("restored service does not use the original daemon/configuration")
    for relative, expected in data["files"].items():
        check_parent_chain(root / relative, root)
        if metadata(root / relative) != expected:
            raise RuntimeError(f"restored file verification failed: {relative}")
    pid = service_value("MainPID", runner)
    if not pid.isdigit() or int(pid) <= 0:
        raise RuntimeError("restored service has no MainPID")
    process_exe = Path(proc_root) / pid / "exe"
    if digest(process_exe) != data["files"]["usr/bin/omend"]["sha256"]:
        raise RuntimeError("running daemon does not match restored executable")


def force_manual_max(sysfs=Path("/sys")):
    check_hardware_identity(sysfs)
    names = list((sysfs / "class/hwmon").glob("hwmon*/name"))
    hp = []
    for name in names:
        try:
            if name.read_text().strip() == "hp":
                hp.append(name.parent.resolve())
        except OSError:
            continue
    if len(hp) != 1:
        raise ValueError("expected exactly one hp hwmon device")
    enable, pwm = hp[0] / "pwm1_enable", hp[0] / "pwm1"
    for path, value in ((enable, b"1"), (pwm, b"255")):
        fd = os.open(path, os.O_WRONLY | getattr(os, "O_NOFOLLOW", 0))
        try:
            if os.write(fd, value) != len(value):
                raise OSError(f"short sysfs write: {path.name}")
        finally:
            os.close(fd)
    if enable.read_text().strip() != "1":
        raise OSError("manual fan mode request did not read back as mode 1")


def rollback(backup, *, root=Path("/"), runner=checked_command, sysfs=Path("/sys"), proc_root=Path("/proc")):
    ensure_root()
    backup, data = verify_snapshot(backup)
    root = Path(root)
    check_target(root)
    check_hardware_identity(sysfs)
    lock_fd = os.open(backup / "rollback.lock", os.O_CREAT | os.O_RDWR, 0o600)
    os.fchown(lock_fd, 0, 0)
    fcntl.flock(lock_fd, fcntl.LOCK_EX)
    try:
        return _rollback_locked(backup, data, root=root, runner=runner, sysfs=sysfs, proc_root=proc_root)
    finally:
        os.close(lock_fd)


def _rollback_locked(backup, data, *, root, runner, sysfs, proc_root):
    prior = read_rollback_state(backup)
    if prior and prior["complete"]:
        try:
            verify_restored_state(data, root=root, runner=runner, proc_root=proc_root)
        except Exception:
            prior = None
        else:
            if not prior.get("manual_max_requested"):
                raise RuntimeError("files and daemon are restored, but manual maximum was not confirmed; operator recovery required")
            return "Files and daemon remain restored; operator cooling verification is still required."

    run_checked(["/usr/bin/systemctl", "stop", "omend.service"], runner)
    state = unit_state(runner)
    if state in ("active", "activating", "deactivating", "reloading"):
        raise RuntimeError("omend.service is still active; retaining the recovery timer")

    errors = []
    try:
        force_manual_max(sysfs)
    except Exception as error:
        errors.append(f"manual maximum request failed: {error}")

    try:
        for relative in MANAGED:
            check_parent_chain(root / relative, root)
            atomic_restore(backup / "files" / relative, root / relative)
        run_checked(["/usr/bin/systemctl", "daemon-reload"], runner)
        run_checked([
            "/usr/bin/busctl", "--system", "call", "org.freedesktop.DBus",
            "/org/freedesktop/DBus", "org.freedesktop.DBus", "ReloadConfig",
        ], runner)
    except Exception as error:
        raise RuntimeError(f"file/policy restoration incomplete; retaining recovery timer: {error}") from error

    run_checked(["/usr/bin/systemctl", "start", "omend.service"], runner)
    verify_restored_state(data, root=root, runner=runner, proc_root=proc_root)
    write_rollback_state(backup, {
        "format": 1,
        "complete": True,
        "manual_max_requested": not errors,
        "manual_max_error": "request failed" if errors else None,
    })
    if errors:
        raise RuntimeError("manual maximum request failed; files and daemon restored, timer remains armed for operator recovery")
    return "Previous files and daemon restored; timer remains armed for operator cooling verification."


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", nargs="?", choices=["prepare"], help="prepare a rollback snapshot")
    parser.add_argument("--expected-helper-sha256", help="reviewed SHA-256 required by prepare")
    parser.add_argument("--rollback", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        if args.rollback and args.action is not None:
            parser.error("choose prepare or --rollback")
        if args.rollback:
            backup = Path(__file__).resolve().parent
            print(rollback(backup))
        elif args.action == "prepare":
            result = prepare(expected_helper_sha256=args.expected_helper_sha256)
            print(json.dumps(result, indent=2))
        else:
            parser.error("specify prepare or --rollback")
    except Exception as error:
        print(f"hardware recovery error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
