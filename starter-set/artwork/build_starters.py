"""Build the reusable catalog with the study's Blender geometry/bake/export path.

Called by generate_parts.py after its deterministic rigid construction stage.
All durable geometry comes from study_geometry, hair/accessories or the canonical
wardrobe sources. Raw GLBs are staging inputs for sculpted curls only.
"""
import argparse
import json
import sys
from pathlib import Path

import bpy

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
sys.path.insert(0,str(ROOT.parents[1]/'studio/tools'))
from artwork import study_geometry as g, study_materials as sm, build_study as export
from artwork import hair, accessories, faces
from artwork.geometry import Mesh
from generate_parts import ASSETS
import generate_person_asset as person
import generate_person_clothing_assets as clothing

BUDGETS={'base':16000,'top':20000,'bottom':9000,'footwear':20000,'hair':17000,
         'facewear':10000,'headwear':13000,'accessory':8000,'face':2500}

FACE_ASSETS = [
    (slug, 'face', slug.replace('-', ' ').title(), 'face', 'face', [])
    for slug in faces.EXPRESSIONS
]


def linear(color):
    return tuple(c/12.92 if c<=.04045 else ((c+.055)/1.055)**2.4 for c in color[:3])


def surface_objects(surfaces,materials):
    objects=[]
    for surface,mat in zip(surfaces,materials):
        positions,indices,joints,weights=surface[:4]
        for joint in sorted({row[0] for row in joints}):
            mesh=Mesh(); remap={}
            for at in range(0,len(indices),3):
                face=indices[at:at+3]
                if joints[face[0]][0]!=joint: continue
                assert all(joints[i][0]==joint for i in face)
                for i in face:
                    if i not in remap:
                        remap[i]=mesh.vertex(tuple(positions[i][axis]+export.BIND[joint][axis] for axis in range(3)))
                mesh.face(*(remap[i] for i in face))
            if mesh.faces: g.object_from_mesh(mat.name,mesh,mat,joint,objects)
    return objects


def body(m,variant):
    # Keep the established head envelope so all interactive expressions and
    # fitted hair/glasses remain supported. Use the study's toy palms/limbs.
    objects=g.body(m)
    for obj in list(objects):
        if obj.vertex_groups.get('2'):
            objects.remove(obj); bpy.data.objects.remove(obj,do_unlink=True)
    head=([],[],[],[])
    person.profile_piece(*head,(0,0,0),(1.02,.94,.85),2,3,'head',variant['head_profile'])
    objects+=surface_objects([head],[m['skin']])
    # The study forearm extended far above the canonical elbow and punched
    # through the sleeve during bending. Keep the exposed segment below it.
    for obj in objects:
        if obj.name.startswith('Forearm'):
            for vertex in obj.data.vertices:
                vertex.co.z=1.085+(vertex.co.z-1.27)*.60
    return objects


