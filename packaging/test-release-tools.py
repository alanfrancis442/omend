#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Temporary-root and mocked-host checks; never touch installed system files."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("manage_files", ROOT / "manage-system-files.py")
manage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manage)
with tempfile.TemporaryDirectory() as temporary:
    temp = Path(temporary)
    (temp / "root-alias").symlink_to("/", target_is_directory=True)
    with patch.dict(os.environ, {"DESTDIR": str(temp / "root-alias")}), patch.object(sys, "argv", ["manage", "remove"]), patch.object(manage.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as service, patch.object(manage, "erase") as erase, contextlib.redirect_stderr(io.StringIO()):
        try:
            manage.main()
            raise AssertionError("root alias bypassed live service guard")
        except SystemExit as error:
            assert error.code == 2
        service.assert_called_once()
        erase.assert_not_called()
    (temp / "stage/etc/omend").mkdir(parents=True)
    with patch.dict(os.environ, {"DESTDIR": str(temp / "stage")}), patch.object(sys, "argv", ["manage", "backup", "--backup", str(temp / "stage/etc/omend/recursive")]), contextlib.redirect_stderr(io.StringIO()):
        try:
            manage.main()
            raise AssertionError("recursive backup accepted")
        except SystemExit as error:
            assert error.code == 2
        assert not (temp / "stage/etc/omend/recursive").exists()
    untrusted = temp / "user-backup"
    (untrusted / "files").mkdir(parents=True)
    (untrusted / "manifest.json").write_text("[]")
    if os.geteuid() != 0:
        with patch.dict(os.environ, {"DESTDIR": str(temp / "stage")}), patch.object(sys, "argv", ["manage", "restore", "--backup", str(untrusted)]), patch.object(manage.os, "geteuid", return_value=0), patch.object(manage, "erase") as erase, contextlib.redirect_stderr(io.StringIO()):
            try:
                manage.main()
                raise AssertionError("root restore accepted a user-owned backup")
            except SystemExit as error:
                assert error.code == 2
            erase.assert_not_called()
    source = temp / "owned"
    source.write_text("settings")
    with patch.object(manage.os, "geteuid", return_value=0), patch.object(manage.os, "chown") as chown:
        manage.copy(source, temp / "restored")
        metadata = source.lstat()
        chown.assert_called_once_with(temp / "restored", metadata.st_uid, metadata.st_gid, follow_symlinks=False)
    bundle = temp / "bundle"
    bundle.mkdir()
    (bundle / "settings").write_text("unchanged payload")
    one, two = temp / "one.tar.gz", temp / "two.tar.gz"
    subprocess.run(["sh", str(ROOT / "normalized-archive.sh"), str(temp), "bundle", str(one), "1700000000"], check=True)
    os.utime(bundle / "settings", (1800000000, 1800000000))
    subprocess.run(["sh", str(ROOT / "normalized-archive.sh"), str(temp), "bundle", str(two), "1700000000"], check=True)
    assert one.read_bytes() == two.read_bytes(), "normalized archives differ"
print("Root-alias, recursive-backup, untrusted-restore, ownership and normalized-archive checks passed.")
