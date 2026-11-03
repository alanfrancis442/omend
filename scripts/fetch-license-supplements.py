#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Explicit network refresh of reviewed, exact-commit, SHA-256-pinned license texts."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import urllib.request

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fetch", action="store_true", help="required acknowledgment of network access")
    args = parser.parse_args()
    if not args.fetch:
        parser.error("pass --fetch to download the manifest's pinned license URLs")
    directory = ROOT / "licensing/third-party"
    manifest = json.loads((directory / "manifest.json").read_text())
    fetched = set()
    for package in manifest["packages"]:
        revision = package["revision"]
        if not re.fullmatch(r"[0-9a-f]{40}", revision):
            raise ValueError("upstream revision must be an exact Git SHA")
        repository = package["repository"].removeprefix("https://github.com/")
        for item in package["files"]:
            url = item["source_url"]
            if not url.startswith(f"https://raw.githubusercontent.com/{repository}/{revision}/"):
                raise ValueError("supplement URL is not pinned to the recorded repository commit")
            name = item["path"]
            if Path(name).name != name:
                raise ValueError("supplement output must be a simple filename")
            target = directory / name
            if target.is_symlink():
                raise ValueError("supplement output cannot be a symlink")
            if (url, name) in fetched:
                continue
            request = urllib.request.Request(url, headers={"User-Agent": "omend-license-review"})
            with urllib.request.urlopen(request, timeout=30) as response:
                data = response.read()
            if hashlib.sha256(data).hexdigest() != item["sha256"]:
                raise ValueError(f"upstream license checksum changed: {url}")
            target.write_bytes(data)
            fetched.add((url, name))
    print(f"Fetched and verified {len(fetched)} pinned license sources; legal review remains required.")


if __name__ == "__main__":
    main()
