---
name: Bug report
about: Report a non-sensitive application problem
title: ''
labels: ''
assignees: ''
---

For vulnerabilities, read SECURITY.md and request a private contact without
publishing exploit details. Do not repeat an unsafe hardware experiment.

**Problem and reproduction**
What happened, what did you expect, and what minimal steps reproduce it?

**Environment**
- Source revision or artifact checksum:
- Distribution and kernel:
- Product model and BIOS (no serial number):
- Installed daemon/policy or local changes:
- Optional keyboard module in use:

**Evidence**
Include relevant redacted errors, read-only `omen-cli doctor` output and, if the
daemon is running, `omen-cli status --json`. Review output before posting; omit
usernames, private paths, serial numbers, raw audio and unrelated journal content.

**Hardware observations, if applicable**
Power source, timestamps, temperatures, both fan RPM readings, requested mode,
and any abort/rollback. A detected interface is not a safety validation.
