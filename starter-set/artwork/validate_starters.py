"""Independently fresh-import every delivered GLB, not the authoring scene.

Blender --background --factory-startup --python-exit-code 1 --python this-file
"""
import hashlib
import json
import math
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[1]


def main():
    report = {"blender": bpy.app.version_string, "assets": []}
    for path in sorted((ROOT / "source/morphs").glob("*/*/*.glb")):
        bpy.ops.wm.read_factory_settings(use_empty=True)
        result = bpy.ops.import_scene.gltf(
            filepath=str(path), import_shading="NORMALS", merge_vertices=False,
            bone_heuristic="BLENDER", guess_original_bind_pose=False, disable_bone_shape=True)
        assert result == {'FINISHED'}, (path, result)
        manifest = json.loads(path.with_suffix('.morph.json').read_text())
        meshes = [o for o in bpy.data.objects if o.type == 'MESH']
        assert {o.name for o in meshes} == {'near', 'mid', 'far'}, path
        skinned = path.parent.parent.name in ('base', 'top', 'bottom', 'footwear')
        asset = {'file': str(path.relative_to(ROOT)),
                 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'lods': {}}
        for obj in meshes:
            assert obj.data.has_custom_normals, (path, obj.name, 'normals')
            assert obj.data.uv_layers.active, (path, obj.name, 'UVs')
            assert bool(obj.vertex_groups) == skinned, (path, 'skinning')
            if skinned:
                assert len(obj.vertex_groups) == 15, path
                assert all(v.groups and abs(sum(g.weight for g in v.groups)-1) < 1e-4
                           for v in obj.data.vertices), (path, 'weights')
            assert all(math.isfinite(c) for uv in obj.data.uv_layers.active.data for c in uv.uv), path
            assert all(math.isfinite(c) for n in obj.data.corner_normals for c in n.vector), path
            evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
            mesh = evaluated.to_mesh(); mesh.calc_loop_triangles()
            assert len(mesh.loop_triangles) == manifest['asset']['lod'][obj.name], path
            points = [evaluated.matrix_world @ v.co for v in mesh.vertices]
            assert points and all(math.isfinite(c) for p in points for c in p), path
            bounds = [[min(p[i] for p in points) for i in range(3)],
                      [max(p[i] for p in points) for i in range(3)]]
            assert all(b-a < 3.6 for a,b in zip(*bounds)), (path, bounds)
            for mat in obj.data.materials:
                textures = [n.image for n in mat.node_tree.nodes if n.type == 'TEX_IMAGE']
                assert textures and all(i and tuple(i.size) == (512,512) for i in textures), (path,mat.name)
            asset['lods'][obj.name] = {'triangles': len(mesh.loop_triangles),
                'vertices': len(mesh.vertices), 'boundsZUp': bounds,
                'authoredNormals': True, 'uvs': True, 'embeddedTexture': True, 'skinned': skinned}
            evaluated.to_mesh_clear()
        report['assets'].append(asset)
    assert len(report['assets']) == 20
    output = ROOT / 'review/fresh-import-report.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + '\n')
    print('Validated 20 independently imported GLBs / 60 LODs:', output)


if __name__ == '__main__':
    main()
