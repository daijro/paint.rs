# Dependency notice provenance

The collector copies all supplied license and notice files from Cargo's metadata
superset, including build/test dependencies and other targets. This directory
supplies reviewed notice material with explicit provenance categories.

`manifest.json` identifies each package name, version, declared license and exact
`.cargo_vcs_info.json` commit. Each notice records its source path,
commit-pinned source URL, byte size, Git blob identity and SHA-256. Files under
`blobs/` retain their source bytes. Some are reused from an existing local cache
only after their Git blob identity matches the exact upstream tree. They are not
generic replacements selected from an SPDX label. The package's declared
`license_file`, when present, remains required.

When a dependency changes, review its own published files and upstream provenance.
Update the reviewed records and original bytes together. Missing text, changed
source revisions or changed notice bytes make packaging fail. Never infer an
author's copyright statement or silently substitute another release's text.

`rust/inventory.json` in each artifact records package metadata and notice
provenance. Its scope is explicitly a metadata superset, not the linked dependency
closure. Existing native codec notices and the actual dav1d license remain
required separately.

## R134 dispatch exception

Registry dispatch 0.2.0 has a metadata-only exception, pinned to its Cargo.lock
checksum, MIT declaration, source revision and exact preserved metadata files.
Its published package supplies no original license/copyright notice. The artifact
includes verbatim Cargo.toml.orig/README.md and a clearly labeled supplement with
standard MIT permission, conditions and disclaimer. Steven Sheldon is identified
only as the declared author. No copyright holder or year is invented.

The supplement records its exact derivation from an existing local MIT body,
without that file's other authors' copyright lines. The inventory explicitly
labels the original notice as absent. This convention is not legal certification
or recovered-original evidence. Every other unknown omission still fails.
Notice packaging uses Python 3.11 or newer for Cargo.lock parsing.

## R135 egui release mapping and later simd terms

Exactly seven registry egui-family 0.37.0 packages have an unresolved published
VCS commit, `7c0b40402271e8d40cda69f87cf3c2617c2e4382`. Their MIT/Apache terms
use the reviewed official 0.37.0 release/sibling mapping. The annotated release
tag resolves to `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`; its root license blobs
match resolving same-version eframe/kittest package revisions. This is release
corroboration, not recovery of exact license files from the unresolved commit.
Registry, version/checksum, original declarations/authors and notice bytes are
pinned. Published manifests and README attribution are retained verbatim.

For simd_helpers 0.1.0, the published commit is
`ca1a2f84aa386d758e98f8a609d990263932fb85`, dated 2019-10-06. The direct-child
commit `82040194cd05affb060bf94d6f19f82a771d07fb`, dated 2019-12-07, adds only
LICENSE, blob `1b7c6aaa5106ef1459ed8c539c06d640e0e0a784`. These are later
upstream-supplied terms, not a file shipped in the release. Original attribution
is preserved. The catalog/inventory retain the dates, direct-parent relationship,
LICENSE-only tree change evidence and package checksum/declaration guards.

The categories are exact originals, official release/sibling mappings, later
upstream-supplied terms, and the R134 metadata-only standard-terms supplement.
None of the latter categories claims unavailable original release provenance.

## R141 libwebp-sys wrapper exception

Registry libwebp-sys 0.9.6 supplies no original wrapper license/copyright notice.
Its published manifest declares MIT. R141 retains its original manifest, README
and VCS metadata verbatim, plus clearly labeled supplemental standard MIT terms.
The two declared author strings are retained without inventing a copyright
holder/year. Registry, version/checksum, declaration, author strings and all five
original manifest/README/VCS/vendor notice identities are guarded.

The supplement is not a recovered upstream wrapper notice or legal certification.
Bundled libwebp's actual BSD `vendor/COPYING`, `vendor/AUTHORS` and other notices
remain separate and byte-identical. The collector requires package-level license
provenance independently of nested third-party terms and excludes implementation
files such as `copying.rs`. Unknown omissions still reject packaging.
