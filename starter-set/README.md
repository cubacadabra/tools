# Cubacadabra morph starter set

This directory is the recovered local development fixture set for the shared
morph library. It was recovered from the local Wrangler R2 `prod` bucket on
2026-09-11 and from the matching authoring files still present in
`/Users/aa/Downloads`.

The runtime directory contains every non-empty morph pack currently present in
local R2, including older immutable pack versions retained by migration
history. The filename and two-character directory prefix are the SHA-256
content address and must not be changed. The source directory contains the
nine GLB/manifest pairs currently available on this machine and the logo image
used by the polo material.

The source files are for authoring and validation. Runtime clients consume the
compiled `.morphpack` files only. A later bootstrap command can upload the
runtime and source objects into a fresh local R2 instance and apply the D1
catalog migrations; this step intentionally does not implement that command.

All 27 runtime packs, all 9 GLB/manifest pairs, and the logo image passed
content and schema validation after recovery.

## a

Yes—the CLI can generate them, and the existing Rust compiler already does this:

  crates/morph_authoring/src/bin/morph_compile.rs accepts:

  .morph.json + .glb → .morphpack

  I rebuilt all nine current assets from starter-set/source and reproduced their exact committed hashes and byte sizes.

  Recommended long-term structure:

  - Source of truth: .glb, .morph.json, and image assets.
  - Generated artifacts: .morphpack and thumbnails.
  - Keep generated packs checked into starter-set as verified golden fixtures for offline setup and exact migration
    compatibility.

  - Never hand-edit packs. Regenerate and verify them.
  - Production R2 remains the delivery location.
  - Keep historical packs because migrations and old catalog versions reference immutable hashes.

  The Python CLI should orchestrate the Rust compiler rather than reimplement the binary format. Eventually add:

  cubacadabra morph build
  cubacadabra morph verify
  cubacadabra setup-local

  morph verify should regenerate packs in a temporary directory and confirm their hashes match the checked-in runtime
  fixtures. If a pack changes, it should require a new content-addressed R2 object and backend migration.

  So: morphpacks are technically reproducible, but I recommend keeping them committed in starter-set as generated,
  validated release fixtures.
