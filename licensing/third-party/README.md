# Supplemental upstream license texts

These small files fill license texts omitted from some published Cargo packages.
`manifest.json` records package/version, repository, exact commit from the
package's `.cargo_vcs_info.json`, original filename/URL and SHA-256. Text files
are named by content hash and original filename to share identical copies safely.
No dependency implementation code is included. These files do not select a
license route or establish legal clearance.

The offline inventory generator checks package revision and text checksums before
using supplements. To explicitly download the same reviewed files again:

```sh
python3 scripts/fetch-license-supplements.py --fetch
```

The helper verifies both commit-pinned URL structure and expected checksums.
Adding a new package or version requires an explicit manifest review against its
published crate metadata; this helper never substitutes a moving HEAD branch.

No license text was found in either the published package or exact upstream root
for drm-fourcc 2.2.0, libseat 0.2.4 and libseat-sys 0.2.0. Their declared MIT
expressions remain documented; copyright attribution/text completeness requires
further review. These entries deliberately have no invented text.

The inventory includes inactive optional packages; their presence does not mean
they are linked into the default binary. If Skia features are enabled, the
rust-skia wrapper MIT license does not inventory the native engine's embedded
components, whose notices require a separate review.
The skia-bindings crate also records a dirty upstream VCS revision; the fetched
wrapper text's exact source provenance does not prove published source identity.
