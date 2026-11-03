#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 alanfrancis442
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Inventory locked host dependencies and copy license texts; not legal clearance."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parent.parent
NOTICE = "THIRD_PARTY_NOTICES.md"
TEXTS = "third-party-licenses"
REPORT = "license-inventory.json"


def command(*args):
    return subprocess.check_output(args, cwd=REPO, text=True)


def regular_within(path, package_root):
    """Reject external symlinks and nonregular files without traversing them."""
    try:
        resolved = path.resolve(strict=True)
        resolved.relative_to(package_root.resolve(strict=True))
        return resolved.is_file()
    except (OSError, RuntimeError, ValueError):
        return False


def license_files(package, recursive=True):
    root = Path(package["manifest_path"]).parent
    files = set()
    declared = package.get("license_file")
    if declared:
        declared = Path(declared)
        if not declared.is_absolute():
            declared = root / declared
        if regular_within(declared, root):
            files.add(declared)
    excluded = {"target", "node_modules", ".git", ".hg", ".svn", "dist", "__pycache__", ".cache"}
    for current, directories, names in os.walk(root, followlinks=False):
        current = Path(current)
        directories[:] = sorted(d for d in directories if d not in excluded and not (current / d).is_symlink())
        relative = current.relative_to(root)
        if not recursive:
            # Workspace scanning is restricted to root texts and root LICENSES.
            if current == root:
                directories[:] = [d for d in directories if d.upper() == "LICENSES"]
        inside_license_dir = any(part.upper() == "LICENSES" for part in relative.parts)
        for name in sorted(names):
            path = current / name
            if inside_license_dir or re.match(r"^(licen[sc]e|copying|notice|unlicense)([._-].*)?$", name, re.I):
                if regular_within(path, root):
                    files.add(path)
    return root, sorted(files, key=lambda p: p.relative_to(root).as_posix())


