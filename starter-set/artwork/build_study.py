"""Build one independent mockup art study with embedded baked color atlases.

Run Blender --background --python artwork/build_study.py. Outputs include
editable source geometry, three delivery LODs per part, and a composed .blend.
"""
import json
import struct
import sys
from pathlib import Path

import bpy
import bmesh

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
sys.path.insert(0,str(ROOT.parents[1]/'studio/tools'))
from artwork import study_geometry as geometry, study_materials
import generate_person_asset as person

OUT=ROOT/'studies/mockup-person'
BIND=[]
for _,parent,translation in person.JOINTS:
    BIND.append(tuple(translation[i]+(BIND[parent][i] if parent is not None else 0) for i in range(3)))

SPECS=[
    ('base','study-person','Studio study base',geometry.body,['base'],['body','static-face']),
    ('hair','study-swept','Sculpted chestnut sweep',geometry.hair,['hair'],['scalp']),
    ('top','study-hoodie','Purple fleece hoodie',geometry.hoodie,['shirt'],['torso','arms']),
    ('bottom','study-denim','Indigo denim shorts',geometry.shorts,['pants'],['legs']),
    ('footwear','study-sneakers','White leather sneakers',geometry.sneakers,['shoes'],['feet']),
]


def join(objects,name):
    bpy.ops.object.select_all(action='DESELECT')
    for obj in objects: obj.select_set(True)
    bpy.context.view_layer.objects.active=objects[0]
    bpy.ops.object.join()
    obj=bpy.context.object; obj.name=name
    if name=='study-swept':
        # Sculpt the intersecting foundation/lock roots into one hair volume.
        # The same Near/Mid/Far delivery ceilings still apply below. A union
        # removes internal contact seams; it does not add painted shadows.
        union=obj.modifiers.new('Continuous sculpted hair roots','REMESH')
        union.mode='VOXEL'; union.voxel_size=.006
        union.use_smooth_shade=True
        bpy.ops.object.modifier_apply(modifier=union.name)
        soften=obj.modifiers.new('Relax voxel surface','SMOOTH')
        soften.factor=.65; soften.iterations=3
        bpy.ops.object.modifier_apply(modifier=soften.name)
        group=obj.vertex_groups.get('2') or obj.vertex_groups.new(name='2')
        group.add(list(range(len(obj.data.vertices))),1.,'REPLACE')
        for polygon in obj.data.polygons: polygon.use_smooth=True
    # Recalculate consistent winding before UVs and delivery reduction.
    bpy.ops.object.mode_set(mode='EDIT')
    bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.mesh.normals_make_consistent(inside=False)
    bpy.ops.uv.smart_project(angle_limit=1.10,island_margin=.012,area_weight=.7)
    bpy.ops.object.mode_set(mode='OBJECT')
    return obj


def bake(obj,slug):
    image=bpy.data.images.new(slug+'-color-contact',width=512,height=512,alpha=False)
    for mat in obj.data.materials:
        node=mat.node_tree.nodes.new('ShaderNodeTexImage'); node.image=image
        mat.node_tree.nodes.active=node
    bpy.ops.object.select_all(action='DESELECT'); obj.select_set(True)
    bpy.context.view_layer.objects.active=obj
    bpy.ops.object.bake(type='EMIT',margin=5,use_clear=True)
    texture_dir=OUT/'textures'; texture_dir.mkdir(parents=True,exist_ok=True)
    image.filepath_raw=str(texture_dir/(slug+'.png')); image.file_format='PNG'; image.save()
    return image


