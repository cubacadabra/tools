"""Bake unified curly hair volumes with Blender's voxel sculpting tools.

The overlapping construction forms are unioned and relaxed before generating
the three delivery LODs. No separate beads, raised wires, or hidden beauty mesh.
"""
import json
import math
import random
import sys
from pathlib import Path
import bpy

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from artwork.geometry import Mesh, add, mul, unit, ellipsoid
from artwork.hair import scalp
from generate_parts import ASSETS, write_glb


def build(slug):
    tight=slug=="coils"
    randomizer=random.Random(47 if tight else 83)
    mesh=Mesh()
    surface=scalp(mesh,3,rx=.535,rz=.465,top=.60,back=-.12,front=.30)
    count=64 if tight else 34
    for index in range(count):
        u=(index*.61803398875+.013)%1
        v=.12+.86*math.sqrt((index+.5)/count)
        center=surface(u,v)
        normal=unit((center[0],center[1]-.02,center[2]))
        center=add(center,mul(normal,.005))
        radius=(.125 if tight else .18)*randomizer.uniform(.84,1.14)
        ellipsoid(mesh,center,(radius*2.05,radius*1.65,radius*1.94),2,
                  twist=randomizer.uniform(-.6,.6))
    # Fill the crown so its profile is a cohesive curly volume.
    ellipsoid(mesh,(0.,.56,.005),(.44,.24,.39),2)
    mesh.clean()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    data=bpy.data.meshes.new(slug)
    data.from_pydata([(x,-z,y) for x,y,z in mesh.vertices],[],mesh.faces)
    data.update()
    obj=bpy.data.objects.new(slug,data)
    bpy.context.collection.objects.link(obj)
    bpy.context.view_layer.objects.active=obj
    obj.select_set(True)
    obj.data.remesh_voxel_size=.009 if tight else .011
    bpy.ops.object.voxel_remesh()
    smooth=obj.modifiers.new("Soft sculpted junctions","SMOOTH")
    smooth.factor=.72
    smooth.iterations=5
    bpy.ops.object.modifier_apply(modifier=smooth.name)
    lods={}
    for level,budget in [("near",9000),("mid",3000),("far",900)]:
        copy=obj.copy()
        copy.data=obj.data.copy()
        bpy.context.collection.objects.link(copy)
        bpy.context.view_layer.objects.active=copy
        decimate=copy.modifiers.new("Delivery budget","DECIMATE")
        source_triangles=sum(len(p.vertices)-2 for p in copy.data.polygons)
        decimate.ratio=min(1.,budget/source_triangles)
        bpy.ops.object.modifier_apply(modifier=decimate.name)
        triangulate=copy.modifiers.new("Delivery triangles","TRIANGULATE")
        bpy.ops.object.modifier_apply(modifier=triangulate.name)
        result=Mesh()
        for vertex in copy.data.vertices:
            x,negative_depth,height=vertex.co
            result.vertex((x,height,-negative_depth))
        for polygon in copy.data.polygons:
            result.face(*polygon.vertices)
        lods[level]=result.clean()
        bpy.data.objects.remove(copy,do_unlink=True)
    path=ROOT/"source/morphs/hair"/slug/f"{slug}.glb"
    materials=next(spec[-1] for spec in ASSETS if spec[0]==slug)
    write_glb(path,lods,materials)
    sidecar=path.with_suffix(".morph.json")
    document=json.loads(sidecar.read_text())
    document["asset"]["lod"]={level:len(mesh.faces) for level,mesh in lods.items()}
    document["asset"]["provenance"]["source"]="Cubacadabra artwork/sculpt_curls.py · voxel-unioned sculpt"
    sidecar.write_text(json.dumps(document,indent=2)+"\n")
    print(slug,document["asset"]["lod"])


for slug in sys.argv[sys.argv.index("--")+1:]: build(slug)
