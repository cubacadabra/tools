# Cubacadabra morph starter set

This directory is the recovered local development fixture set for the shared
morph library. It was recovered from the local Wrangler R2 `prod` bucket on
2026-09-11 and from the matching authoring files still present in
`/Users/aa/Downloads`.

`catalog.json` and `source/` are the editable source of truth. Generated packs
are written to `.cubacadabra/generated/morphs/` and are intentionally ignored
by Git. The release lock file records their hashes and is consumed by the D1
publication step.

The source files are for authoring and validation. Runtime clients consume the
compiled `.morphpack` files only. Run `cubacadabra setup-local` to compile the
source, upload immutable runtime objects into local R2, and install the current
catalog release into local D1.

The current source set contains nine GLB/manifest pairs and the logo image.

## Build and publish

The existing Rust authoring compiler accepts `.morph.json + .glb` and emits a
`.morphpack`. The Python CLI orchestrates it:

```text
cubacadabra morph build
cubacadabra setup-local
cubacadabra morph publish
```

`morph build` writes compiled packs and `catalog.lock.json` below the ignored
`.cubacadabra/generated/morphs/` directory. `setup-local` uploads those packs
and installs the catalog into local R2/D1. `morph publish` uploads the same
immutable hash-addressed objects to production and updates the catalog only
after all uploads succeed.