def inventory(destination):
    host = next(line.split(": ", 1)[1] for line in command("rustc", "-vV").splitlines() if line.startswith("host: "))
    metadata = json.loads(command("cargo", "metadata", "--format-version", "1", "--locked", "--offline", "--filter-platform", host))
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    reachable = set()
    pending = list(metadata["workspace_members"])
    while pending:
        package_id = pending.pop()
        if package_id not in reachable:
            reachable.add(package_id)
            pending.extend(nodes[package_id]["dependencies"])
    packages = sorted((p for p in metadata["packages"] if p["id"] in reachable), key=lambda p: (p["name"], p["version"], p["id"]))
    destination.mkdir(parents=True, exist_ok=True)
    texts = destination / TEXTS
    texts.mkdir()
    lines = ["# Locked dependency license inventory", "", "This generated inventory is **not legal clearance** and does not select a license", "route for dependencies offering alternatives. Review redistribution obligations,", "copyright notices, source requirements and any LicenseRef terms before publishing.", "It includes Cargo metadata host-target dependencies reachable from all workspace packages, including", "inactive optional, build and test dependencies; it is broader than the active binary dependency tree.", "System libraries, toolchains and optional kernel-module distribution require separate review.", "If Skia features are enabled, native binaries and embedded components require their own transitive", "copyright/notice inventory; rust-skia wrapper license texts do not close that review.", "", f"Target: `{host}`", "", f"Packages inventoried: {len(packages)}", "", "Only regular license/notice files contained in each package source are copied.", "Missing files or license expressions require manual investigation. No dependency", "implementation code is vendored. Workspace license texts also ship at bundle root.", ""]
    supplement_path = REPO / "licensing/third-party/manifest.json"
    supplements = json.loads(supplement_path.read_text())["packages"] if supplement_path.exists() else []
    supplements = {(p["name"], p["version"]): p for p in supplements}
    records = []
    for package in packages:
        source_label = package.get("source") or "workspace/local path"
        identity = f"{package['name']}@{package['version']}|{source_label}"
        suffix = hashlib.sha256(identity.encode()).hexdigest()[:12]
        folder = f"{package['name']}-{package['version']}-{suffix}"
        root, files = license_files(package)
        if package["id"] in metadata["workspace_members"]:
            # Workspace crates inherit project licensing from the workspace root.
            root, files = license_files({"manifest_path": str(REPO / "Cargo.toml")}, recursive=False)
        lines += [f"## {package['name']} {package['version']}", "", f"Declared license: `{package.get('license') or 'UNSPECIFIED'}`", "", f"Source: `{package.get('source') or 'workspace/local path'}`", ""]
        if package.get("license_file"):
            lines += [f"Declared license file: `{Path(package['license_file']).name}`", ""]
        record = {"name": package["name"], "version": package["version"],
                  "source": source_label, "license": package.get("license"),
                  "license_file_declared": bool(package.get("license_file")), "texts": []}
        if not files and not supplements.get((package["name"], package["version"]), {}).get("files"):
            lines += ["No package-contained or pinned upstream license text found; review required.", ""]
        else:
            for source in files:
                relative = source.relative_to(root)
                target = texts / folder / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
                digest = hashlib.sha256(target.read_bytes()).hexdigest()
                link = target.relative_to(destination).as_posix().replace(" ", "%20")
                lines.append(f"- [{relative.as_posix()}]({link}) — SHA-256 `{digest}`")
                record["texts"].append({"path": target.relative_to(destination).as_posix(), "sha256": digest})
            lines.append("")
        supplement = supplements.get((package["name"], package["version"]))
        if supplement and not files:
            vcs_path = Path(package["manifest_path"]).parent / ".cargo_vcs_info.json"
            vcs = json.loads(vcs_path.read_text()) if vcs_path.exists() else {}
            if vcs.get("git", {}).get("sha1") != supplement["revision"]:
                raise ValueError(f"supplement revision mismatch: {package['name']}")
            record["supplement_revision"] = supplement["revision"]
            record["upstream_vcs_dirty"] = supplement.get("vcs_dirty", False)
            for item in supplement["files"]:
                source = REPO / "licensing/third-party" / item["path"]
                if not regular_within(source, REPO / "licensing/third-party"):
                    raise ValueError("invalid supplement path")
                if hashlib.sha256(source.read_bytes()).hexdigest() != item["sha256"]:
                    raise ValueError(f"supplement checksum mismatch: {source}")
                if Path(item["path"]).name != item["path"]:
                    raise ValueError("invalid supplement filename")
                target = texts / folder / item["path"]
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
                record["texts"].append({"path": target.relative_to(destination).as_posix(),
                                       "sha256": item["sha256"], "source_url": item["source_url"]})
                lines.append(f"- Upstream supplement: [{item['upstream_path']}]({target.relative_to(destination).as_posix()}) from [{supplement['revision'][:12]}]({item['source_url']})")
            if supplement.get("vcs_dirty"):
                lines += ["", "Upstream crate metadata marks this revision dirty; text provenance does not prove the published source is identical.", ""]
        records.append(record)
    report = {"schema_version": 1, "legal_clearance": False, "target": host,
              "cargo_lock_sha256": hashlib.sha256((REPO / "Cargo.lock").read_bytes()).hexdigest(),
              "package_count": len(packages),
              "missing_license_text_count": sum(not p["texts"] for p in records),
              "missing_license_expression_count": sum(not p["license"] for p in records),
              "review_gaps": ["If Skia features are enabled, native transitive/embedded third-party notices require separate review",
                              "An SPDX license expression or copied text is not legal clearance"],
              "packages": records}
    (destination / REPORT).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    (destination / NOTICE).write_text("\n".join(lines) + "\n")
    return len(packages)


def contents(root):
    return {p.relative_to(root).as_posix(): p.read_bytes() for p in root.rglob("*") if p.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=REPO / "dist/license-inventory")
    parser.add_argument("--check", action="store_true", help="compare existing output against current locked host metadata")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="omend-notices-") as temporary:
        stage = Path(temporary) / "inventory"
        count = inventory(stage)
        if args.check:
            expected = contents(stage)
            actual = {}
            for name in [NOTICE, REPORT, TEXTS]:
                path = args.output_dir / name
                if path.is_file():
                    actual[name] = path.read_bytes()
                elif path.is_dir():
                    actual.update({f"{name}/{key}": value for key, value in contents(path).items()})
            if actual != expected:
                print("License inventory differs or is missing; regenerate and review.", file=sys.stderr)
                return 1
        else:
            args.output_dir.mkdir(parents=True, exist_ok=True)
            for name in [NOTICE, REPORT, TEXTS]:
                target = args.output_dir / name
                if target.is_symlink():
                    raise ValueError(f"refusing output symlink: {target}")
                if target.is_dir():
                    shutil.rmtree(target)
                elif target.exists():
                    target.unlink()
                source = stage / name
                if source.is_dir():
                    shutil.copytree(source, target)
                else:
                    shutil.copyfile(source, target)
    print(f"{'Checked' if args.check else 'Generated'} {count} package notices in {args.output_dir}; license review remains required.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