def write_glb(path,lods,image):
    binary=bytearray(); views=[]; accessors=[]; meshes=[]
    def blob(data,target=None):
        binary.extend(b'\0'*(-len(binary)%4))
        view={'buffer':0,'byteOffset':len(binary),'byteLength':len(data)}
        if target: view['target']=target
        views.append(view); binary.extend(data)
        return len(views)-1
    def accessor(values,fmt,component,kind,target):
        idx=len(accessors)
        item={'bufferView':blob(b''.join(struct.pack(fmt,*v) for v in values),target),
              'componentType':component,'count':len(values),'type':kind}
        if kind=='VEC3':
            item['min']=[min(p[i] for p in values) for i in range(3)]
            item['max']=[max(p[i] for p in values) for i in range(3)]
        accessors.append(item); return idx
    # Reuse one atlas at all LODs. UVs survive the geometry decimator.
    image_view=blob(Path(image.filepath_raw).read_bytes())
    counts={}
    materials=[]
    for mat in lods['near'].data.materials:
        tint=bool(mat.get('cubaUseAvatarTint',False))
        default=([184/255,123/255,78/255,1] if mat.name.startswith('Warm skin') else [131/255,84/255,181/255,1]) if tint else [1,1,1,1]
        # glTF factors are linear, whereas the editor palette is display sRGB.
        # The runtime substitutes its live palette for these tintable surfaces;
        # standalone GLB viewers need the correctly encoded default factor.
        default=[c/12.92 if c<=.04045 else ((c+.055)/1.055)**2.4 for c in default[:3]]+[default[3]]
        materials.append({'name':mat.name,'extras':{'cubaUseAvatarTint':bool(mat.get('cubaUseAvatarTint',False))},
                          'pbrMetallicRoughness':{'baseColorFactor':default, 'baseColorTexture':{'index':0},
                                                   'roughnessFactor':mat.get('roughness',.86),'metallicFactor':0}})
    for level,obj in lods.items():
        data=obj.data; data.calc_loop_triangles(); uv=data.uv_layers.active.data
        primitives=[]; total=0
        for mat_index in range(len(materials)):
            lookup={}; positions=[]; normals=[]; uvs=[]; joints=[]; weights=[]; indices=[]
            for tri in data.loop_triangles:
                if tri.material_index!=mat_index: continue
                for loop_index in tri.loops:
                    index=data.loops[loop_index].vertex_index; vertex=data.vertices[index]
                    texcoord=tuple(uv[loop_index].uv)
                    normal=tuple(data.corner_normals[loop_index].vector)
                    key=(index,tuple(round(v,6) for v in texcoord),normal)
                    if key not in lookup:
                        group=max(vertex.groups,key=lambda g:g.weight)
                        joint=int(obj.vertex_groups[group.group].name)
                        x,negative_z,y=vertex.co; world=(x,y,-negative_z)
                        positions.append(tuple(world[i]-BIND[joint][i] for i in range(3)))
                        nx,nz,ny=normal; normals.append((nx,ny,-nz))
                        uvs.append((texcoord[0],1-texcoord[1]))
                        joints.append((joint,0,0,0)); weights.append((1.,0.,0.,0.))
                        lookup[key]=len(positions)-1
                    indices.append((lookup[key],))
            if not indices: continue
            total+=len(indices)//3
            attributes={'POSITION':accessor(positions,'<3f',5126,'VEC3',34962),
                        'NORMAL':accessor(normals,'<3f',5126,'VEC3',34962),
                        'TEXCOORD_0':accessor(uvs,'<2f',5126,'VEC2',34962),
                        'JOINTS_0':accessor(joints,'<4H',5123,'VEC4',34962),
                        'WEIGHTS_0':accessor(weights,'<4f',5126,'VEC4',34962)}
            primitives.append({'attributes':attributes,'indices':accessor(indices,'<I',5125,'SCALAR',34963),
                               'mode':4,'material':mat_index})
        counts[level]=total; meshes.append({'name':level,'primitives':primitives})
    nodes=[{'name':name,'translation':list(t)} for name,_,t in person.JOINTS]
    for i,(_,parent,_) in enumerate(person.JOINTS):
        if parent is not None: nodes[parent].setdefault('children',[]).append(i)
    for i,level in enumerate(lods): nodes.append({'name':level,'mesh':i,'skin':0})
    nodes[0].setdefault('children',[]).extend([15,16,17])
    identity=tuple(1. if i%5==0 else 0. for i in range(16))
    binds=accessor([identity]*15,'<16f',5126,'MAT4',None)
    binary.extend(b'\0'*(-len(binary)%4))
    document={'asset':{'version':'2.0','generator':'Cubacadabra mockup study'},'scene':0,'scenes':[{'nodes':[0]}],
              'nodes':nodes,'meshes':meshes,'materials':materials,'images':[{'bufferView':image_view,'mimeType':'image/png'}],
              'textures':[{'source':0}], 'skins':[{'name':'Person_Skin','joints':list(range(15)),'inverseBindMatrices':binds,'skeleton':0}],
              'buffers':[{'byteLength':len(binary)}],'bufferViews':views,'accessors':accessors}
    doc=json.dumps(document,separators=(',',':')).encode(); doc+=b' '*(-len(doc)%4)
    path.write_bytes(struct.pack('<III',0x46546c67,2,28+len(doc)+len(binary))+
                     struct.pack('<II',len(doc),0x4e4f534a)+doc+struct.pack('<II',len(binary),0x004e4942)+binary)
    return counts,[m['name'].lower().replace(' ','-').replace('.','-') for m in materials]


