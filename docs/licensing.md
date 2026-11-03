# Licensing and redistribution

Userspace project code is available under MIT OR Apache-2.0. The optional
keyboard kernel module is GPL-2.0-only and is excluded from the native binary
bundle. These project grants do not replace dependency licenses.

## Selected Slint route

The maintainer selected preparation under the Slint Royalty-free Desktop,
Mobile, and Web Applications License version 2.0. The locked Slint 1.18.1
license text provides the relevant terms. The GUI now includes Slint's actual
`AboutSlint` widget on an About page accessible from the desktop and compact
top-level navigation. The widget is compiled into the application rather than
replaced with a plain-text credit. See [Slint's published terms](https://slint.dev/terms-and-conditions).

The native bundle includes the exact dependency's available license texts in
its generated `third-party-licenses/` tree. The GUI is a desktop application;
this selection does not authorize redistribution of standalone Slint libraries,
API exports or an embedded-device derivative. Review those cases separately.
Project source headers remain MIT OR Apache-2.0; dependency terms still apply to
the linked application. A visual check of the About page on supported desktops
remains part of release validation.

## Inventory and remaining review

Run `RUSTUP_TOOLCHAIN=stable python3 scripts/generate-notices.py` on the current
development host, or omit the override when the repository's pinned toolchain
is installed. The generator uses locked, offline host metadata, copies
package-contained and pinned supplemental license texts, and records hashes.
`--check` verifies an existing inventory without replacing it. The bundle builder
generates a fresh inventory directly into the staged bundle.

The inventory covers dependencies reachable in the resolved host graph,
including build/test dependencies and optional components; it is broader than
the default binaries' active dependency tree. It is an inventory, not legal
clearance. Missing package texts, feature-dependent native dependencies, bundled
assets and system-library redistribution require review. Supplemental provenance
and unresolved entries are documented in
[the supplement record](../licensing/third-party/README.md).

The known `drm-fourcc`, `libseat` and `libseat-sys` missing-text entries and
native Skia notice requirements must be assessed against the actual features
distributed. The current default native dependency tree does not activate those
packages; changing renderer/backend features requires a fresh review. Do not
infer a general distribution permission from a missing license file or a clean
vulnerability scan.

Official candidate mode requires a clean matching version tag and an explicit
`OMEND_LICENSE_REVIEWED_REVISION` attestation naming the reviewed full commit.
That attestation is a maintainer sign-off, not an automated legal conclusion.
Never set it merely to bypass an unresolved distribution review. The local
candidate build does not require or imply that sign-off.
