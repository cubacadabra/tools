# Mockup character art study

One isolated five-part character: sculpted swept hair, purple hoodie, denim
shorts, white sneakers, and a soft-square base with a modeled smile. It does
not replace any of the 24 existing starter recipes.

These are **actual shared-engine captures**, not Blender beauty renders:

- [Front](review/front/mockup-person.png), [three-quarter](review/three-quarter/mockup-person.png), [back](review/back/mockup-person.png)
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

## Budget and current limits

Near totals 83,000 triangles, Mid 26,559, Far 5,690. Five 512px source atlases
bake color/fiber variation and local contact occlusion; the current compiler
resizes them to 256px runtime textures. The five v5 packs total about 7.62 MB.
Source normals survive compilation; runtime geometry and upload caps were not
increased. No mobile frame-rate or crowd-performance claim is made.

The hoodie has a lowered hollow hood, drawstrings/aglets, ribbing, and a
kangaroo pocket. Shorts have baked denim variation and modeled seams/hem.
Sneakers have a leather upper, sole, socks, laces, and panel seams. Hair uses
layered grooved locks, not painted hair on a flat thumbnail.

This is **not yet mockup fidelity or production-ready character art**. The
motion stress images expose single-joint sleeve weighting and joint-fit defects;
hair still has visible layered intersections and the pocket edge needs cleanup.
The modeled smile is static. Additional smooth skin weighting and fit work are
needed before replacing a starter. Normal/roughness texture maps, full PBR,
environment lighting, and real-time contact shadows are not implemented here.
Increasing LOD alone will not resolve those remaining differences.
