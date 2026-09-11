# Cubacadabra starter morphs

This is the editable source of truth for Studio's 24 starter people and their
reusable parts. A starter is an appearance recipe, not a new mesh or a kind of
person. Choosing it replaces the complete appearance; every part remains
editable afterward. Changing one recipe never changes another recipe.

## Where to edit

- `catalog.json`: component manifests, starter ordering, and builtin exclusions.
- `presets/person-01.json` through `person-24.json`: complete appearance recipes.
- `source/morphs/`: editable GLB geometry and `.morph.json` manifests.
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

`python3 starter-set/generate_parts.py` regenerates the initial hair, glasses,
and hearing-device GLBs/manifests and the two tintable base GLBs. It does not
rewrite the curated recipes. Do not run it over hand-edited generated parts
unless replacing those edits is intended.

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

## Content coverage and limits

The first release has 20 authored components plus 24 builtin hair/expression
components and 24 recipes. It includes 12 skin tones, seven new hair meshes,
two glasses styles, and hearing aids. These are initial stylized assets, not
a claim to represent every child or an art-complete diversity roster.

The two bodies currently share a build. More body shapes, culturally varied
hair/clothing, mobility devices, prostheses, and seated rigs still need proper
art and animation work. Do not represent wheelchair support as a cosmetic hat
slot: it needs its own fit, pose, locomotion, and interaction support.

Authored hair and hearing devices advertise `hair.authored.v1` and
`accessory.ear-device.v1`. Their rendering support is currently Studio-gated;
mobile and web clients must implement and advertise those capabilities before
using these complete presets. Their existing default renderer paths are
unchanged.
