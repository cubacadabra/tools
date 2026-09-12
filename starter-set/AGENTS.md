# Starter-set character artwork

These instructions apply to this directory and its descendants. For a
documentation-only task, do not regenerate assets or launch rendering jobs.

## Skills to use

For morph/character art, use the relevant **Blender Agent Studio** workflows.
The [README skill guide](README.md#character-art-skills) links their upstream
`SKILL.md` files. Choose the smallest relevant set and read each selected skill
and its required references before making asset changes:

- `blender-iterative-refinement`: establish a baseline and requirement ledger;
  make a targeted change and compare with identical capture settings.
- `blender-modeling-workflow`: silhouette, proportions, secondary forms,
  topology, smoothing, garment construction and readable detail.
- `blender-character-workflow`: rigging, weights, garment/body fit, hands,
  footwear and deformation at shoulders, elbows, hips and knees.
- `blender-rendering-workflow`: diagnose camera, framing, lighting, contact
  shadows and material scale separately from geometry.
- `blender-asset-validation`: inspect hero/front/back/left/right/top views and
  fresh-import the exported GLB; validate the actual runtime artifact too.

These skills are recommendations, not a claim that they are installed. If
unavailable locally, consult the linked upstream guidance and use the existing
Blender/Python tooling. If guidance cannot be read, report that limitation and
follow the local rules below. Do not silently install plugins, MCP servers,
skills or dependencies. No new server is required for the existing workflow.

The CLI-Anything RenderDoc skill is only for investigating GPU captures on
supported APIs, such as Vulkan/Android. RenderDoc is not a Metal/macOS capture
tool. Use the shared-engine capture path for this Mac; do not make RenderDoc a
prerequisite for art work. Raster generation is not a substitute for geometry
or evidence of runtime rendering quality.

## Mac GPU debugging

This repository's macOS renderer is `wgpu` over Metal. Use Xcode's Metal GPU
frame capture and shader debugger, not RenderDoc. The recommended setup is an
Xcode **External Build System** project that runs `cargo`, with the Studio
executable selected as the run target. Reproduce the fixed runtime capture,
then inspect the hair/hoodie draw calls, vertex buffers, authored normals, atlas
textures, bind groups, pipeline state and render target in Xcode.

For this checkout, use `build --manifest-path ../../studio/Cargo.toml --bin
studio` as the external build arguments and select
`../../studio/target/debug/studio` as the executable. Use a debug build so
Metal validation and GPU capture are available.

For a development-only “capture next GPU frame” command, wgpu 29 provides:

```rust
unsafe { device.start_graphics_debugger_capture(); }
// record commands, submit the queue, then wait for the frame to complete
unsafe { device.stop_graphics_debugger_capture(); }
```

The calls must enclose both command recording and queue submission. Do not
enable them in release rendering. For Metal API validation, launch from Xcode
or set `METAL_DEVICE_WRAPPER_TYPE=1` in a debug environment. Apple's optional
`gpudebug` utility can inspect a `.gputrace` when installed; verify with
`xcrun --find gpudebug` rather than assuming it exists. Links and the exact
platform matrix are in [README.md](README.md#mac-gpu-debugging).

## Art iteration contract

1. Inspect existing sources, the supplied reference, and any study README and
   review ledger before editing. Label assumptions about unseen views.
2. Keep procedural changes in the durable `artwork/` Python source. The shared
   `artwork/material_textures.py` generator owns the small deterministic color
   atlases used by every starter GLB; do not hand-edit generated atlas PNGs.
   Editing
   only a generated GLB or local `.blend` loses the change on regeneration.
   Preserve unrelated user work and check the generator's overwrite scope.
3. Freeze an actual runtime baseline: exact preset, equipment, palette, pose,
   camera, lighting, resolution and LOD. For the mockup study use
   `cuba:preset/mockup-person.v1`, with its four parts and no glasses/extras.
4. Evaluate silhouette and fit first: head/body ratio, hair crown/part/clumps,
   shoulders, sleeve taper, waist, hood/pocket fit, shorts, hands and shoes.
   Inspect presentation and material frequency before adding geometry.
5. Make one causal change or tightly related repair group per iteration.
   Regenerate, compare before/after at identical settings, inspect other views,
   and retain changes only when the targeted defect improves without an
   unaddressed regression. Record rejected candidates and remaining failures.
6. Keep existing delivery budgets and morph/rendering architecture fixed for
   reference-matching art passes unless a concrete defect demonstrates that a
   limit or missing capability is the cause. Triangle ceilings are not targets.
   Do not add LOD levels, normal maps or a new shading system speculatively.
7. Preserve authored corner normals, UVs, material/tint semantics and skinning
   through Blender → GLB → MorphPack → renderer. We are pre-launch: use the
   required current pack schema; do not add legacy-normal reconstruction or
   old-schema compatibility without an explicit requirement.
8. Asset capabilities must mean the same thing in Studio, web, iOS and Android.
   Never gate asset interpretation behind `studio-ui`. Test-only presentation
   helpers must be identified as such; they do not change the live editor.
9. Fresh-import exported GLBs in a clean Blender process/scene. Check bounds,
   triangle counts, normals, UVs, embedded materials/textures and skinning.
   Inspect neutral and hero views, not only the generator's `.blend`.
10. Validate the composed character in the shared renderer, including Near,
    Mid, Far, walk, side/turn views and elbow/knee/jump stress. Stress poses do
    not substitute for gameplay animation checks. Do not hide deformation
    failures behind a good idle image or a passing numerical test.
11. Distinguish Blender renders from runtime captures. Do not paint fake
    directional lighting into textures to conceal runtime defects. Deliberate
    color/fabric variation and documented nondirectional AO are allowed.
12. Report evidence paths, verification results and unresolved visual failures.
    Do not call an asset mockup-quality or production-ready while its review
    gates still fail. Do not publish/upload or replace existing starters unless
    that action is within the user's requested scope.

## Local workflow and boundaries

Read [the study guide](studies/mockup-person/README.md) and
[its review ledger](studies/mockup-person/review/iteration_review.json) when
continuing the single-character study. They document capture commands, failed
gates and the distinction between modular source GLBs and the assembled
inspection GLB. Use unique `review_study.py --label` names to preserve evidence.

From this directory, the engine is `../../rust` and the compiler/Studio project
is `../../studio`. Read each repository's own instructions before editing it.
Prefer existing generators, validators and capture tools; no automatic plugin
installation or new custom skill is needed to use this workflow.
