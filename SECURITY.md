# Security policy

## Project status

This is an experimental development preview. There is no supported release
series, published security-update schedule or response-time commitment yet.
The root daemon changes hardware settings; membership in the `omen` group grants
ongoing control rather than checking for an active local desktop session.
See [daemon safety](docs/daemon-safety.md) for the trust boundary and limitations.

## Reporting a vulnerability

Report vulnerabilities privately through GitHub private vulnerability
reporting: open the repository's **Security** tab and choose **Report a
vulnerability**, or go to
<https://github.com/alanfrancis442/omend/security/advisories/new>.

Do not open a public issue, discussion or pull request for a suspected
vulnerability. If private reporting is unavailable, open a public issue that
only asks for a private contact, without exploit details, secrets, full logs or
hardware identifiers. Ordinary non-sensitive bugs can follow
[CONTRIBUTING.md](CONTRIBUTING.md).

A private report should describe affected revisions, the trust boundary crossed,
required privileges, impact, a minimal reproduction and possible mitigation.
Redact serial numbers, account names and unrelated journal content. Do not run
thermal stress or destructive hardware experiments to prove a vulnerability.

## Local precautions

Install only artifacts whose origin and checksum you have checked. Review the
binary and D-Bus policy together; installing a new executable under an old
permissive policy does not enable the new authorization rules. Avoid running
the GUI or audio helper as root. Keep rollback copies before changing a service
that controls cooling. A checksum verifies file identity, not publisher trust
or physical hardware safety.