def wardrobe(key,m):
    if key=='top':
        # Every tintable detail follows the chosen hoodie color, including the
        # seams. Purple stitching on teal/pink cloth was an old fit/read defect.
        m['seam']=sm.material('Recessed cloth seam',(.30,)*3,tint=True)
        m['stitch']=sm.material('Matching cloth stitch',(.66,)*3,tint=True)
        return g.hoodie(m)
    if key=='bottom':
        m['denim']=sm.material('Denim dye',(.56,)*3,'denim',True)
        m['denim-hem']=sm.material('Turned denim hem',(.66,)*3,'denim',True)
        return g.shorts(m)
    if key in ('shoes','sparkles'):
        if key=='sparkles':
            m['leather']=sm.material('Amethyst satin',linear((.61,.43,.76)),'plain')
            m['cotton']=sm.material('Champagne laces',linear((.87,.78,.55)),'fleece')
        return g.sneakers(m)
    if key=='slacks':
        objects=[]
        cloth=sm.material('Charcoal tailored twill',linear((.19,.215,.26)),'fleece')
        thread=sm.material('Charcoal topstitch',linear((.26,.28,.32)))
        for side,upper,lower in [(-1,9,10),(1,12,13)]:
            g.shell('Tailored upper leg',[(.65,.195,.206),(.77,.213,.225),(1.04,.228,.239),(1.18,.221,.230)],
                    (side*.235,0,0),.48,cloth,upper,objects,folds=.003)
            g.shell('Tapered trouser cuff',[(.405,.187,.194),(.44,.189,.198),(.65,.198,.210),(.72,.204,.216)],
                    (side*.235,0,0),.50,cloth,lower,objects,folds=.002)
            g.tube('Pressed front crease',((side*.235,1.10,-.242),(side*.235,.96,-.245),
                   (side*.235,.80,-.23),(side*.235,.67,-.209)),.0025,thread,upper,objects)
        g.shell('Tailored waistband',[(1.125,.441,.230),(1.17,.453,.241),(1.20,.440,.230)],
                (0,0,0),.45,cloth,0,objects)
        return objects
    asset=clothing.ASSETS[key]
    mats=[]
    for source in asset['materials']:
        tint=source['avatar_tint']; color=(.85,)*3 if tint else linear(source['color'])
        mat=sm.material(source['name'],color,'fleece',tint)
        mats.append(mat)
    surfaces=clothing.clothing_lod_surfaces(asset,3)
    # Retain the actual fitted polo logo, baking its original UV image to the
    # new atlas along with the cloth. Other surfaces receive a fresh unwrap.
    objects=surface_objects(surfaces,mats)
    if key=='short_sleeve_collared':
        # Retain the folded collar, placket and buttons; replace the old
        # disconnected capsule sleeves and narrow torso with fitted cloth.
        for obj in list(objects):
            if obj.data.materials[0] in (mats[0],mats[4]) or any(obj.vertex_groups.get(str(j)) for j in (3,6)):
                objects.remove(obj); bpy.data.objects.remove(obj,do_unlink=True)
            else:
                for vertex in obj.data.vertices:
                    vertex.co.z += .06
                    vertex.co.y += .06
        torso=g.shell('Pique polo body',[(1.18,.438,.290),(1.30,.461,.308),(1.65,.490,.324),
                       (1.91,.492,.307),(2.05,.417,.250),(2.18,.236,.175)],
                      (0,0,0),.48,mats[0],1,objects,folds=.004)
        for side,joint in [(-1,3),(1,6)]:
            sleeve=g.shell('Fitted short sleeve',[(1.47,.166,.182),(1.52,.181,.198),
                          (1.72,.191,.214),(1.87,.169,.186),(2.00,.060,.090)],
                          (side*.555,0,0),.72,mats[0],joint,objects,folds=.003)
            for vertex in sleeve.data.vertices:
                vertex.co.x -= side*.45*max(0,vertex.co.z-1.82)
            g.shell('Polo cuff binding',[(1.455,.166,.183),(1.48,.175,.191),(1.505,.178,.195)],
                    (side*.555,0,0),.72,mats[2],joint,objects)
        # Embroidered geometric mark, not a transparent logo baked on a black
        # rectangle. It follows the actual curved shirt surface at every tint.
        from mathutils import Vector
        coral=sm.material('Coral embroidered mark',linear((.90,.36,.23)))
        def point(x,y):
            hit,location,_,_=torso.ray_cast(Vector((x,2,y)),Vector((0,-1,0)))
            assert hit
            return (x,y,-location.y-.006)
        outline=[(.225,1.917),(.265,1.875),(.225,1.833),(.185,1.875),(.225,1.917)]
        for a,b in zip(outline,outline[1:]):
            p,q=point(*a),point(*b)
            g.tube('Embroidered diamond',(p,p,q,q),.004,coral,1,objects)
        for a,b in [((.213,1.887),(.237,1.863)),((.213,1.863),(.237,1.887))]:
            p,q=point(*a),point(*b); g.tube('Embroidered cross',(p,p,q,q),.004,coral,1,objects)
    return objects


def blend_fit(objects,kind):
    """Blend the reusable garments across existing rig joints, not rigid caps."""
    from artwork.geometry import smooth
    for obj in objects:
        if not obj.vertex_groups: continue
        original=int(obj.vertex_groups[0].name)
        if original not in (3,4,6,7,9,10,12,13): continue
        if original in (3,4,6,7):
            upper,lower=(3,4) if original in (3,4) else (6,7)
            def weights(y):
                forearm=1-smooth((y-1.075)/.235)
                shoulder=smooth((y-1.82)/.23) if kind=='top' else 0
                return [(lower,forearm),(upper,(1-forearm)*(1-shoulder)),(1,(1-forearm)*shoulder)]
        else:
            upper,lower=(9,10) if original in (9,10) else (12,13)
            def weights(y):
                knee=1-smooth((y-.32)/.28)
                return [(upper,1-knee),(lower,knee)]
        obj.vertex_groups.clear()
        for vertex in obj.data.vertices:
            for joint,weight in weights(vertex.co.z):
                if weight<=1e-6: continue
                group=obj.vertex_groups.get(str(joint)) or obj.vertex_groups.new(name=str(joint))
                group.add([vertex.index],weight,'REPLACE')


