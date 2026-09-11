#!/usr/bin/env python3
"""Build artist-authored starter surfaces and editable GLBs.

No runtime or Python package dependencies. Sculpted curls require Blender
at authoring time. --blend also saves an editable scene for each component.
"""
import argparse
import json
import shutil
import struct
import subprocess
from pathlib import Path

from artwork import hair, accessories
from artwork.geometry import add, cross, sub, unit

ROOT = Path(__file__).resolve().parent

# Colors are deliberately restrained sRGB values, matching the game's renderer.
ASSETS = [
    ("floppy", "hair", "Floppy Hair", "hair", "scalp", [("chestnut", [.29,.155,.085,1], .65)]),
    ("curls", "hair", "Loose Curls", "hair", "scalp", [("warm-brown", [.32,.18,.095,1], .76)]),
    ("coils", "hair", "Coils", "hair", "scalp", [("espresso", [.105,.067,.046,1], .84)]),
    ("braids", "hair", "Twin Braids", "hair", "scalp", [("dark-brown", [.135,.08,.052,1], .73), ("ties", [.30,.56,.51,1], .8)]),
    ("buzz", "hair", "Buzz Cut", "hair", "scalp", [("umber", [.13,.087,.059,1], .9)]),
    ("bob", "hair", "Bob", "hair", "scalp", [("chocolate", [.19,.10,.065,1], .68)]),
    ("buns", "hair", "Double Buns", "hair", "scalp", [("espresso", [.13,.075,.05,1], .74)]),
    ("round-glasses", "facewear", "Round Glasses", "facewear", "eyes", [("slate-acetate", [.19,.28,.32,1], .35), ("brass-hinges", [.67,.47,.25,1], .4)]),
    ("square-glasses", "facewear", "Square Glasses", "facewear", "eyes", [("tortoise-acetate", [.35,.19,.115,1], .35), ("brass-hinges", [.67,.47,.25,1], .4)]),
    ("hearing-aids", "accessory", "Hearing Aids", "ear-device", "ears", [("blue-shell", [.27,.44,.53,1], .6), ("receiver", [.47,.55,.58,1], .8)]),
    ("headphones", "headwear", "Headphones", "ear-accessory", "ears", [("slate-shell", [.16,.24,.30,1], .5), ("soft-padding", [.075,.095,.11,1], .9), ("blue-trim", [.29,.43,.48,1], .5)]),
    ("test-top-hat", "headwear", "Top Hat", "headwear", "head", [("indigo-felt", [.18,.20,.36,1], .95), ("ochre-ribbon", [.70,.43,.22,1], .8), ("brass-buckle", [.80,.63,.36,1], .4)]),
]


def write_glb(path, lods, materials):
    binary, views, accessors, meshes, nodes = bytearray(), [], [], [], []
    def accessor(values, fmt, component, kind, target):
        while len(binary) % 4: binary.append(0)
        offset = len(binary)
        binary.extend(b"".join(struct.pack(fmt, *v) for v in values))
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(binary)-offset, "target": target})
        item = {"bufferView": len(views)-1, "componentType": component, "count": len(values), "type": kind}
        if kind == "VEC3":
            item["min"] = [min(v[i] for v in values) for i in range(3)]
            item["max"] = [max(v[i] for v in values) for i in range(3)]
        accessors.append(item)
        return len(accessors)-1

    for level, mesh in lods.items():
        normals = [(0.,0.,0.) for _ in mesh.vertices]
        for a,b,c in mesh.faces:
            n = cross(sub(mesh.vertices[b],mesh.vertices[a]),sub(mesh.vertices[c],mesh.vertices[a]))
            for index in (a,b,c): normals[index]=add(normals[index],n)
        normals = [unit(n) for n in normals]
        primitives = []
        for material in range(len(materials)):
            faces = [f for f,m in zip(mesh.faces,mesh.materials) if m==material]
            if not faces: continue
            used = sorted({i for face in faces for i in face})
            remap = {old:new for new,old in enumerate(used)}
            position = accessor([mesh.vertices[i] for i in used], "<3f", 5126, "VEC3", 34962)
            normal = accessor([normals[i] for i in used], "<3f", 5126, "VEC3", 34962)
            uv = accessor([mesh.uvs[i] for i in used], "<2f", 5126, "VEC2", 34962)
            indices = accessor([(remap[i],) for f in faces for i in f], "<I", 5125, "SCALAR", 34963)
            primitives.append({"attributes": {"POSITION":position,"NORMAL":normal,"TEXCOORD_0":uv},
                               "indices":indices, "material":material, "mode":4})
        meshes.append({"name":level,"primitives":primitives})
        nodes.append({"name":level,"mesh":len(meshes)-1})
    document = {"asset":{"version":"2.0","generator":"Cubacadabra sculpted starter artwork"},
                "scene":0,"scenes":[{"nodes":[0,1,2]}],"nodes":nodes,"meshes":meshes,
                "materials":[{"name":name,"pbrMetallicRoughness":{"baseColorFactor":color,"roughnessFactor":roughness,"metallicFactor":0}}
                             for name,color,roughness in materials],
                "accessors":accessors,"bufferViews":views,"buffers":[{"byteLength":len(binary)}]}
    data=json.dumps(document,separators=(",",":")).encode()
    data += b" " * (-len(data)%4)
    path.write_bytes(struct.pack("<III",0x46546C67,2,28+len(data)+len(binary))
                     +struct.pack("<II",len(data),0x4E4F534A)+data
                     +struct.pack("<II",len(binary),0x004E4942)+binary)