def remove_collapsed_details(obj):
    """Drop closed detail volumes flattened to double-sided wafers by decimation.

    At Far, subpixel sole siping can collapse to two opposite triangles with
    zero averaged normals. Remove those vanished volumes at authoring time.
    """
    mesh=bmesh.new(); mesh.from_mesh(obj.data)
    pending=set(mesh.verts); collapsed=[]
    while pending:
        seed=pending.pop(); component={seed}; frontier=[seed]
        while frontier:
            for edge in frontier.pop().link_edges:
                for vertex in edge.verts:
                    if vertex in pending:
                        pending.remove(vertex); component.add(vertex); frontier.append(vertex)
        faces={face for vertex in component for face in vertex.link_faces}
        origin=seed.co
        volume=0.
        for face in faces:
            points=[v.co-origin for v in face.verts]
            for i in range(1,len(points)-1): volume+=points[0].dot(points[i].cross(points[i+1]))/6
        if abs(volume)<1e-12: collapsed.extend(component)
    if collapsed: bmesh.ops.delete(mesh,geom=collapsed,context='VERTS')
    mesh.to_mesh(obj.data); mesh.free(); obj.data.update()


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene=bpy.context.scene; scene.render.engine='CYCLES'; scene.cycles.samples=24
    scene.cycles.bake_type='EMIT'; scene.render.bake.use_pass_direct=False; scene.render.bake.use_pass_indirect=False
    scene.world=bpy.data.worlds.new('Neutral bake world')
    materials=study_materials.materials()
    objects={slug:join(builder(materials),slug) for _,slug,_,builder,_,_ in SPECS}
    catalog=[]
    for kind,slug,name,_,slots,coverage in SPECS:
        obj=objects[slug]
        # An atlas is unique to its part; clone shared materials before assigning
        # a bake target so other parts cannot accidentally receive this image.
        for i,mat in enumerate(obj.data.materials): obj.data.materials[i]=mat.copy()
        image=bake(obj,slug)
        # Bake from the sculpt, then deliver silhouette-preserving reductions.
        # The source construction density is not a runtime triangle budget.
        targets={'base':16000,'hair':18000,'top':20000,'bottom':9000,'footwear':20000}
        obj.data.calc_loop_triangles()
        bpy.context.view_layer.objects.active=obj
        reduction=obj.modifiers.new('Near delivery budget','DECIMATE')
        reduction.ratio=min(1.,targets[kind]/len(obj.data.loop_triangles))
        bpy.ops.object.modifier_apply(modifier=reduction.name)
        remove_collapsed_details(obj)
        lods={'near':obj}
        for level,ratio in [('mid',.32),('far',.07)]:
            low=obj.copy(); low.data=obj.data.copy(); bpy.context.collection.objects.link(low)
            bpy.context.view_layer.objects.active=low
            modifier=low.modifiers.new('Delivery reduction','DECIMATE'); modifier.ratio=ratio
            bpy.ops.object.modifier_apply(modifier=modifier.name)
            remove_collapsed_details(low)
            low.hide_render=True; low.hide_set(True); lods[level]=low
        directory=OUT/'source/morphs'/kind/slug; directory.mkdir(parents=True,exist_ok=True)
        path=directory/(slug+'.glb'); counts,material_names=write_glb(path,lods,image)
        capabilities=['skin.biped15-linear.v1','material.base-color-texture.v1']
        if kind=='base': capabilities.extend(['face.authored-static.v1','rig.canonical-rest.v1'])
        if kind=='hair': capabilities.append('hair.authored.v1')
        manifest={'schemaVersion':2,'asset':{'id':f'cuba:{kind}/{slug}.v1','kind':kind,'displayName':name,
                  'rigProfile':'cuba:rig/biped15.v1','fitProfiles':['cuba:fit/person-standard.v1'],
                  'occupiedSlots':slots,'coverage':coverage,'conflicts':[],
                  'materials':material_names,'lod':counts,'requiredCapabilities':capabilities,'source':{'geometry':path.name},
                  'provenance':{'source':'Cubacadabra starter-set/artwork/build_study.py','license':'Cubacadabra official'}},
                  'geometry':{'file':path.name,'lodNodes':{level:level for level in lods}},
                  'attachment':{'mode':'skinned','joint':'root','translation':[0,0,0],'rotation':[0,0,0,1],'scale':[1,1,1]},
                  'skin':{'skeleton':'Person_Skin','jointOrder':[j[0] for j in person.JOINTS],'maxInfluences':4}}
        if kind!='base': manifest['asset']['supportedBases']=['cuba:base/study-person.v1']
        path.with_suffix('.morph.json').write_text(json.dumps(manifest,indent=2)+'\n')
        catalog.append({'id':manifest['asset']['id'],'source':str(path.with_suffix('.morph.json').relative_to(OUT))})
        for mat in obj.data.materials:
            n=mat.node_tree.nodes; n.clear(); output=n.new('ShaderNodeOutputMaterial'); bsdf=n.new('ShaderNodeBsdfPrincipled')
            tex=n.new('ShaderNodeTexImage'); tex.image=image
            mat.node_tree.links.new(tex.outputs['Color'],bsdf.inputs['Base Color'])
            if mat.get('cubaUseAvatarTint'):
                mix=n.new('ShaderNodeMixRGB'); mix.blend_type='MULTIPLY'; mix.inputs[0].default_value=1
                mix.inputs[2].default_value=(*mat['displayColor'],1)
                mat.node_tree.links.new(tex.outputs['Color'],mix.inputs[1]); mat.node_tree.links.new(mix.outputs[0],bsdf.inputs['Base Color'])
            bsdf.inputs['Roughness'].default_value=mat['roughness']
            mat.node_tree.links.new(bsdf.outputs[0],output.inputs[0])
        print(slug,counts,flush=True)
    (OUT/'catalog.json').write_text(json.dumps({'schemaVersion':1,'assets':catalog,
        'builtins':'../../../../rust/assets/characters/morph_catalog.json','excludeBuiltinKinds':['outfit'],
        'presets':[{'source':'presets/mockup-person.json'}]},indent=2)+'\n')
    (OUT/'presets').mkdir(exist_ok=True)
    (OUT/'presets/mockup-person.json').write_text(json.dumps({'id':'cuba:preset/mockup-person.v1','displayName':'Mockup art study',
        'base':'cuba:base/study-person.v1','parts':[f'cuba:{kind}/{slug}.v1' for kind,slug,*_ in SPECS[1:]],
        'parameters':{'skin':'#b87b4e','primary':'#8354b5','secondary':'#24384e','sole':'#f1ebdf'}},indent=2)+'\n')
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'mockup-person.blend'),compress=True)


if __name__=='__main__': main()