def build_one(path,objects,kind):
    slug=path.parent.name
    path.parent.mkdir(parents=True, exist_ok=True)
    if kind in ('base','top','bottom','footwear'): blend_fit(objects,kind)
    # Smart-project writes a separate atlas UV layer, leaving the logo's source
    # coordinates intact for baking.
    for obj in objects:
        if not obj.data.uv_layers.get('AtlasUV'): obj.data.uv_layers.new(name='AtlasUV')
        obj.data.uv_layers.active_index=obj.data.uv_layers.find('AtlasUV')
    obj=export.join(objects,'study-swept' if slug=='floppy' else slug)
    for i,mat in enumerate(obj.data.materials): obj.data.materials[i]=mat.copy()
    for mat in obj.data.materials:
        if mat.get('cubaUseAvatarTint'):
            mat['defaultTint']=([.72,.48,.31,1] if kind=='base' else
                                [.20,.275,.40,1] if kind=='bottom' else [.514,.329,.710,1])
    obj.data.calc_loop_triangles()
    bpy.context.view_layer.objects.active=obj
    mod=obj.modifiers.new('Near delivery budget','DECIMATE')
    mod.ratio=min(1.,BUDGETS[kind]/len(obj.data.loop_triangles))
    bpy.ops.object.modifier_apply(modifier=mod.name)
    # The polo is deliberately open at the neck and cuffs; never delete open
    # panels as though they were collapsed closed detail volumes.
    if slug!='short-sleeve-collared': export.remove_collapsed_details(obj)
    # Unwrap and bake the actual delivery mesh. Baking a huge sculpt first,
    # then collapsing its tiny UV islands, caused dark speckled hair edges.
    bpy.ops.object.mode_set(mode='EDIT'); bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.uv.smart_project(angle_limit=1.10,island_margin=.012,area_weight=.7)
    bpy.ops.object.mode_set(mode='OBJECT')
    image=export.bake(obj,slug,ROOT/'review/bakes')
    lods={'near':obj}
    for level,ratio in [('mid',.32),('far',.07)]:
        low=obj.copy(); low.data=obj.data.copy(); bpy.context.collection.objects.link(low)
        bpy.context.view_layer.objects.active=low
        mod=low.modifiers.new('Delivery reduction','DECIMATE'); mod.ratio=ratio
        bpy.ops.object.modifier_apply(modifier=mod.name)
        if slug!='short-sleeve-collared': export.remove_collapsed_details(low)
        lods[level]=low
    rigid=kind not in ('base','top','bottom','footwear')
    counts,materials=export.write_glb(path,lods,image,rigid=rigid,preserve_weights=True)
    manifest_path = path.with_suffix('.morph.json')
    if manifest_path.is_file():
        manifest=json.loads(manifest_path.read_text())
    else:
        manifest={
            'schemaVersion': 1,
            'asset': {
                'id': f'cuba:{kind}/{slug}.v1',
                'kind': kind,
                'displayName': slug.replace('-', ' ').title(),
                'rigProfile': 'cuba:rig/biped15.v1',
                'fitProfiles': ['cuba:fit/person-standard.v1'],
                'supportedBases': ['cuba:base/person.v1', 'cuba:base/person-02.v1'],
                'occupiedSlots': ['face'],
                'coverage': ['face'],
                'conflicts': [],
                'materials': [],
                'lod': {},
                'requiredCapabilities': [],
                'source': {'geometry': path.name},
                'provenance': {
                    'source': 'Cubacadabra starter-set/artwork/faces.py',
                    'license': 'Cubacadabra official',
                },
            },
            'geometry': {'file': path.name, 'lodNodes': {level: level for level in lods}},
            'attachment': {
                'mode': 'rigid', 'joint': 'head', 'translation': [0, 0, 0],
                'rotation': [0, 0, 0, 1], 'scale': [1, 1, 1],
            },
        }
    manifest['asset']['lod']=counts; manifest['asset']['materials']=materials
    manifest['geometry']['lodNodes']={level:level for level in lods}
    caps=['mesh.rigid.v1' if rigid else 'skin.biped15-linear.v1','material.base-color-texture.v1']
    if kind=='base': caps.append('rig.canonical-rest.v1')
    if kind=='hair': caps.append('hair.authored.v1')
    if kind=='accessory': caps.append('accessory.ear-device.v1')
    if kind=='face': caps.append('face.authored-static.v1')
    manifest['asset']['requiredCapabilities']=caps
    manifest['asset']['provenance']['source']='Cubacadabra artwork/build_starters.py; study construction and Blender color/AO bake'
    manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'BAKED {slug}: {counts}',flush=True)