def build_asset(spec):
    slug,kind,name,slot,coverage,materials = spec
    directory=ROOT/"source/morphs"/kind/slug
    directory.mkdir(parents=True,exist_ok=True)
    filename = "test_top_hat" if slug=="test-top-hat" else slug
    path=directory/f"{filename}.glb"
    builder=hair.build if kind=="hair" else accessories.build
    lods={level:builder(slug,detail) for level,detail in [("near",3),("mid",2),("far",1)]}
    write_glb(path,lods,materials)
    manifest_path=directory/f"{filename}.morph.json"
    manifest=json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    capabilities=["mesh.rigid.v1"]
    if kind=="hair": capabilities.append("hair.authored.v1")
    if slug=="hearing-aids": capabilities.append("accessory.ear-device.v1")
    manifest.update({"schemaVersion":1,"asset":{
        "id":f"cuba:{kind}/{slug}.v1","kind":kind,"displayName":name,
        "rigProfile":"cuba:rig/biped15.v1","fitProfiles":["cuba:fit/person-standard.v1"],
        "supportedBases":["cuba:base/person.v1","cuba:base/person-02.v1"],
        "occupiedSlots":[slot],"coverage":[coverage],"conflicts":[],
        "materials":[m[0] for m in materials],"lod":{level:len(mesh.faces) for level,mesh in lods.items()},
        "requiredCapabilities":capabilities,"source":{"geometry":path.name},
        "provenance":{"source":"Cubacadabra starter-set/artwork","license":"Cubacadabra official"}},
        "geometry":{"file":path.name,"lodNodes":{level:level for level in lods}},
        "attachment":{"mode":"rigid","joint":"head","translation":[0,0,0],
                      "rotation":[0,0,0,1],"scale":[1,1,1]}})
    manifest_path.write_text(json.dumps(manifest,indent=2)+"\n")
    print(f"{name}: " + ", ".join(f"{level}={len(mesh.faces)} triangles" for level,mesh in lods.items()))
    return path


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only",nargs="+",choices=[spec[0] for spec in ASSETS])
    parser.add_argument("--blend",action="store_true",help="Save editable Blender sources as well")
    parser.add_argument("--wardrobe",action="store_true",help="Also rebuild the two bodies and six skinned clothing components")
    args=parser.parse_args()
    sculpted=[slug for slug in ("curls","coils") if not args.only or slug in args.only]
    blender=shutil.which("blender") or "/Applications/Blender.app/Contents/MacOS/Blender"
    if (sculpted or args.blend) and not Path(blender).is_file():
        raise SystemExit("Install Blender or put it on PATH to rebuild sculpted curls or .blend sources.")
    paths=[ROOT/"source/morphs/hair"/spec[0]/f"{spec[0]}.glb" if spec[0] in sculpted else build_asset(spec)
           for spec in ASSETS if not args.only or spec[0] in args.only]
    if sculpted:
        subprocess.run([blender,"--background","--factory-startup","--python",
                        str(ROOT/"artwork/sculpt_curls.py"),"--",*sculpted],check=True)
    if args.wardrobe:
        from artwork import clothing
        paths.extend(clothing.build())
    if args.blend:
        subprocess.run([blender,"--background","--factory-startup","--python",
                        str(ROOT/"artwork/save_blender.py"),"--",*[str(p) for p in paths]],check=True)


if __name__=="__main__": main()
