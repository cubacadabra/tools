# Cubacadabra starter morphs

This is the editable source of truth for Studio's 24 starter people and their
reusable parts. A starter is an appearance recipe, not a new mesh or a kind of
person. Choosing it replaces the complete appearance; every part remains
editable afterward. Changing one recipe never changes another recipe.

## Where to edit

- `catalog.json`: component manifests, starter ordering, and builtin exclusions.
- `presets/person-01.json` through `person-24.json`: complete appearance recipes.
- `source/morphs/`: editable GLB geometry and `.morph.json` manifests. The GLB
  files contain the geometry, materials, and three LODs needed by the build and
  runtime; Blender `.blend` authoring files are intentionally not committed.
- `artwork/`: authored curves, wearable construction, and curly-hair sculpting.
- `presets/thumbnails/`: full-character previews rendered by the engine.

For example, `presets/person-17.json` combines the Broad Jaw base, Floppy Hair,
Short Sleeve Collared Shirt, Slacks, and Sparkles shoes. Its `parameters` hold
skin and clothing colors; `face` selects an expression. Change those references
to give Person 17 a new look without manufacturing another full-body asset.

`base` is now strictly the underlying body/facial geometry. It is not a gender,
skin tone, or fully dressed starter. Clothing, hair, expressions, glasses,
headphones, hats, and hearing devices are independent choices. The two current
bases are labeled Soft Jaw and Broad Jaw in Studio, not Person 1 and Person 2.

## Selection and layering

Studio opens with **Starters**: 24 complete preview tiles. **Customize** exposes
the selected person's components and 12 skin swatches. Optional parts have a
None choice and can be toggled off. Required clothing is replaced, not removed.
Returning to Starters doesn't erase edits; choosing another starter does.

Browsing categories don't decide exclusivity. Component `occupiedSlots` and
explicit `conflicts` do. A new item replaces items occupying or conflicting
with its slots. The starter set uses independent `hair`, `headwear`,
`ear-accessory`, `facewear`, and `ear-device` slots, allowing hair + hat +
headphones + glasses + hearing aids. Two pairs of glasses replace each other.
Actual geometry still needs to be checked for clipping when adding new parts.

Legacy builtin `outfit` assets are excluded from this release and hidden from
the Studio library. A preset is not an outfit asset. The engine's old bundled
catalog is retained for other clients, but is not merged into a published
Studio catalog.

## Build and local publication

Run these from `tools/` (or use the installed `cubacadabra` command):

```sh
PYTHONPATH=src python3 -m cubacadabra morph build
PYTHONPATH=src python3 -m cubacadabra setup-local
```

The build compiles the parts and validates every preset with the shared Rust
resolver. It writes immutable packs, PNG previews, and `catalog.lock.json`
under the ignored `.cubacadabra/generated/morphs/` directory. Failed validation
does not replace the previous lock file. `setup-local` uploads to local R2 and
installs the release in local D1; no production upload is necessary.

Studio fetches the complete paginated catalog and verifies downloaded pack
sizes and hashes. A starter is displayed only after all its parts are ready.
A failed or superseded download does not replace the current appearance.

The separate `morph publish` command uploads to **production**. Only use it
when a production release is intended.

## Regenerating source and previews

The artwork is real source geometry, not painted thumbnails. Floppy hair uses
layered tapered curves; curls/coils are voxel-unioned, relaxed sculpted meshes;
glasses have continuous rims; headphones have a padded arch and fitted cups.
Bodies and clothing have welded shading seams and separate delivery LODs.
Polo fabric follows the preset's `primary` color while keeping its logo/trim.

Run from `tools/`:

```sh
python3 starter-set/generate_parts.py --wardrobe --blend
```

This rebuilds all 20 GLBs without changing the 24 recipes. It requires Blender
for sculpted curls and optional local `.blend` output; Blender is an authoring
dependency only, not a game/runtime dependency. Other hair and accessories can
be regenerated without it, for example:

```sh
python3 starter-set/generate_parts.py --only floppy
```

`--wardrobe` also refreshes bodies and clothing via the canonical rig tools in
`studio/tools/`; omit it to work only on hair/accessories. **Regeneration replaces
the selected source files**, including manual edits. Generate local `.blend` files
with `--blend` when hand-editing the model. Keep all three LOD node names, the
head-local origin (for rigid wearables), and the existing rig (for skinned
clothing). Export GLB with hidden LODs included and Custom Properties enabled so
material avatar-tint flags survive. Update sidecar triangle counts when geometry
changes. The checked-in GLBs remain the source assets.

After changing geometry or a recipe, build, render the actual composed people,
then build/publish locally again so the new preview hashes enter the release:

```sh
PYTHONPATH=src python3 -m cubacadabra morph build
CUBA_STARTER_CATALOG="$PWD/.cubacadabra/generated/morphs/catalog.lock.json" \
CUBA_STARTER_THUMBNAILS="$PWD/starter-set/presets/thumbnails" \
cargo test --manifest-path ../rust/Cargo.toml --features studio-ui capture_starters -- --ignored
PYTHONPATH=src python3 -m cubacadabra setup-local
```

The preview capture requires a GPU but not an unlocked desktop. Studio also
has a GPU-backed library layout/interaction smoke test:

```sh
cargo test --manifest-path ../studio/Cargo.toml morph_library_layout_and_interactions -- --ignored
```

For close-up artwork review, set `CUBA_STARTER_PORTRAIT=1` and use a temporary
output directory. `CUBA_STARTER_YAW=1.2` gives a side view; `3.14159` gives the
back. These use the actual game geometry/material pipelines, not a separate
Blender beauty render. Leave those variables unset when regenerating thumbnails.

Geometry/fit checks run without Blender:

```sh
PYTHONPATH=src python3 -m unittest discover -s tests
```

## Content coverage and limits

The first release has 20 authored components plus 24 builtin hair/expression
components and 24 recipes. It includes 12 skin tones, seven new hair meshes,
two glasses styles, and hearing aids. The artwork uses a consistent rounded toy
style; it is not a claim to represent every child or an art-complete diversity
roster. Presentation-detail hair/accessory LOD budgets stay below
17,000/7,000/2,000 triangles; the skinned body remains bounded below
30,000/8,000/1,400. Hair's softer Studio material is feature-gated so it
doesn't change other clients' default shading.

The two bodies currently share a build. More body shapes, culturally varied
hair/clothing, mobility devices, prostheses, and seated rigs still need proper
art and animation work. Do not represent wheelchair support as a cosmetic hat
slot: it needs its own fit, pose, locomotion, and interaction support.

Authored hair and hearing devices advertise `hair.authored.v1` and
`accessory.ear-device.v1`. Their rendering support is currently Studio-gated;
mobile and web clients must implement and advertise those capabilities before
using these complete presets. Their existing default renderer paths are
unchanged.