def main():
    parser=argparse.ArgumentParser(); parser.add_argument('--only',nargs='+'); parser.add_argument('--wardrobe',action='store_true')
    args=parser.parse_args(sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else [])
    specs=[]
    for spec in ASSETS:
        slug,kind,*_=spec
        if args.only and slug not in args.only: continue
        filename='test_top_hat' if slug=='test-top-hat' else slug
        specs.append((ROOT/f'source/morphs/{kind}/{slug}/{filename}.glb',kind,slug,spec))
    if not args.only or any(slug in faces.EXPRESSIONS for slug in args.only):
        specs.extend(
            (ROOT / 'source/morphs/face' / slug / f'{slug}.glb', kind, slug, spec)
            for slug, kind, _name, _slot, _coverage, _materials in FACE_ASSETS
            if not args.only or slug in args.only
        )
    if args.wardrobe:
        for relative,key in [('base/person/person_skinned','person-01'),('base/person-02/person_02','person-02'),
                             ('top/person-top/person_top','top'),('bottom/person-bottom/person_bottom','bottom'),
                             ('footwear/person-shoes/person_shoes','shoes'),('footwear/sparkles/person_sparkles','sparkles'),
                             ('top/short-sleeve-collared/person_short_sleeve_collared','short_sleeve_collared'),
                             ('bottom/slacks/person_slacks','slacks')]:
            specs.append((ROOT/f'source/morphs/{relative}.glb',relative.split('/')[0],key,None))
    for path,kind,key,spec in specs:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        scene=bpy.context.scene; scene.render.engine='CYCLES'; scene.cycles.samples=16
        scene.cycles.seed=47; scene.world=bpy.data.worlds.new('Neutral bake world')
        if kind=='base': objects=body(sm.materials(),person.PERSON_VARIANTS[key])
        elif kind in ('top','bottom','footwear'): objects=wardrobe(key,sm.materials())
        elif kind=='face':
            face_materials = {
                'sclera': sm.material('Face sclera',linear((.93,.89,.82))),
                'iris': sm.material('Face iris',linear((.17,.095,.055))),
                'pupil': sm.material('Face pupil',linear((.018,.012,.010))),
                'highlight': sm.material('Face catchlight',linear((.99,.98,.94))),
                'ink': sm.material('Face brow ink',linear((.12,.07,.045))),
                'mouth': sm.material('Face mouth',linear((.08,.05,.07))),
                'tongue': sm.material('Face tongue',linear((.82,.29,.34))),
                'teeth': sm.material('Face teeth',linear((.96,.92,.82))),
            }
            objects=faces.build(key,3,face_materials)
        elif key=='floppy':
            objects=g.hair(sm.materials())
            # Fit the established broader interactive head envelope.
            for obj in objects:
                for vertex in obj.data.vertices:
                    vertex.co.x *= 1.045
                    vertex.co.y *= 1.035
        else:
            materials=[sm.material(name,linear(color),'hair' if kind=='hair' else 'plain') for name,color,rough in spec[-1]]
            if key in ('curls','coils'):
                bpy.ops.import_scene.gltf(filepath=str(path),import_shading='NORMALS')
                obj=next(o for o in bpy.data.objects if o.type=='MESH' and o.name=='near')
                mesh=Mesh()
                for vertex in obj.data.vertices:
                    x,nz,y=vertex.co; mesh.vertex((x,y+2.70,-nz))
                obj.data.calc_loop_triangles()
                for tri in obj.data.loop_triangles: mesh.face(*tri.vertices)
                bpy.ops.object.select_all(action='SELECT'); bpy.ops.object.delete(use_global=False)
            else:
                mesh=(hair.build if kind=='hair' else accessories.build)(key,3)
                mesh.vertices=[(x,y+2.70,z) for x,y,z in mesh.vertices]
            objects=[]
            obj=g.object_from_mesh(key,mesh,materials[0],2,objects)
            for mat in materials[1:]: obj.data.materials.append(mat)
            for poly,index in zip(obj.data.polygons,mesh.materials): poly.material_index=index
        build_one(path,objects,kind)


if __name__=='__main__': main()
