# Cubacadabra starter morphs

The runtime now requires [morph pack v5](../../rust/docs/morph-pack-v5.md)
with authored normals; rebuild old local releases before use. The reusable
starter parts now use the same presentation-oriented export contract as the
mockup study: authored normals and UVs, embedded color atlases, and explicit
Near/Mid/Far delivery meshes. The [single-character mockup study](studies/mockup-person/README.md)
remains the visual reference for future art passes.

This is the editable source of truth for Studio's 24 starter people and their
reusable parts. A starter is an appearance recipe, not a new mesh or a kind of
person. Choosing it replaces the complete appearance; every part remains
editable afterward. Changing one recipe never changes another recipe.

## Where to edit

- `catalog.json`: component manifests, starter ordering, and builtin exclusions.
- `presets/person-01.json` through `person-24.json`: complete appearance recipes.
- `source/morphs/`: editable GLB geometry and `.morph.json` manifests. The GLB
  files contain geometry, authored normals, UVs, embedded materials/textures,
  and three LODs needed by the build and runtime; Blender `.blend` authoring
  files are intentionally not committed.
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
Bodies and clothing have welded shading seams, modeled secondary construction,
embedded color atlases, and separate delivery LODs. Polo fabric follows the
preset's `primary` color while keeping its logo/trim.

Run from `tools/`:

```sh
python3 starter-set/generate_parts.py --wardrobe --blend
```

This rebuilds all 20 GLBs without changing the 24 recipes. Blender is required
for every final asset's unwrap and color/AO bake; it is an authoring dependency
only, not a game/runtime dependency. To regenerate one hairstyle:

```sh
python3 starter-set/generate_parts.py --only floppy
```

`--wardrobe` also refreshes bodies and clothing; omit it to work only on
hair/accessories. `artwork/build_starters.py` is the final delivery stage: it
uses the study hoodie, shorts, sneakers, hands and swept-hair construction,
fitted polo/slacks, and the established interactive head shapes. All garments
use the existing 15-joint rig with blended shoulder/elbow/knee weights.
**Regeneration replaces
the selected source files**, including manual edits. Generate local `.blend` files
with `--blend` when hand-editing the model. Keep all three LOD node names, the
head-local origin (for rigid wearables), and the existing rig (for skinned
clothing). Export GLB with hidden LODs included and Custom Properties enabled so
material avatar-tint flags survive. Final 512px color/AO atlases are baked onto
the reduced Near mesh, embedded in the GLBs, and saved for inspection under
`review/bakes/textures/`. Materials are authored in `artwork/study_materials.py`;
the small `assets/starter-*.png` tiles are staging inputs only. Never hand-edit
generated textures or GLBs as a durable fix. Sidecar counts are refreshed by
the exporter. The checked-in GLBs are the compiler's source assets.

From `starter-set/`, independently validate all exported GLBs and capture the
full catalog with identical hero/front/back/side/top, LOD and motion settings:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --python-exit-code 1 --python artwork/validate_starters.py
python3 review_starters.py --label unique-review-name
```

The review script uses the shared runtime renderer; Pillow only assembles
contact sheets. It refuses to overwrite an existing review label. Read
`review/iteration_review.json` for accepted improvements and unresolved gates.
Static bend/jump stress captures do not establish full gameplay animation quality.

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
roster. Near ceilings are 17,000 triangles for hair, 16,000 for bodies, 20,000
for tops/footwear, and 9,000 for bottoms. Mid/Far reductions are 32%/7%; small
accessories stay below their separate 8,000–13,000 Near ceilings. The shared
renderer registry has a 64 MiB residency ceiling so the entire baked wardrobe
can be registered; per-frame skinning and instance limits are unchanged.
Authored normals, UVs, embedded color atlases, and material
interpretation are shared across renderer targets.

The two bodies share limb construction but retain distinct head profiles. More body shapes, culturally varied
hair/clothing, mobility devices, prostheses, and seated rigs still need proper
art and animation work. Do not represent wheelchair support as a cosmetic hat
slot: it needs its own fit, pose, locomotion, and interaction support.

Authored hair and hearing devices advertise `hair.authored.v1` and
`accessory.ear-device.v1`. Client catalog integration must advertise the
capabilities it actually supports before selecting these complete presets.
Both starter bases now require shared canonical-rest semantics. The isolated
study additionally uses a static authored face; the reusable starters retain
their interactive expression choices.

## Character-art skills

For Blender work on the isolated study or starter artwork, these are
recommended workflows from [Blender Agent Studio](https://github.com/ifBars/blender-agent-studio).
They are recommendations, not assumed-installed skills; the existing local
Blender/Python setup is sufficient. Do not auto-install a plugin, MCP server,
or dependency. The recommendation names use a `blender-` prefix; the links
point to the upstream skill files:

- [blender-iterative-refinement](https://github.com/ifBars/blender-agent-studio/blob/main/plugins/blender-agent-studio/skills/blender-iterative-refinement/SKILL.md): freeze a baseline and review ledger, make one causal edit, and compare with the same settings.
- [blender-modeling-workflow](https://github.com/ifBars/blender-agent-studio/blob/main/plugins/blender-agent-studio/skills/blender-modeling-workflow/SKILL.md): solve silhouette, secondary forms, and fit; treat budgets as ceilings, not targets.
- [blender-character-workflow](https://github.com/ifBars/blender-agent-studio/blob/main/plugins/blender-agent-studio/skills/blender-character-workflow/SKILL.md): check garment fit and skinning/joint stress, including motion.
- [blender-rendering-workflow](https://github.com/ifBars/blender-agent-studio/blob/main/plugins/blender-agent-studio/skills/blender-rendering-workflow/SKILL.md): diagnose camera, light, material, and scale before changing geometry.
- [blender-asset-validation](https://github.com/ifBars/blender-agent-studio/blob/main/plugins/blender-agent-studio/skills/blender-asset-validation/SKILL.md): review hero plus front/back/left/right/top views, perform a fresh GLB import, and accept the actual runtime result.

See this directory's [AGENTS.md](AGENTS.md), the [mockup-person study
README](studies/mockup-person/README.md), and its [iteration review
ledger](studies/mockup-person/review/iteration_review.json). From this
directory, useful checkpoints are:

```sh
python3 review_study.py --beauty-only --label baseline-review
python3 review_study.py --generate --motion --label hair-pass-01
Blender --background --factory-startup --python-exit-code 1 \
  --python artwork/validate_study_export.py
