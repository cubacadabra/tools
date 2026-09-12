"""Geometry and fit checks for the authored starter GLBs; no Blender required."""
import importlib.util
import math
import json
import struct
import sys
import unittest
from collections import Counter
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]/"starter-set"
sys.path.insert(0,str(ROOT))
from artwork import hair, accessories
from artwork.geometry import Mesh, cross, sub, dot
from generate_parts import ASSETS


def sculpted_mesh(slug, detail):
    """Test the actual sculpt delivery topology, without requiring Blender."""
    data=(ROOT/"source/morphs/hair"/slug/f"{slug}.glb").read_bytes()
    length=struct.unpack_from("<I",data,12)[0]
    document=json.loads(data[20:20+length])
    binary=data[28+length:]
    def read(index):
        accessor=document["accessors"][index]
        view=document["bufferViews"][accessor["bufferView"]]
        size={"SCALAR":1,"VEC3":3}[accessor["type"]]
        fmt="<"+{5126:"f",5125:"I"}[accessor["componentType"]]*size
        return [struct.unpack_from(fmt,binary,view.get("byteOffset",0)+i*struct.calcsize(fmt))
                for i in range(accessor["count"])]
    mesh=Mesh()
    for primitive in document["meshes"][3-detail]["primitives"]:
        start=len(mesh.vertices)
        for point in read(primitive["attributes"]["POSITION"]): mesh.vertex(point)
        indices=read(primitive["indices"])
        for offset in range(0,len(indices),3):
            mesh.face(*(start+i[0] for i in indices[offset:offset+3]))
    return mesh.clean()


class StarterArtworkTests(unittest.TestCase):
    def test_every_source_lod_exports_authored_unit_normals(self):
        paths=list((ROOT/'source/morphs').rglob('*.glb'))
        paths+=list((ROOT/'studies/mockup-person/source/morphs').rglob('*.glb'))
        for path in paths:
            data=path.read_bytes(); length=struct.unpack_from('<I',data,12)[0]
            doc=json.loads(data[20:20+length]); binary=data[28+length:]
            for mesh in doc['meshes']:
                for primitive in mesh['primitives']:
                    with self.subTest(asset=path.name,lod=mesh.get('name')):
                        attributes=primitive['attributes']
                        self.assertIn('NORMAL',attributes)
                        normal=doc['accessors'][attributes['NORMAL']]
                        self.assertEqual(normal['count'],doc['accessors'][attributes['POSITION']]['count'])
                        self.assertEqual((normal['componentType'],normal['type']),(5126,'VEC3'))
                        view=doc['bufferViews'][normal['bufferView']]
                        offset=view.get('byteOffset',0)+normal.get('byteOffset',0)
                        for i in range(normal['count']):
                            n=struct.unpack_from('<3f',binary,offset+i*view.get('byteStride',12))
                            self.assertTrue(all(math.isfinite(v) for v in n))
                            self.assertAlmostEqual(sum(v*v for v in n),1.,delta=.02)

    def test_all_lods_are_finite_closed_outward_and_bounded(self):
        for slug,kind,*_ in ASSETS:
            counts=[]
            for detail in (3,2,1):
                with self.subTest(asset=slug,lod=detail):
                    mesh=(sculpted_mesh(slug,detail) if slug in ("curls","coils")
                          else (hair.build if kind=="hair" else accessories.build)(slug,detail))
                    counts.append(len(mesh.faces))
                    self.assertTrue(all(math.isfinite(v) and abs(v)<1.5 for p in mesh.vertices for v in p))
                    self.assertEqual(len(mesh.faces),len(mesh.materials))
                    edges=Counter()
                    volume=0.
                    for a,b,c in mesh.faces:
                        self.assertEqual(len({a,b,c}),3)
                        self.assertLess(max(a,b,c),len(mesh.vertices))
                        pa,pb,pc=[mesh.vertices[i] for i in (a,b,c)]
                        n=cross(sub(pb,pa),sub(pc,pa))
                        self.assertGreater(dot(n,n),1e-18)
                        volume+=dot(pa,cross(pb,pc))/6
                        for start,end in ((a,b),(b,c),(c,a)):
                            edges[tuple(sorted((start,end)))]+=1
                    self.assertTrue(all(count==2 for count in edges.values()),"open/non-manifold surface")
                    self.assertGreater(volume,0,"inward triangle winding")
            # Near is intentionally presentation quality; the renderer only
            # selects it above 180 projected pixels. Keep a hard content bound
            # while allowing layered locks and sculpted curl silhouettes.
            self.assertLess(counts[0],17000)
            self.assertLess(counts[2],2000)
            self.assertGreater(counts[0],counts[1])
            self.assertGreater(counts[1],counts[2])

    def test_cap_clears_both_existing_foreheads_and_temples(self):
        spec=importlib.util.spec_from_file_location("person_fit",ROOT.parents[1]/"studio/tools/generate_person_asset.py")
        person=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(person)
        surface=hair.scalp(Mesh(),3)
        for variant in person.PERSON_VARIANTS.values():
            for row in range(1,24):
                for column in range(64):
                    x,y,z=surface(column/64,row/24)
                    if not -.30<y<.42: continue
                    rx,rz=person.profile(variant["head_profile"],(y+.47)/.94)
                    outside=(abs(x)/(rx*1.02))**(2/.58)+(abs(z)/(rz*.85))**(2/.58)
                    self.assertGreaterEqual(outside,1.0,"hair cap intersects the base head")

    def test_generation_is_deterministic_and_glasses_are_not_beads(self):
        first=accessories.build("round-glasses",3)
        second=accessories.build("round-glasses",3)
        self.assertEqual(first.vertices,second.vertices)
        self.assertEqual(first.faces,second.faces)
        # Two welded toroidal rims, three arms/bridge sweeps, and two hinges.
        neighbors={index:set() for index in range(len(first.vertices))}
        for face in first.faces:
            for a in face: neighbors[a].update(face)
        pending=set(i for face in first.faces for i in face)
        components=0
        while pending:
            frontier=[pending.pop()]
            while frontier:
                new=neighbors[frontier.pop()] & pending
                pending.difference_update(new)
                frontier.extend(new)
            components+=1
        self.assertEqual(components,7)
