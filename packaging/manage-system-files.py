#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""File-only backup/restore/removal; DESTDIR supports unprivileged staging."""
import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess

FILES = ["usr/bin/omend", "usr/bin/omen-cli", "usr/bin/omen-gui",
         "usr/share/applications/omen-gui.desktop",
         "usr/share/icons/hicolor/scalable/apps/omen-gui.svg",
         "etc/dbus-1/system.d/dev.omend.conf", "etc/systemd/system/omend.service",
         "usr/lib/systemd/system-sleep/omend"]
CONFIG = "etc/omend"
LICENSES = "usr/share/licenses/omend"


def erase(path):
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)


def preserve_ownership(source, destination):
    if os.geteuid() != 0:
        return
    metadata = source.lstat()
    os.chown(destination, metadata.st_uid, metadata.st_gid, follow_symlinks=False)
    if source.is_dir() and not source.is_symlink():
        for child in source.iterdir():
            preserve_ownership(child, destination / child.name)


def copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir() and not source.is_symlink():
        shutil.copytree(source, destination, symlinks=True)
    elif source.is_symlink():
        destination.symlink_to(os.readlink(source))
    else:
        shutil.copy2(source, destination)
    preserve_ownership(source, destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["backup", "restore", "remove"])
    parser.add_argument("--backup", type=Path)
    args = parser.parse_args()
    root = Path(os.environ.get("DESTDIR", "/"))
    if not root.is_absolute():
        parser.error("DESTDIR must be absolute")
    root = root.resolve()
    if args.backup is not None:
        if not args.backup.is_absolute():
            parser.error("--backup must be absolute")
        args.backup = args.backup.resolve()
        if any(args.backup.is_relative_to((root / name).resolve()) for name in [CONFIG, LICENSES]):
            parser.error("backup must be outside managed configuration and license directories")
    if root == Path("/") and args.action != "backup":
        active = subprocess.run(["systemctl", "is-active", "--quiet", "omend.service"], check=False)
        if active.returncode != 3:
            parser.error("confirm omend.service is inactive before changing installed files; monitor cooling first")
    paths = FILES + [CONFIG, LICENSES]
    if args.action == "backup":
        if args.backup is None or not args.backup.is_absolute():
            parser.error("backup requires an absolute --backup path")
        args.backup.mkdir(mode=0o700, parents=True, exist_ok=False)
        present = []
        for name in paths:
            source = root / name
            if source.exists() or source.is_symlink():
                copy(source, args.backup / "files" / name)
                present.append(name)
        (args.backup / "manifest.json").write_text(json.dumps(present))
    elif args.action == "restore":
        if args.backup is None:
            parser.error("restore requires --backup")
        if os.geteuid() == 0:
            # Restored files land in /usr/bin and /etc; only root may have shaped them.
            for trusted in [args.backup, args.backup / "manifest.json", args.backup / "files"]:
                if not trusted.exists() and not trusted.is_symlink() and trusted.name == "files":
                    continue
                metadata = trusted.lstat()
                if stat.S_ISLNK(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
                    parser.error(f"backup must be root-owned and not group/other writable: {trusted}")
        present = json.loads((args.backup / "manifest.json").read_text())
        if not isinstance(present, list) or any(name not in paths for name in present):
            parser.error("invalid backup manifest")
        for name in present:
            source = args.backup / "files" / name
            if not source.exists() and not source.is_symlink():
                parser.error(f"incomplete backup: {name}")
        for name in paths:
            erase(root / name)
            if name in present:
                copy(args.backup / "files" / name, root / name)
    else:
        for name in FILES + [LICENSES]:
            erase(root / name)
        print("Removed managed system files; configuration and user audio installation preserved.")
    print("No service activation or hardware commands performed.")


if __name__ == "__main__":
    main()