```

`--generate` overwrites isolated study source files, so preserve a baseline
and use a unique label for each pass. The optional
[RenderDoc workflow](https://github.com/HKUDS/CLI-Anything/blob/main/skills/cli-anything-renderdoc/SKILL.md)
is only for `.rdc` GPU diagnosis on supported APIs such as Vulkan/Android;
it does not apply to Metal/macOS. See [RenderDoc API
support](https://github.com/baldurk/renderdoc#api-support).

Prefer shared-engine runtime captures over Blender beauty renders when judging
whether an asset is ready.

## Mac GPU debugging

This engine uses `wgpu` with the Metal backend on macOS. RenderDoc is not the
Mac debugger: its capture/replay backends do not support Metal. Use Xcode's
Metal GPU capture and shader debugger instead. The [wgpu debugging guide](https://wgpu.rs/doc/wgpu/documentation/debugging/debugging_applications/index.html)
recommends launching Rust programs from an Xcode **External Build System**
project and selecting the built executable as the run target.

For this repository, configure the external build tool with `cargo`, use
`build --manifest-path ../../studio/Cargo.toml --bin studio` as its arguments,
and select the resulting `../../studio/target/debug/studio` executable. Run a
debug build from Xcode so Metal validation and GPU capture are available.

For a useful frame, run the Studio executable from Xcode, select **Debug >
Capture GPU Frame** (or the Metal capture button), and reproduce the fixed
mockup-person view. Inspect the hoodie/hair draw calls, vertex buffers,
authored normals, atlas texture, bind groups, pipeline state and final render
target. Xcode can debug the translated WGSL/Metal shader and individual pixels.

For validation without the full Xcode UI, wgpu 29 exposes the native capture
hook on `wgpu::Device`:

```rust
unsafe { device.start_graphics_debugger_capture(); }
// Record commands, submit the queue, and wait for completion for the frame.
unsafe { device.stop_graphics_debugger_capture(); }
```

The capture must surround command recording **and** submission. Put this behind
a development-only command such as “Capture next GPU frame”; never enable it in
normal release rendering. If Metal validation diagnostics are needed, launch
from Xcode or set `METAL_DEVICE_WRAPPER_TYPE=1` for a debug run. An optional
Apple `gpudebug` command-line tool may be useful for text inspection of a
`.gputrace`, but check `xcrun --find gpudebug` first—this local Xcode install
does not currently provide that utility. See Apple's [Metal debugger](https://developer.apple.com/documentation/xcode/metal-debugger)
and [programmatic Metal capture](https://developer.apple.com/documentation/xcode/capturing-a-metal-workload-programmatically)
documentation.

Use RenderDoc only for a Linux/Windows Vulkan/OpenGL/D3D capture, or a separate
Android Vulkan investigation. A MoltenVK portability layer does not make
RenderDoc a supported macOS/Metal debugger.
