#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Check bundle-local Markdown link targets; external URLs and anchors are skipped."""
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

root = Path(sys.argv[1]).resolve()
failures = []
for markdown in sorted(root.rglob("*.md")):
    if markdown.relative_to(root).parts[0] == "third-party-licenses":
        continue  # Upstream texts may link into their original source repositories.
    for target in re.findall(r"\]\(([^)]+)\)", markdown.read_text(errors="replace")):
        target = target.split(' "', 1)[0].strip("<>")
        link = urlsplit(target)
        if link.scheme or link.netloc or not link.path:
            continue
        path = (markdown.parent / unquote(link.path)).resolve()
        if not path.is_relative_to(root) or not path.exists():
            failures.append(f"{markdown.relative_to(root)}: {target}")
if failures:
    raise SystemExit("Broken bundle links:\n" + "\n".join(failures))
print("Bundle-local Markdown link targets exist (anchors not validated).")
