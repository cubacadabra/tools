"""Fresh-process GLB import, geometry checks and neutral multiview renders.

Run Blender --background --factory-startup --python this-file.py.
This deliberately never opens the generator's .blend. Engine captures remain
the visual acceptance evidence; these views diagnose the exported geometry.
"""
import hashlib
import json
import math
from pathlib import Path

import bpy
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[1]
STUDY=ROOT/'studies/mockup-person'
OUT=STUDY/'review/fresh-import'
BUDGETS={'base':16000,'hair':18000,'top':20000,'bottom':9000,'footwear':20000}


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    OUT.mkdir(parents=True,exist_ok=True)
    meshes=[]; armatures=[]; report={'blender':bpy.app.version_string,'assets':[]}
    for path in sorted((STUDY/'source/morphs').glob('*/*/*.glb')):
        before=set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(path),import_shading='NORMALS',merge_vertices=False,
                                 bone_heuristic='BLENDER',guess_original_bind_pose=False)
        imported=set(bpy.data.objects)-before
        near=[o for o in imported if o.type=='MESH' and o.name.split('.')[0]=='near']
        assert len(near)==1, (path, [o.name for o in imported])
        for obj in imported:
            if obj.type=='MESH' and obj not in near:
                obj.hide_render=True; obj.hide_set(True)
        obj=near[0]; obj.name=path.stem; meshes.append(obj)
        armatures.extend(o for o in imported if o.type=='ARMATURE')
        assert obj.data.uv_layers.active is not None, path
        assert len(obj.vertex_groups)==15, path
        assert obj.data.has_custom_normals, f'{path}: authored normals lost at import'
        evaluated=obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
        mesh=evaluated.to_mesh()
        mesh.calc_loop_triangles()
        points=[evaluated.matrix_world @ v.co for v in mesh.vertices]
        assert all(math.isfinite(c) for p in points for c in p), path
        bounds=[[min(p[i] for p in points) for i in range(3)],
                [max(p[i] for p in points) for i in range(3)]]
        assert len(mesh.loop_triangles)<=BUDGETS[path.parent.parent.name], path
        assert bounds[0][2]>=0 and bounds[1][2]<3.6, (path,bounds)
        for mat in obj.data.materials:
            textures=[n.image for n in mat.node_tree.nodes if n.type=='TEX_IMAGE']
            assert textures and all(i and i.size[0]==512 for i in textures), (path,mat.name)
        report['assets'].append({'file':str(path.relative_to(STUDY)),
            'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
            'triangles':len(mesh.loop_triangles),'vertices':len(mesh.vertices),
            'boundsZUp':bounds,'authoredNormals':True,'uvs':True,'embeddedTextures':True})
        evaluated.to_mesh_clear()

    # A convenient portable Near character, independent of the generator's
    # joint-local source convention. This is not fed back into the morph pack.
    bpy.ops.object.select_all(action='DESELECT')
    for obj in meshes+armatures: obj.select_set(True)
    bpy.context.view_layer.objects.active=meshes[0]
    bpy.ops.export_scene.gltf(filepath=str(STUDY/'mockup-person.glb'),export_format='GLB',
                             use_selection=True,export_animations=False,export_skins=True)

    # Render the portable deliverable after a second fresh scene/import, not
    # the pre-export Blender objects. Assert its evaluated rest geometry too.
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(STUDY/'mockup-person.glb'),import_shading='NORMALS',
                             guess_original_bind_pose=False,disable_bone_shape=True)
    imported=[o for o in bpy.data.objects if o.type=='MESH']
    assert len(imported)==5, [o.name for o in imported]
    for obj in imported:
        expected=next(a for a in report['assets'] if Path(a['file']).stem==obj.name)
        evaluated=obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
        mesh=evaluated.to_mesh(); mesh.calc_loop_triangles()
        assert len(mesh.loop_triangles)==expected['triangles'], obj.name
        points=[evaluated.matrix_world @ v.co for v in mesh.vertices]
        bounds=[[min(p[i] for p in points) for i in range(3)],
                [max(p[i] for p in points) for i in range(3)]]
        assert all(abs(a-b)<1e-4 for x,y in zip(bounds,expected['boundsZUp']) for a,b in zip(x,y)), obj.name
        assert obj.data.has_custom_normals and obj.data.uv_layers.active, obj.name
        evaluated.to_mesh_clear()
    report['portableGlb']={'file':'mockup-person.glb',
        'sha256':hashlib.sha256((STUDY/'mockup-person.glb').read_bytes()).hexdigest(),
        'freshReimport':'five parts, triangle counts, evaluated bounds, normals and UVs match'}

    scene=bpy.context.scene
    scene.render.engine='CYCLES'; scene.cycles.samples=24; scene.cycles.seed=0
    scene.cycles.use_denoising=True
    scene.render.resolution_x=512; scene.render.resolution_y=640; scene.render.resolution_percentage=100
    scene.render.image_settings.file_format='PNG'
    scene.view_settings.view_transform='Standard'; scene.view_settings.look='None'
    scene.view_settings.exposure=0; scene.view_settings.gamma=1
    world=bpy.data.worlds.new('Neutral diagnostic world'); world.use_nodes=True
    world.node_tree.nodes['Background'].inputs['Color'].default_value=(.13,.16,.21,1)
    world.node_tree.nodes['Background'].inputs['Strength'].default_value=.45
    scene.world=world
    for name,location,energy,size in [('key',(-3,-4,6),420,4),('fill',(4,-2,3),180,3),('rim',(1,3,5),300,3)]:
        light=bpy.data.lights.new(name,'AREA'); light.energy=energy; light.shape='DISK'; light.size=size
        obj=bpy.data.objects.new(name,light); scene.collection.objects.link(obj); obj.location=location
        obj.rotation_euler=(Vector((0,0,1.7))-obj.location).to_track_quat('-Z','Y').to_euler()
    camera=bpy.data.objects.new('Fixed diagnostic camera',bpy.data.cameras.new('Camera'))
    scene.collection.objects.link(camera); scene.camera=camera; camera.data.type='ORTHO'; camera.data.ortho_scale=4.56
    for name,yaw,pitch in [('hero',.26,.12),('front',0,0),('back',math.pi,0),
                           ('left',math.pi/2,0),('right',-math.pi/2,0),('top',0,1.55)]:
        # Engine -Z front becomes Blender +Y after the glTF import conversion.
        target=Vector((0,0,1.62))
        camera.location=target+Vector((7*math.sin(yaw)*math.cos(pitch),7*math.cos(yaw)*math.cos(pitch),7*math.sin(pitch)))
        camera.rotation_euler=(target-camera.location).to_track_quat('-Z','Y').to_euler()
        scene.render.filepath=str(OUT/f'{name}.png'); bpy.ops.render.render(write_still=True)
    report['status']='geometry, materials, skin and normals imported; visual inspection required'
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__': main()
