# Mockup character art study

One isolated five-part character: sculpted swept hair, purple hoodie, denim
shorts, white sneakers, and a soft-square base with a modeled smile. It does
not replace any of the 24 existing starter recipes.

These are **actual shared-engine captures**, not Blender beauty renders:

- [Fixed reference view](review/beauty/mockup-person.png), [frozen baseline](review/iterations/baseline/beauty/mockup-person.png)
- [Front](review/front/mockup-person.png), [three-quarter](review/three-quarter/mockup-person.png), [back](review/back/mockup-person.png)
- [Left](review/left/mockup-person.png), [right](review/right/mockup-person.png), [top](review/top/mockup-person.png)
- [Walk](review/walk/mockup-person.png), [side walk](review/walk-side/mockup-person.png)
- [Elbow stress](review/bend/mockup-person.png), [jump-like joint stress](review/jump-stress/mockup-person.png)
- [Mid](review/mid/mockup-person.png), [Far](review/far/mockup-person.png)

## Reproduce

From `tools/starter-set/`:

```sh
python3 review_study.py --generate --motion
```

Requires Blender and sibling `studio`/`rust` repositories. Omit `--generate` to
compile the checked-in GLBs and capture them without Blender. The command uses
the shared renderer without enabling Studio UI; it does not upload anything.
The generator saves an editable local `mockup-person.blend` (not versioned).
Source construction lives in `../../artwork/study_geometry.py`, materials in
`../../artwork/study_materials.py`, export in `../../artwork/build_study.py`.

The reference view asserts the exact base, four parts and palette, forces rest
pose/Near LOD, and fixes yaw at 0.26 radians, pitch at 0.12 and full-body framing.
Its blue-gray blocks and platform use existing opaque meshes/materials in a
**test-only** capture helper. This does not change the live editor camera/world.
There is no added shadow pass or generated-image substitute for the renderer.

Use `--label pass-name` to preserve an iteration without overwriting an earlier
one, or `--beauty-only` for a quick fixed-view check. `review/capture.json`
records the preset, source hashes, pack hashes, camera and diagnostic poses.
[The review ledger](review/iteration_review.json) includes rejected candidates
and unresolved visual failures, not just the best front image.

### Portable GLB and independent validation

Open [mockup-person.glb](mockup-person.glb) for the assembled Near character.
The five separate three-LOD source GLBs remain the engine's modular inputs.
The portable file contains five skinned parts and embedded textures, no stage
or animation clips. It is an inspection deliverable, not a new morph-pack input.

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --python-exit-code 1 --python artwork/validate_study_export.py
```

This starts from the exported GLBs, validates normals/UVs/textures/skin and
bounds, exports the combined character, then resets Blender and imports that
file again. Triangle counts and evaluated rest bounds must match. Six neutral
renders and a report land in [review/fresh-import](review/fresh-import/report.json).
Those use Blender lighting and source textures; they are **not runtime captures**.
Default glTF tint factors are linear values corresponding to the fixed sRGB
palette, while the engine continues to substitute its live palette.

## Budget and current limits

Near totals 82,924 triangles, Mid 26,534, Far 5,653. Five 512px source atlases
bake color/fiber variation and local contact occlusion; the current compiler
resizes them to 256px runtime textures. The five v5 packs total 7,752,905 bytes.
Source normals survive compilation; runtime geometry and upload caps were not
increased. No mobile frame-rate or crowd-performance claim is made.

The hoodie has a lowered hollow hood, drawstrings/aglets, ribbing, and a
kangaroo pocket. Shorts have baked denim variation and modeled seams/hem.
Sneakers have a leather upper, sole, socks, laces, and panel seams. Hair uses
layered grooved locks, not painted hair on a flat thumbnail.

This is **not yet mockup fidelity or production-ready character art**. The
motion stress images expose single-joint sleeve weighting and joint-fit defects;
hair still has overly regular/fused side and back locks and deep crown creases.
The modeled smile is static. Additional smooth skin weighting and fit work are
needed before replacing a starter. Normal/roughness texture maps, full PBR,
environment lighting, and real-time contact shadows are not implemented here.
Increasing LOD alone will not resolve those remaining differences.

Verification for this pass: 36 tools tests and 190 Rust workspace tests passed;
13 explicit GPU capture cases passed pack admission/normal assertions. Studio
and non-Studio reference PNGs are byte-identical, including a run with conflicting
external camera/pose settings. WASM/web-renderer and host Studio/Android-backend
build checks passed. These are not on-device mobile performance measurements.

## Workflow review

Applied the modeling, character-fit, rendering, multiview validation and
targeted-refinement guidance from
[Blender Agent Studio](https://github.com/ifBars/blender-agent-studio).
Its useful contribution here was a repeatable review loop, not an automatic
quality guarantee. No global plugin, MCP server, dependency or custom skill was
installed. The existing local Blender/Python pipeline is sufficient to execute it.

The proposed RenderDoc workflow is not applicable to this Mac/Metal capture:
[RenderDoc supports other graphics APIs, not Metal](https://github.com/baldurk/renderdoc#api-support).
It remains a possible diagnostic tool for a later Vulkan/Android discrepancy.
