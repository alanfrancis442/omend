#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Exercise maintainer guards using fake processes/service responses only."""
import os
from pathlib import Path
import subprocess
import tempfile

guard = Path(__file__).with_name("deb-service-guard.sh")
with tempfile.TemporaryDirectory(prefix="omend-deb-guard-") as temporary:
    root = Path(temporary)
    service = root / "systemctl"
    service.write_text('#!/bin/sh\n[ "$*" = "show --property=ActiveState --value omend.service" ] || exit 90\nprintf "%s\\n" "$FAKE_STATE"\nexit "$FAKE_QUERY_EXIT"\n')
    process = root / "pgrep"
    process.write_text('#!/bin/sh\n[ "$*" = "-x omend" ] || exit 90\nexit "$FAKE_PROCESS_EXIT"\n')
    service.chmod(0o755)
    process.chmod(0o755)
    cases = [("inactive", 0, 1, True), ("failed", 0, 1, True),
             ("active", 0, 1, False), ("activating", 0, 1, False),
             ("deactivating", 0, 1, False), ("inactive", 0, 0, False),
             ("inactive", 0, 2, False), ("inactive", 1, 1, False),
             ("", 0, 1, False)]
    for state, query, running, allowed in cases:
        env = dict(os.environ, PATH=str(root), FAKE_STATE=state,
                   FAKE_QUERY_EXIT=str(query), FAKE_PROCESS_EXIT=str(running))
        result = subprocess.run(["/bin/sh", str(guard), "configure"], env=env,
                                capture_output=True, text=True)
        assert (result.returncode == 0) == allowed, (state, query, running, result.stderr)
print("Nine file-only package guard scenarios pass; no real systemctl/pgrep invoked.")
