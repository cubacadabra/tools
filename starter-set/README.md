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
